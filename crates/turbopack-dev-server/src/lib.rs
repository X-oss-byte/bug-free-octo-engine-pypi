#![feature(min_specialization)]
#![feature(trait_alias)]
#![feature(array_chunks)]

pub mod html;
mod http;
pub mod introspect;
pub mod source;
pub mod update;

use std::{
    future::Future,
    net::{SocketAddr, TcpListener},
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use hyper::{
    server::{conn::AddrIncoming, Builder},
    service::{make_service_fn, service_fn},
    Request, Response, Server,
};
use turbo_tasks::{
    run_once, trace::TraceRawVcs, util::FormatDuration, CollectiblesSource, RawVc,
    TransientInstance, TransientValue, TurboTasksApi,
};
use turbopack_core::issue::{IssueReporter, IssueReporterVc, IssueVc};

use self::{
    source::{ContentSource, ContentSourceResultVc, ContentSourceVc},
    update::UpdateServer,
};

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

async fn handle_issues<T: Into<RawVc> + CollectiblesSource + Copy>(
    source: T,
    path: &str,
    operation: &str,
    issue_reporter: IssueReporterVc,
) -> Result<()> {
    let issues = IssueVc::peek_issues_with_path(source)
        .await?
        .strongly_consistent()
        .await?;

    issue_reporter.report_issues(
        TransientInstance::new(issues.clone()),
        TransientValue::new(source.into()),
    );

    if issues.has_fatal().await? {
        Err(anyhow!("Fatal issue(s) occurred in {path} ({operation})"))
    } else {
        Ok(())
    }
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
        get_issue_reporter: Arc<dyn Fn() -> IssueReporterVc + Send + Sync>,
    ) -> DevServer {
        let make_svc = make_service_fn(move |_| {
            let tt = turbo_tasks.clone();
            let source_provider = source_provider.clone();
            let get_issue_reporter = get_issue_reporter.clone();
            async move {
                let handler = move |request: Request<hyper::Body>| {
                    let start = Instant::now();
                    let tt = tt.clone();
                    let get_issue_reporter = get_issue_reporter.clone();
                    let source_provider = source_provider.clone();
                    let future = async move {
                        run_once(tt.clone(), async move {
                            let issue_reporter = get_issue_reporter();
                            let source = source_provider.get_source();

                            if hyper_tungstenite::is_upgrade_request(&request) {
                                let base_path = source.base_path().await?;

                                let uri = request.uri();
                                let path = uri.path();

                                let path = if let Some(base_path) = base_path.as_deref() {
                                    if let Some(path) = path.strip_prefix(base_path) {
                                        path
                                    } else {
                                        return Ok(Response::builder()
                                            .status(404)
                                            .body(hyper::Body::empty())?);
                                    }
                                } else {
                                    path
                                };

                                if path == "/turbopack-hmr" {
                                    let (response, websocket) =
                                        hyper_tungstenite::upgrade(request, None)?;
                                    let update_server =
                                        UpdateServer::new(source_provider, issue_reporter);
                                    update_server.run(&*tt, websocket);
                                    return Ok(response);
                                }

                                println!("[404] {} (WebSocket)", path);
                                if path == "/_next/webpack-hmr" {
                                    // Special-case requests to webpack-hmr as these are made by
                                    // Next.js clients built
                                    // without turbopack, which may be making requests in
                                    // development.
                                    println!(
                                        "A non-turbopack next.js client is trying to connect."
                                    );
                                    println!(
                                        "Make sure to reload/close any browser window which has \
                                         been opened without --turbo."
                                    );
                                }

                                return Ok(Response::builder()
                                    .status(404)
                                    .body(hyper::Body::empty())?);
                            }

                            let uri = request.uri();
                            let path = uri.path().to_string();
                            let source = source_provider.get_source();
                            handle_issues(source, &path, "get source", issue_reporter).await?;
                            let resolved_source = source.resolve_strongly_consistent().await?;
                            let response = http::process_request_with_content_source(
                                resolved_source,
                                request,
                                issue_reporter,
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

pub fn register() {
    turbo_tasks::register();
    turbo_tasks_fs::register();
    turbopack_core::register();
    turbopack_cli_utils::register();
    turbopack_ecmascript::register();
    include!(concat!(env!("OUT_DIR"), "/register.rs"));
}
