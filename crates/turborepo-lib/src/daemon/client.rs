use thiserror::Error;
use tonic::Code;

use self::proto::turbod_client::TurbodClient;
use super::connector::DaemonConnector;
use crate::get_version;

pub mod proto {
    tonic::include_proto!("turbodprotocol");
}

#[derive(Debug)]
pub struct DaemonClient<T> {
    pub client: TurbodClient<tonic::transport::Channel>,
    pub connect_settings: T,
}

#[derive(Default)]
pub struct DaemonClientOpts {
    pub dont_start: bool,
    pub dont_kill: bool,
}

impl<T> DaemonClient<T> {
    /// Get the status of the daemon.
    pub async fn status(&mut self) -> Result<proto::DaemonStatus, DaemonError> {
        self.client
            .status(proto::StatusRequest {})
            .await
            .map_err(|s| s.code())
            .map_err(DaemonError::from)?
            .into_inner()
            .daemon_status
            .ok_or(DaemonError::MissingResponse)
    }

    /// Stops the daemon and closes the connection, returning
    /// the connection settings that were used to connect.
    pub async fn stop(mut self) -> Result<T, DaemonError> {
        self.client
            .shutdown(proto::ShutdownRequest {})
            .await
            .map_err(|s| s.code())
            .map_err(DaemonError::from)?;

        Ok(self.connect_settings)
    }

    /// Interrogate the server for its version.
    pub(super) async fn handshake(&mut self) -> Result<(), DaemonError> {
        let _ret = self
            .client
            .hello(proto::HelloRequest {
                version: get_version().to_string(),
                // todo(arlyon): add session id
                ..Default::default()
            })
            .await
            .map_err(|s| s.code())
            .map_err(DaemonError::from)?;

        Ok(())
    }
}

impl DaemonClient<()> {
    /// Augment the client with the connect settings, allowing it to be
    /// restarted.
    pub fn with_connect_settings(
        self,
        connect_settings: DaemonConnector,
    ) -> DaemonClient<DaemonConnector> {
        DaemonClient {
            client: self.client,
            connect_settings,
        }
    }
}

impl DaemonClient<DaemonConnector> {
    /// Stops the daemon, closes the connection, and opens a new connection.
    pub async fn restart(self) -> Result<DaemonClient<DaemonConnector>, DaemonError> {
        self.stop().await?.connect().await
    }
}

#[derive(Error, Debug)]
pub enum DaemonError {
    #[error("Failed to connect to daemon")]
    Connection,
    #[error("Failed to find sock file")]
    Socket,
    #[error("Daemon version mismatch")]
    VersionMismatch,
    #[error("could not connect: {0}")]
    GrpcTransport(#[from] tonic::transport::Error),
    #[error("could not fork")]
    Fork,
    #[error("could not connect: {0}")]
    GrpcFailure(tonic::Code),
    #[error("missing response")]
    MissingResponse,
    #[error("could not read pid file")]
    PidFile,
}

impl From<Code> for DaemonError {
    fn from(code: Code) -> DaemonError {
        match code {
            Code::FailedPrecondition => DaemonError::VersionMismatch,
            Code::Unavailable => DaemonError::Connection,
            c => DaemonError::GrpcFailure(c),
        }
    }
}
