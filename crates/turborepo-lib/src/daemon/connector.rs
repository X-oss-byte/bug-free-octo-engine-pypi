use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use command_group::AsyncCommandGroup;
use log::{debug, error};
use notify::{Config, Event, RecommendedWatcher, Watcher};
use same_file::Handle;
use tokio::{net::UnixStream, sync::mpsc};
use tonic::transport::Endpoint;

use super::{client::proto::turbod_client::TurbodClient, DaemonClient, DaemonError};

#[derive(Debug)]
pub struct DaemonConnector {
    pub dont_start: bool,
    pub dont_kill: bool,
    pub pid_file: PathBuf,
    pub sock_file: PathBuf,
}

impl DaemonConnector {
    const CONNECT_RETRY_MAX: usize = 3;

    /// Attempt, with retries, to:
    /// 1. find (or start) the daemon process
    /// 2. locate its unix socket
    /// 3. connect to the socket
    /// 4. send the 'hello' message, negotiating versions
    ///
    /// A new server will be spawned (and the old one killed) if
    /// dont_kill is unset and one of these cases is hit:
    /// 1. the versions do not match
    /// 2. the server is not running
    /// 3. the server is unresponsive
    pub async fn connect(self) -> Result<DaemonClient<DaemonConnector>, DaemonError> {
        for _ in 0..Self::CONNECT_RETRY_MAX {
            let pid = self.get_or_start_daemon().await?;
            debug!("got daemon with pid: {}", pid);

            let path = match self.wait_for_socket().await {
                Ok(p) => p,
                Err(_) => continue,
            };

            let conn = Self::get_connection(path.into()).await?;
            let mut client = DaemonClient {
                client: conn,
                connect_settings: (),
            };

            match (client.handshake().await, &self.dont_kill) {
                (Ok(_), _) => return Ok(client.with_connect_settings(self)),
                // should be able to opt out of kill
                (Err(DaemonError::VersionMismatch), true) => {
                    return Err(DaemonError::VersionMismatch)
                }
                (Err(DaemonError::VersionMismatch), false) => {
                    self.kill_live_server(pid)?;
                    continue;
                }
                (Err(DaemonError::Connection), _) => {
                    self.kill_dead_server(pid)?;
                    continue;
                }
                // unhandled error
                (Err(e), _) => return Err(e),
            };
        }

        Err(DaemonError::Connection)
    }

    /// Gets the PID of the daemon process.
    ///
    /// If a daemon is not running, it starts one.
    async fn get_or_start_daemon(&self) -> Result<u32, DaemonError> {
        debug!("looking for pid in lockfile: {:?}", self.pid_file);

        // avoid allocating again
        let pidfile = pidlock::Pidlock::new(self.pid_file.to_str().ok_or(DaemonError::PidFile)?);

        match pidfile.get_owner() {
            Some(pid) => {
                debug!("found pid: {}", pid);
                Ok(pid)
            }
            None => {
                debug!("no pid found, starting daemon");
                Self::start_daemon().await
            }
        }
    }

    /// Starts the daemon process, returning its PID.
    async fn start_daemon() -> Result<u32, DaemonError> {
        let mut group =
            tokio::process::Command::new(std::env::current_exe().map_err(|_| DaemonError::Fork)?)
                .arg("daemon")
                .group_spawn()
                .map_err(|_| DaemonError::Fork)?;

        group.inner().id().ok_or(DaemonError::Fork)
    }

    async fn get_connection(
        path: PathBuf,
    ) -> Result<TurbodClient<tonic::transport::Channel>, DaemonError> {
        debug!("connecting to socket: {}", path.to_string_lossy());
        let arc = Arc::new(path);

        // note, this path is just a dummy. the actual path is passed in
        let channel = match Endpoint::try_from("http://[::]:50051")?
            .connect_with_connector(tower::service_fn(move |_| {
                // we clone the reference counter here and move it into the async closure
                let arc = arc.clone();
                async move { UnixStream::connect::<&Path>(arc.as_path().into()).await }
            }))
            .await
        {
            Ok(c) => c,
            Err(e) => {
                error!("failed to connect to socket: {}", e);
                return Err(DaemonError::Connection);
            }
        };

        Ok(TurbodClient::new(channel))
    }

    ///
    fn kill_live_server(&self, pid: u32) -> Result<(), DaemonError> {
        // call shutdown
        // if fail
        // - kill_dead_server on fail
        // else
        // - check for lockfile
        // - once deleted, cleanup complete
        todo!()
    }

    ///
    fn kill_dead_server(&self, pid: u32) -> Result<(), DaemonError> {
        todo!()
    }

    async fn wait_for_socket(&self) -> Result<&Path, DaemonError> {
        match tokio::time::timeout(
            Duration::from_secs(100),
            wait_for_file(self.sock_file.clone()),
        )
        .await
        {
            Ok(Ok(_)) => Ok(self.sock_file.as_path()),
            Ok(Err(e)) => {
                debug!(
                    "error '{:?}' when waiting for socket: {:?}",
                    e, self.sock_file
                );
                Err(DaemonError::Socket)
            }
            Err(_) => {
                debug!("timeout when waiting for socket: {:?}", self.sock_file);
                Err(DaemonError::Socket)
            }
        }
    }
}

/// Waits for a file to appear on the filesystem,
/// by listening for create events on the parent directory.
///
/// This is an alternative to polling the filesystem.
async fn wait_for_file(path: PathBuf) -> Result<(), notify::Error> {
    if path.exists() {
        debug!("file already exists");
        return Ok(());
    };

    let (tx, mut rx) = mpsc::channel(1);

    let parent = path.parent().expect("should have a parent").to_path_buf();
    let handle = Arc::new(same_file::Handle::from_path(path.as_path())?);

    let mut watcher = RecommendedWatcher::new(
        move |res| match res {
            // for some reason, socket _creation_ is not detected on macOS
            // however, we can assume that any event except delete implies readiness
            Ok(Event { kind, paths, .. }) if !kind.is_remove() => {
                if paths
                    .iter()
                    .filter_map(|p| Handle::from_path(p).ok())
                    // using this handle allows us to compare across symlinks etc rather than paths
                    .any(|h| h.eq(&handle))
                {
                    debug!("file appeared");
                    futures::executor::block_on(async {
                        tx.send(()).await.unwrap();
                    })
                }
            }
            e => {
                debug!("got event: {:?}", e);
            }
        },
        Config::default(),
    )?;

    std::fs::create_dir_all(&parent)?;

    watcher.watch(parent.as_path(), notify::RecursiveMode::NonRecursive)?;
    rx.recv().await.expect("should receive a message");

    Ok(())
}
