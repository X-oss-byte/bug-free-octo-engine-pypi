#![feature(future_join)]
#![feature(min_specialization)]

pub mod devserver_options;
mod turbo_tasks_viz;

use std::{
    collections::HashSet,
    env::current_dir,
    future::{join, Future},
    net::{IpAddr, SocketAddr},
    path::MAIN_SEPARATOR,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use devserver_options::DevServerOptions;
use next_core::{
    create_app_source, create_server_rendered_source, create_web_entry_source, env::load_env,
    manifest::DevManifestContentSource, next_config::load_next_config,
    next_image::NextImageContentSourceVc,
};
use owo_colors::OwoColorize;
use turbo_malloc::TurboMalloc;
use turbo_tasks::{
    util::{FormatBytes, FormatDuration},
    RawVc, StatsType, TransientInstance, TransientValue, TurboTasks, TurboTasksBackendApi, Value,
};
use turbo_tasks_fs::{DiskFileSystemVc, FileSystemVc};
use turbo_tasks_memory::MemoryBackend;
use turbopack_cli_utils::issue::{ConsoleUi, ConsoleUiVc, LogOptions};
use turbopack_core::{
    environment::ServerAddr,
    issue::IssueSeverity,
    resolve::{parse::RequestVc, pattern::QueryMapVc},
};
use turbopack_dev_server::{
    fs::DevServerFileSystemVc,
    introspect::IntrospectionSource,
    source::{
        combined::CombinedContentSourceVc, router::RouterContentSource,
        source_maps::SourceMapContentSourceVc, static_assets::StaticAssetsContentSourceVc,
        ContentSourceVc,
    },
    DevServer,
};
use turbopack_node::{
    execution_context::ExecutionContextVc, source_map::NextSourceMapTraceContentSourceVc,
};

#[derive(Clone)]
pub enum EntryRequest {
    Relative(String),
    Module(String, String),
}

pub struct NextDevServerBuilder {
    turbo_tasks: Arc<TurboTasks<MemoryBackend>>,
    project_dir: String,
    root_dir: String,
    entry_requests: Vec<EntryRequest>,
    eager_compile: bool,
    hostname: Option<IpAddr>,
    port: Option<u16>,
    browserslist_query: String,
    log_level: IssueSeverity,
    show_all: bool,
    log_detail: bool,
    allow_retry: bool,
}

impl NextDevServerBuilder {
    pub fn new(
        turbo_tasks: Arc<TurboTasks<MemoryBackend>>,
        project_dir: String,
        root_dir: String,
    ) -> NextDevServerBuilder {
        NextDevServerBuilder {
            turbo_tasks,
            project_dir,
            root_dir,
            entry_requests: vec![],
            eager_compile: false,
            hostname: None,
            port: None,
            browserslist_query: "last 1 Chrome versions, last 1 Firefox versions, last 1 Safari \
                                 versions, last 1 Edge versions"
                .to_owned(),
            log_level: IssueSeverity::Warning,
            show_all: false,
            log_detail: false,
            allow_retry: false,
        }
    }

    pub fn entry_request(mut self, entry_asset_path: EntryRequest) -> NextDevServerBuilder {
        self.entry_requests.push(entry_asset_path);
        self
    }

    pub fn eager_compile(mut self, eager_compile: bool) -> NextDevServerBuilder {
        self.eager_compile = eager_compile;
        self
    }

    pub fn hostname(mut self, hostname: IpAddr) -> NextDevServerBuilder {
        self.hostname = Some(hostname);
        self
    }

    pub fn port(mut self, port: u16) -> NextDevServerBuilder {
        self.port = Some(port);
        self
    }

    pub fn browserslist_query(mut self, browserslist_query: String) -> NextDevServerBuilder {
        self.browserslist_query = browserslist_query;
        self
    }

    pub fn log_level(mut self, log_level: IssueSeverity) -> NextDevServerBuilder {
        self.log_level = log_level;
        self
    }

    pub fn show_all(mut self, show_all: bool) -> NextDevServerBuilder {
        self.show_all = show_all;
        self
    }

    pub fn allow_retry(mut self, allow_retry: bool) -> NextDevServerBuilder {
        self.allow_retry = allow_retry;
        self
    }

    pub fn log_detail(mut self, log_detail: bool) -> NextDevServerBuilder {
        self.log_detail = log_detail;
        self
    }

    pub async fn build(self) -> Result<DevServer> {
        let start_port = self.port.context("port must be set")?;
        let host = self.hostname.context("hostname must be set")?;

        // Retry to listen on the different port if the port is already in use.
        let mut bound_server = None;
        for retry_count in 0..10 {
            let current_port = start_port + retry_count;
            let addr = SocketAddr::new(host, current_port);

            let listen_result = DevServer::listen(addr);

            match listen_result {
                Ok(server) => {
                    bound_server = Some(Ok(server));
                    break;
                }
                Err(e) => {
                    let should_retry = if self.allow_retry {
                        // Returned error from `listen` is not `std::io::Error` but `anyhow::Error`,
                        // so we need to access its source to check if it is
                        // `std::io::ErrorKind::AddrInUse`.
                        e.source()
                            .map(|e| {
                                e.source()
                                    .map(|e| {
                                        e.downcast_ref::<std::io::Error>()
                                            .map(|e| e.kind() == std::io::ErrorKind::AddrInUse)
                                            == Some(true)
                                    })
                                    .unwrap_or_else(|| false)
                            })
                            .unwrap_or_else(|| false)
                    } else {
                        false
                    };

                    if should_retry {
                        println!(
                            "{} - Port {} is in use, trying {} instead",
                            "warn ".yellow(),
                            current_port,
                            current_port + 1
                        );
                    } else {
                        bound_server = Some(Err(e));
                        break;
                    }
                }
            }
        }

        let server = bound_server.unwrap()?;

        let turbo_tasks = self.turbo_tasks;
        let project_dir = self.project_dir;
        let root_dir = self.root_dir;
        let eager_compile = self.eager_compile;
        let show_all = self.show_all;
        let log_detail = self.log_detail;
        let browserslist_query = self.browserslist_query;
        let log_options = LogOptions {
            current_dir: current_dir().unwrap(),
            show_all,
            log_detail,
            log_level: self.log_level,
        };
        let entry_requests = Arc::new(self.entry_requests);
        let console_ui = Arc::new(ConsoleUi::new(log_options));
        let console_ui_to_dev_server = console_ui.clone();
        let server_addr = Arc::new(server.addr);
        let tasks = turbo_tasks.clone();
        let source = move || {
            source(
                root_dir.clone(),
                project_dir.clone(),
                entry_requests.clone().into(),
                eager_compile,
                turbo_tasks.clone().into(),
                console_ui.clone().into(),
                browserslist_query.clone(),
                server_addr.clone().into(),
            )
        };

        Ok(server.serve(tasks, source, console_ui_to_dev_server))
    }
}

async fn handle_issues<T: Into<RawVc>>(source: T, console_ui: ConsoleUiVc) -> Result<()> {
    let state = console_ui
        .group_and_display_issues(TransientValue::new(source.into()))
        .await?;

    if state.has_fatal {
        Err(anyhow!("Fatal issue(s) occurred"))
    } else {
        Ok(())
    }
}

#[turbo_tasks::function]
async fn project_fs(project_dir: &str, console_ui: ConsoleUiVc) -> Result<FileSystemVc> {
    let disk_fs = DiskFileSystemVc::new("project".to_string(), project_dir.to_string());
    handle_issues(disk_fs, console_ui).await?;
    disk_fs.await?.start_watching()?;
    Ok(disk_fs.into())
}

#[turbo_tasks::function]
async fn output_fs(project_dir: &str, console_ui: ConsoleUiVc) -> Result<FileSystemVc> {
    let disk_fs = DiskFileSystemVc::new("output".to_string(), project_dir.to_string());
    handle_issues(disk_fs, console_ui).await?;
    disk_fs.await?.start_watching()?;
    Ok(disk_fs.into())
}

#[allow(clippy::too_many_arguments)]
#[turbo_tasks::function]
async fn source(
    root_dir: String,
    project_dir: String,
    entry_requests: TransientInstance<Vec<EntryRequest>>,
    eager_compile: bool,
    turbo_tasks: TransientInstance<TurboTasks<MemoryBackend>>,
    console_ui: TransientInstance<ConsoleUi>,
    browserslist_query: String,
    server_addr: TransientInstance<SocketAddr>,
) -> Result<ContentSourceVc> {
    let console_ui = (*console_ui).clone().cell();
    let output_fs = output_fs(&project_dir, console_ui);
    let fs = project_fs(&root_dir, console_ui);
    let project_relative = project_dir.strip_prefix(&root_dir).unwrap();
    let project_relative = project_relative
        .strip_prefix(MAIN_SEPARATOR)
        .unwrap_or(project_relative);
    let project_path = fs.root().join(project_relative);

    let env = load_env(project_path);
    let build_output_root = output_fs.root().join(".next/build");

    let execution_context = ExecutionContextVc::new(project_path, build_output_root);

    let next_config = load_next_config(execution_context.join("next_config"));

    let output_root = output_fs.root().join(".next/server");
    let server_addr = ServerAddr::new(*server_addr).cell();

    let dev_server_fs = DevServerFileSystemVc::new().as_file_system();
    let dev_server_root = dev_server_fs.root();
    let entry_requests = entry_requests
        .iter()
        .map(|r| match r {
            EntryRequest::Relative(p) => RequestVc::relative(Value::new(p.clone().into()), false),
            EntryRequest::Module(m, p) => {
                RequestVc::module(m.clone(), Value::new(p.clone().into()), QueryMapVc::none())
            }
        })
        .collect();

    let web_source = create_web_entry_source(
        project_path,
        execution_context,
        entry_requests,
        dev_server_root,
        env,
        eager_compile,
        &browserslist_query,
        next_config,
    );
    let rendered_source = create_server_rendered_source(
        project_path,
        execution_context,
        output_root.join("pages"),
        dev_server_root,
        env,
        &browserslist_query,
        next_config,
        server_addr,
    );
    let app_source = create_app_source(
        project_path,
        execution_context,
        output_root.join("app"),
        dev_server_root,
        env,
        &browserslist_query,
        next_config,
        server_addr,
    );
    let viz = turbo_tasks_viz::TurboTasksSource {
        turbo_tasks: turbo_tasks.into(),
    }
    .cell()
    .into();
    let static_source =
        StaticAssetsContentSourceVc::new(String::new(), project_path.join("public")).into();
    let manifest_source = DevManifestContentSource {
        page_roots: vec![app_source, rendered_source],
    }
    .cell()
    .into();
    let main_source = CombinedContentSourceVc::new(vec![
        manifest_source,
        static_source,
        app_source,
        rendered_source,
        web_source,
    ]);
    let introspect = IntrospectionSource {
        roots: HashSet::from([main_source.into()]),
    }
    .cell()
    .into();
    let main_source = main_source.into();
    let source_maps = SourceMapContentSourceVc::new(main_source).into();
    let source_map_trace = NextSourceMapTraceContentSourceVc::new(main_source).into();
    let img_source = NextImageContentSourceVc::new(
        CombinedContentSourceVc::new(vec![static_source, rendered_source]).into(),
    )
    .into();
    let source = RouterContentSource {
        routes: vec![
            ("__turbopack__/".to_string(), introspect),
            ("__turbo_tasks__/".to_string(), viz),
            (
                "__nextjs_original-stack-frame".to_string(),
                source_map_trace,
            ),
            // TODO: Load path from next.config.js
            ("_next/image".to_string(), img_source),
            ("__turbopack_sourcemap__/".to_string(), source_maps),
        ],
        fallback: main_source,
    }
    .cell()
    .into();

    handle_issues(dev_server_fs, console_ui).await?;
    handle_issues(web_source, console_ui).await?;
    handle_issues(rendered_source, console_ui).await?;

    Ok(source)
}

pub fn register() {
    next_core::register();
    include!(concat!(env!("OUT_DIR"), "/register.rs"));
}

/// Start a devserver with the given options.
pub async fn start_server(options: &DevServerOptions) -> Result<()> {
    let start = Instant::now();

    #[cfg(feature = "tokio_console")]
    console_subscriber::init();
    register();

    let dir = options
        .dir
        .as_ref()
        .map(|dir| dir.canonicalize())
        .unwrap_or_else(current_dir)
        .context("project directory can't be found")?
        .to_str()
        .context("project directory contains invalid characters")?
        .to_string();

    let root_dir = if let Some(root) = options.root.as_ref() {
        root.canonicalize()
            .context("root directory can't be found")?
            .to_str()
            .context("root directory contains invalid characters")?
            .to_string()
    } else {
        dir.clone()
    };

    let tt = TurboTasks::new(MemoryBackend::new());

    let stats_type = match options.full_stats {
        true => StatsType::Full,
        false => StatsType::Essential,
    };
    tt.set_stats_type(stats_type);

    let tt_clone = tt.clone();

    #[allow(unused_mut)]
    let mut server = NextDevServerBuilder::new(tt, dir, root_dir)
        .entry_request(EntryRequest::Relative("src/index".into()))
        .eager_compile(options.eager_compile)
        .hostname(options.hostname)
        .port(options.port)
        .log_detail(options.log_detail)
        .show_all(options.show_all)
        .log_level(
            options
                .log_level
                .map_or_else(|| IssueSeverity::Warning, |l| l.0),
        );

    #[cfg(feature = "serializable")]
    {
        server = server.allow_retry(options.allow_retry);
    }

    let server = server.build().await?;

    {
        let index_uri = ServerAddr::new(server.addr).to_string()?;
        println!(
            "{} - started server on {}:{}, url: {}",
            "ready".green(),
            server.addr.ip(),
            server.addr.port(),
            index_uri
        );
        if !options.no_open {
            let _ = webbrowser::open(&index_uri);
        }
    }

    let stats_future = async move {
        if options.log_detail {
            println!(
                "{event_type} - initial compilation {start} ({memory})",
                event_type = "event".purple(),
                start = FormatDuration(start.elapsed()),
                memory = FormatBytes(TurboMalloc::memory_usage())
            );
        } else {
            println!(
                "{event_type} - initial compilation {start}",
                event_type = "event".purple(),
                start = FormatDuration(start.elapsed()),
            );
        }

        loop {
            let update_future = profile_timeout(
                tt_clone.as_ref(),
                tt_clone.get_or_wait_update_info(Duration::from_millis(100)),
            );

            let (elapsed, count) = update_future.await;
            if options.log_detail {
                println!(
                    "{event_type} - updated in {elapsed} ({tasks} tasks, {memory})",
                    event_type = "event".purple(),
                    elapsed = FormatDuration(elapsed),
                    tasks = count,
                    memory = FormatBytes(TurboMalloc::memory_usage())
                );
            } else {
                println!(
                    "{event_type} - updated in {elapsed}",
                    event_type = "event".purple(),
                    elapsed = FormatDuration(elapsed),
                );
            }
        }
    };

    join!(stats_future, async { server.future.await.unwrap() }).await;

    Ok(())
}

#[cfg(feature = "profile")]
// When profiling, exits the process when no new updates have been received for
// a given timeout and there are no more tasks in progress.
async fn profile_timeout<T>(tt: &TurboTasks<MemoryBackend>, future: impl Future<Output = T>) -> T {
    /// How long to wait in between updates before force-exiting the process
    /// during profiling.
    const PROFILE_EXIT_TIMEOUT: Duration = Duration::from_secs(5);

    futures::pin_mut!(future);
    loop {
        match tokio::time::timeout(PROFILE_EXIT_TIMEOUT, &mut future).await {
            Ok(res) => return res,
            Err(_) => {
                if tt.get_in_progress_count() == 0 {
                    std::process::exit(0)
                }
            }
        }
    }
}

#[cfg(not(feature = "profile"))]
fn profile_timeout<T>(
    _tt: &TurboTasks<MemoryBackend>,
    future: impl Future<Output = T>,
) -> impl Future<Output = T> {
    future
}
