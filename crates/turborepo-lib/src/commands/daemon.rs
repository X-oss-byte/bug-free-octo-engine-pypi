use std::{path::PathBuf, time::Duration};

use log::debug;
use tokio::{join, net::UnixStream};
use tonic::transport::Server;

use super::CommandBase;
use crate::{
    cli::DaemonCommand,
    daemon::{DaemonConnector, DaemonError},
};

/// Runs the daemon command.
pub async fn main(command: &Option<DaemonCommand>, base: &CommandBase) -> Result<(), DaemonError> {
    let command = match command {
        Some(command) => command,
        None => return run_daemon(base).await,
    };

    let (can_start_server, can_kill_server) = match command {
        DaemonCommand::Status { .. } => (false, false),
        DaemonCommand::Restart | DaemonCommand::Stop => (false, true),
        DaemonCommand::Start => (true, true),
    };

    let connector = DaemonConnector {
        can_start_server,
        can_kill_server,
        pid_file: base.daemon_file_root().join("turbod.pid"),
        sock_file: base.daemon_file_root().join("turbod.sock"),
    };

    let mut client = connector.connect().await?;

    match command {
        DaemonCommand::Restart => {
            client.restart().await?;
        }
        // connector.connect will have already started the daemon if needed,
        // so this is a no-op
        DaemonCommand::Start => {}
        DaemonCommand::Stop => {
            client.stop().await?;
        }
        DaemonCommand::Status { json } => {
            let status = client.status().await?;
            let status = DaemonStatus {
                uptime_ms: status.uptime_msec,
                log_file: status.log_file.into(),
                pid_file: client.connect_settings.pid_file.clone(),
                sock_file: client.connect_settings.sock_file.clone(),
            };
            if *json {
                println!("{}", serde_json::to_string_pretty(&status).unwrap());
            } else {
                println!("Daemon log file: {}", status.log_file.to_string_lossy());
                println!(
                    "Daemon uptime: {}s",
                    humantime::format_duration(Duration::from_millis(status.uptime_ms))
                );
                println!("Daemon pid file: {}", status.pid_file.to_string_lossy());
                println!("Daemon socket file: {}", status.sock_file.to_string_lossy());
            }
        }
    };

    Ok(())
}

pub async fn run_daemon(base: &CommandBase) -> Result<(), DaemonError> {
    let server = crate::daemon::DaemonServer::new(&base, Duration::from_secs(60 * 60 * 4));
    server.serve().await;

    Ok(())
}

#[derive(serde::Serialize)]
pub struct DaemonStatus {
    pub uptime_ms: u64,
    pub log_file: PathBuf,
    pub pid_file: PathBuf,
    pub sock_file: PathBuf,
}
