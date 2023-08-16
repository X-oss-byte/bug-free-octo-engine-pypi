use std::{collections::HashSet, sync::Arc};

use futures::Future;
use log::debug;
use notify::{Config, RecommendedWatcher, Watcher};
use tokio::{
    join,
    sync::{
        oneshot::{Receiver, Sender},
        Mutex,
    },
};
use tonic::transport::{NamedService, Server};

use super::proto::{self};
use crate::{commands::CommandBase, get_version};

pub struct DaemonServer {
    repo_root: std::path::PathBuf,
    daemon_root: std::path::PathBuf,
    log_file: std::path::PathBuf,

    timeout: std::time::Duration,
    start_time: std::time::Instant,
    watcher: Arc<crate::globwatcher::GlobWatcher>,

    shutdown: Mutex<Option<Sender<()>>>,
}

impl DaemonServer {
    pub fn new(base: &CommandBase, timeout: std::time::Duration) -> Self {
        let repo_root = base.repo_root.clone();

        let watcher = Arc::new(crate::globwatcher::GlobWatcher::new(repo_root.clone()));

        let dirs =
            directories::ProjectDirs::from_path("turborepo".into()).expect("user has a home dir");
        let log_file = dirs.data_dir().join("logs").join("HASH-turbo.log");

        Self {
            repo_root,
            daemon_root: base.daemon_file_root(),
            log_file,
            timeout,
            start_time: std::time::Instant::now(),
            shutdown: Mutex::new(None),
            watcher,
        }
    }

    fn with_shutdown(mut self) -> (DaemonServer, Receiver<()>) {
        let (send_shutdown, recv_shutdown) = tokio::sync::oneshot::channel::<()>();
        self.shutdown = Mutex::new(Some(send_shutdown));
        (self, recv_shutdown)
    }

    /// Serve the daemon server, while also watching for filesystem changes.
    pub async fn serve(self) {
        let (server, recv_shutdown) = self.with_shutdown();

        let watcher = server.watcher.clone();
        let watch_fut = watcher.watch();

        let stream = crate::daemon::endpoint::open_socket(server.daemon_root.clone())
            .await
            .unwrap();

        let server_fut = Server::builder()
            .add_service(crate::daemon::proto::turbod_server::TurbodServer::new(
                server,
            ))
            .serve_with_incoming_shutdown(stream, async { recv_shutdown.await.unwrap() });

        let (a, b) = join!(server_fut, watch_fut);
    }
}

#[tonic::async_trait]
impl proto::turbod_server::Turbod for DaemonServer {
    async fn hello(
        &self,
        request: tonic::Request<proto::HelloRequest>,
    ) -> Result<tonic::Response<proto::HelloResponse>, tonic::Status> {
        if request.into_inner().version != get_version() {
            return Err(tonic::Status::unimplemented("version mismatch"));
        } else {
            Ok(tonic::Response::new(proto::HelloResponse {}))
        }
    }

    async fn shutdown(
        &self,
        _request: tonic::Request<proto::ShutdownRequest>,
    ) -> Result<tonic::Response<proto::ShutdownResponse>, tonic::Status> {
        self.shutdown.lock().await.take().map(|s| s.send(()));

        // if Some(Ok), then the server is shutting down now
        // if Some(Err), then the server is already shutting down
        // if None, then someone has already called shutdown
        Ok(tonic::Response::new(proto::ShutdownResponse {}))
    }

    async fn status(
        &self,
        _request: tonic::Request<proto::StatusRequest>,
    ) -> Result<tonic::Response<proto::StatusResponse>, tonic::Status> {
        Ok(tonic::Response::new(proto::StatusResponse {
            daemon_status: Some(proto::DaemonStatus {
                uptime_msec: self.start_time.elapsed().as_millis() as u64,
                log_file: self.log_file.to_str().unwrap().to_string(),
            }),
        }))
    }

    async fn notify_outputs_written(
        &self,
        request: tonic::Request<proto::NotifyOutputsWrittenRequest>,
    ) -> Result<tonic::Response<proto::NotifyOutputsWrittenResponse>, tonic::Status> {
        let inner = request.into_inner();
        self.watcher.watch_globs(
            inner.hash,
            HashSet::from_iter(inner.output_globs),
            HashSet::from_iter(inner.output_exclusion_globs),
        );

        Ok(tonic::Response::new(proto::NotifyOutputsWrittenResponse {}))
    }

    async fn get_changed_outputs(
        &self,
        request: tonic::Request<proto::GetChangedOutputsRequest>,
    ) -> Result<tonic::Response<proto::GetChangedOutputsResponse>, tonic::Status> {
        let inner = request.into_inner();
        let changed = self
            .watcher
            .changed_globs(&inner.hash, HashSet::from_iter(inner.output_globs));

        Ok(tonic::Response::new(proto::GetChangedOutputsResponse {
            changed_output_globs: changed.into_iter().collect(),
        }))
    }
}

impl NamedService for DaemonServer {
    const NAME: &'static str = "turborepo.Daemon";
}
