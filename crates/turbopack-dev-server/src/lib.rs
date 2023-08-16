#![feature(min_specialization)]
#![feature(trait_alias)]
#![feature(array_chunks)]

pub mod html;
pub mod introspect;
pub mod source;
pub mod update;

use std::{
    collections::btree_map::Entry,
    future::Future,
    net::{SocketAddr, TcpListener},
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use futures::{StreamExt, TryStreamExt};
use hyper::{
    header::HeaderName,
    server::{conn::AddrIncoming, Builder},
    service::{make_service_fn, service_fn},
    Request, Response, Server, Uri,
};
use mime_guess::mime;
use source::{
    headers::{HeaderValue, Headers},
    Body, Bytes,
};
use turbo_tasks::{
    run_once, trace::TraceRawVcs, util::FormatDuration, RawVc, TransientValue, TurboTasksApi, Value,
};
use turbo_tasks_fs::{FileContent, FileContentReadRef};
use turbopack_cli_utils::issue::{ConsoleUi, ConsoleUiVc};
use turbopack_core::asset::AssetContent;

use self::{
    source::{
        query::Query, ContentSourceContent, ContentSourceDataVary, ContentSourceResult,
        ContentSourceResultVc, ContentSourceVc, ProxyResultReadRef,
    },
    update::{protocol::ResourceIdentifier, UpdateServer},
};
use crate::source::ContentSourceData;

pub trait SourceProvider: Send + Clone + 'static {
    /// must call a turbo-tasks function internally
    fn get_source(&self) -> ContentSourceVc;
}

pub trait ContentProvider: Send + Clone + 'static {
    fn get_content(&self) -> ContentSourceResultVc;
}

impl<T> SourceProvider for T
where
    T: Fn() -> ContentSourceVc + Send + Clone + 'static,
{
    fn get_source(&self) -> ContentSourceVc {
        self()
    }
}

#[derive(TraceRawVcs, Debug)]
pub struct DevServerBuilder {
    #[turbo_tasks(trace_ignore)]
    pub addr: SocketAddr,
    #[turbo_tasks(trace_ignore)]
    server: Builder<AddrIncoming>,
}

#[derive(TraceRawVcs)]
pub struct DevServer {
    #[turbo_tasks(trace_ignore)]
    pub addr: SocketAddr,
    #[turbo_tasks(trace_ignore)]
    pub future: Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>>,
}

// Just print issues to console for now...
async fn handle_issues<T: Into<RawVc>>(
    source: T,
    path: &str,
    operation: &str,
    console_ui: ConsoleUiVc,
) -> Result<()> {
    let state = console_ui
        .group_and_display_issues(TransientValue::new(source.into()))
        .await?;
    if state.has_fatal {
        bail!("Fatal issue(s) occurred in {path} ({operation}")
    }

    Ok(())
}

#[turbo_tasks::value]
pub enum GetFromSourceResult {
    Static {
        content: FileContentReadRef,
        status_code: u16,
        headers: Vec<(String, String)>,
    },
    HttpProxy(ProxyResultReadRef),
    NotFound,
}

pub async fn get_from_source(
    path: &str,
    source: ContentSourceVc,
    mut request: Request<hyper::Body>,
    console_ui: ConsoleUiVc,
) -> Result<GetFromSourceResultVc> {
    let mut data = ContentSourceData::default();
    let mut current_source = source;
    // Remove leading slash.
    let mut current_asset_path = urlencoding::decode(&path[1..])?.into_owned();
    loop {
        let result = current_source.get(&current_asset_path, Value::new(data));
        handle_issues(result, path, "get content from source", console_ui).await?;

        let get_result = match &*result.strongly_consistent().await? {
            ContentSourceResult::NotFound => GetFromSourceResult::NotFound,
            ContentSourceResult::NeedData(needed) => {
                current_source = needed.source.resolve().await?;
                current_asset_path = needed.path.clone();
                data = request_to_data(&mut request, &needed.vary).await?;
                continue;
            }
            ContentSourceResult::Result { get_content, .. } => {
                let content_vary = get_content.vary().await?;
                let content_data = request_to_data(&mut request, &content_vary).await?;
                let content = get_content.get(Value::new(content_data)).await?;
                match &*content {
                    ContentSourceContent::Rewrite(rewrite) => {
                        let rewrite = rewrite.await?;
                        // If a source isn't specified, we restart at the top.
                        let new_source = rewrite.source.unwrap_or(source);
                        let new_uri = Uri::try_from(&rewrite.path_and_query)?;
                        if new_source == current_source && new_uri == *request.uri() {
                            bail!("rewrite loop detected: {}", new_uri);
                        }
                        let new_asset_path =
                            urlencoding::decode(&request.uri().path()[1..])?.into_owned();

                        current_source = new_source;
                        *request.uri_mut() = new_uri;
                        current_asset_path = new_asset_path;
                        data = ContentSourceData::default();
                        continue;
                    }
                    ContentSourceContent::Static {
                        content: content_vc,
                        status_code,
                        headers,
                    } => {
                        if let AssetContent::File(file) = &*content_vc.content().await? {
                            GetFromSourceResult::Static {
                                content: file.await?,
                                status_code: *status_code,
                                headers: headers.await?.clone(),
                            }
                        } else {
                            GetFromSourceResult::NotFound
                        }
                    }
                    ContentSourceContent::HttpProxy(proxy) => {
                        GetFromSourceResult::HttpProxy(proxy.await?)
                    }
                    ContentSourceContent::NotFound => GetFromSourceResult::NotFound,
                }
            }
        };

        return Ok(get_result.cell());
    }
}

async fn process_request_with_content_source(
    path: &str,
    source: ContentSourceVc,
    request: Request<hyper::Body>,
    console_ui: ConsoleUiVc,
) -> Result<Response<hyper::Body>> {
    let result = get_from_source(path, source, request, console_ui)
        .await?
        .await?;
    match &*result {
        GetFromSourceResult::Static {
            content: file,
            status_code,
            headers,
        } => {
            if let FileContent::Content(content) = &**file {
                let mut response = Response::builder().status(*status_code);

                let header_map = response.headers_mut().expect("headers must be defined");

                for (header_name, header_value) in headers {
                    header_map.append(
                        HeaderName::try_from(header_name.clone())?,
                        hyper::header::HeaderValue::try_from(header_value.as_str())?,
                    );
                }

                if let Some(content_type) = content.content_type() {
                    header_map.append(
                        "content-type",
                        hyper::header::HeaderValue::try_from(content_type.to_string())?,
                    );
                } else if let hyper::header::Entry::Vacant(entry) = header_map.entry("content-type")
                {
                    let guess = mime_guess::from_path(path).first_or_octet_stream();
                    // If a text type, application/javascript, or application/json was
                    // guessed, use a utf-8 charset as  we most likely generated it as
                    // such.
                    entry.insert(hyper::header::HeaderValue::try_from(
                        if (guess.type_() == mime::TEXT
                            || guess.subtype() == mime::JAVASCRIPT
                            || guess.subtype() == mime::JSON)
                            && guess.get_param("charset").is_none()
                        {
                            guess.to_string() + "; charset=utf-8"
                        } else {
                            guess.to_string()
                        },
                    )?);
                }

                let content = content.content();
                header_map.insert(
                    "Content-Length",
                    hyper::header::HeaderValue::try_from(content.len().to_string())?,
                );

                let bytes = content.read();
                return Ok(response.body(hyper::Body::wrap_stream(bytes))?);
            }
        }
        GetFromSourceResult::HttpProxy(proxy_result) => {
            let mut response = Response::builder().status(proxy_result.status);
            let headers = response.headers_mut().expect("headers must be defined");

            for [name, value] in proxy_result.headers.array_chunks() {
                headers.append(
                    HeaderName::from_bytes(name.as_bytes())?,
                    hyper::header::HeaderValue::from_str(value)?,
                );
            }

            return Ok(response.body(hyper::Body::wrap_stream(proxy_result.body.read()))?);
        }
        _ => {}
    }

    Ok(Response::builder().status(404).body(hyper::Body::empty())?)
}

impl DevServer {
    pub fn listen(addr: SocketAddr) -> Result<DevServerBuilder, anyhow::Error> {
        // This is annoying. The hyper::Server doesn't allow us to know which port was
        // bound (until we build it with a request handler) when using the standard
        // `server::try_bind` approach. This is important when binding the `0` port,
        // because the OS will remap that to an actual free port, and we need to know
        // that port before we build the request handler. So we need to construct a
        // real TCP listener, see if it bound, and get its bound address.
        let listener = TcpListener::bind(addr).context("not able to bind address")?;
        let addr = listener
            .local_addr()
            .context("not able to get bound address")?;

        let server = Server::from_tcp(listener).context("Not able to start server")?;
        Ok(DevServerBuilder { addr, server })
    }
}

impl DevServerBuilder {
    pub fn serve(
        self,
        turbo_tasks: Arc<dyn TurboTasksApi>,
        source_provider: impl SourceProvider + Clone + Send + Sync,
        console_ui: Arc<ConsoleUi>,
    ) -> DevServer {
        let make_svc = make_service_fn(move |_| {
            let tt = turbo_tasks.clone();
            let source_provider = source_provider.clone();
            let console_ui = console_ui.clone();
            async move {
                let handler = move |request: Request<hyper::Body>| {
                    let console_ui = console_ui.clone();
                    let start = Instant::now();
                    let tt = tt.clone();
                    let source_provider = source_provider.clone();
                    let future = async move {
                        if hyper_tungstenite::is_upgrade_request(&request) {
                            let uri = request.uri();
                            let path = uri.path();

                            if path == "/turbopack-hmr" {
                                let (response, websocket) =
                                    hyper_tungstenite::upgrade(request, None)?;
                                let update_server = UpdateServer::new(source_provider);
                                update_server.run(&*tt, websocket);
                                return Ok(response);
                            }

                            println!("[404] {} (WebSocket)", path);
                            if path == "/_next/webpack-hmr" {
                                // Special-case requests to webpack-hmr as these are made by Next.js
                                // clients built without turbopack, which may be making requests in
                                // development.
                                println!("A non-turbopack next.js client is trying to connect.");
                                println!(
                                    "Make sure to reload/close any browser window which has been \
                                     opened without --turbo."
                                );
                            }

                            return Ok(Response::builder()
                                .status(404)
                                .body(hyper::Body::empty())?);
                        }

                        run_once(tt, async move {
                            let console_ui = (*console_ui).clone().cell();
                            let uri = request.uri();
                            let path = uri.path().to_string();
                            let source = source_provider.get_source();
                            handle_issues(source, &path, "get source", console_ui).await?;
                            let resolved_source = source.resolve_strongly_consistent().await?;
                            let response = process_request_with_content_source(
                                &path,
                                resolved_source,
                                request,
                                console_ui,
                            )
                            .await?;
                            let status = response.status().as_u16();
                            let is_error = response.status().is_client_error()
                                || response.status().is_server_error();
                            let elapsed = start.elapsed();
                            if is_error
                                || (cfg!(feature = "log_request_stats")
                                    && elapsed > Duration::from_secs(1))
                            {
                                println!(
                                    "[{status}] {path} ({duration})",
                                    duration = FormatDuration(elapsed)
                                );
                            }
                            Ok(response)
                        })
                        .await
                    };
                    async move {
                        match future.await {
                            Ok(r) => Ok::<_, hyper::http::Error>(r),
                            Err(e) => {
                                println!(
                                    "[500] error: {:?} ({})",
                                    e,
                                    FormatDuration(start.elapsed())
                                );
                                Ok(Response::builder()
                                    .status(500)
                                    .body(hyper::Body::from(format!("{:?}", e,)))?)
                            }
                        }
                    }
                };
                anyhow::Ok(service_fn(handler))
            }
        });
        let server = self.server.serve(make_svc);

        DevServer {
            addr: self.addr,
            future: Box::pin(async move {
                server.await?;
                Ok(())
            }),
        }
    }
}

static CACHE_BUSTER: AtomicU64 = AtomicU64::new(0);

async fn request_to_data(
    request: &mut Request<hyper::Body>,
    vary: &ContentSourceDataVary,
) -> Result<ContentSourceData> {
    let mut data = ContentSourceData::default();
    if vary.method {
        data.method = Some(request.method().to_string());
    }
    if vary.url {
        data.url = Some(request.uri().to_string());
    }
    if vary.body {
        let bytes: Vec<_> = request
            .body_mut()
            .map(|bytes| bytes.map(Bytes::from))
            .try_collect::<Vec<_>>()
            .await?;
        data.body = Some(Body::new(bytes).into());
    }
    if let Some(filter) = vary.query.as_ref() {
        if let Some(query) = request.uri().query() {
            let mut query: Query = serde_qs::from_str(query)?;
            query.filter_with(filter);
            data.query = Some(query);
        } else {
            data.query = Some(Query::default())
        }
    }
    if let Some(filter) = vary.headers.as_ref() {
        let mut headers = Headers::default();
        for (header_name, header_value) in request.headers().iter() {
            if !filter.contains(header_name.as_str()) {
                continue;
            }
            match headers.entry(header_name.to_string()) {
                Entry::Vacant(e) => {
                    if let Ok(s) = header_value.to_str() {
                        e.insert(HeaderValue::SingleString(s.to_string()));
                    } else {
                        e.insert(HeaderValue::SingleBytes(header_value.as_bytes().to_vec()));
                    }
                }
                Entry::Occupied(mut e) => {
                    if let Ok(s) = header_value.to_str() {
                        e.get_mut().extend_with_string(s.to_string());
                    } else {
                        e.get_mut()
                            .extend_with_bytes(header_value.as_bytes().to_vec());
                    }
                }
            }
        }
        data.headers = Some(headers);
    }
    if vary.cache_buster {
        data.cache_buster = CACHE_BUSTER.fetch_add(1, Ordering::SeqCst);
    }
    Ok(data)
}

pub(crate) fn resource_to_data(
    resource: ResourceIdentifier,
    vary: &ContentSourceDataVary,
) -> ContentSourceData {
    let mut data = ContentSourceData::default();
    if vary.method {
        data.method = Some("GET".to_string());
    }
    if vary.url {
        data.url = Some(resource.path);
    }
    if vary.body {
        data.body = Some(Body::new(Vec::new()).into());
    }
    if vary.query.is_some() {
        data.query = Some(Query::default())
    }
    if let Some(filter) = vary.headers.as_ref() {
        let mut headers = Headers::default();
        if let Some(resource_headers) = resource.headers {
            for (header_name, header_value) in resource_headers {
                if !filter.contains(header_name.as_str()) {
                    continue;
                }
                match headers.entry(header_name) {
                    Entry::Vacant(e) => {
                        e.insert(HeaderValue::SingleString(header_value));
                    }
                    Entry::Occupied(mut e) => {
                        e.get_mut().extend_with_string(header_value);
                    }
                }
            }
        }
        data.headers = Some(headers);
    }
    if vary.cache_buster {
        data.cache_buster = CACHE_BUSTER.fetch_add(1, Ordering::SeqCst);
    }
    data
}

pub fn register() {
    turbo_tasks::register();
    turbo_tasks_fs::register();
    turbopack_core::register();
    turbopack_cli_utils::register();
    turbopack_ecmascript::register();
    include!(concat!(env!("OUT_DIR"), "/register.rs"));
}
