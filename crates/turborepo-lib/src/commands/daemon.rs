use std::path::PathBuf;

use super::CommandBase;
use crate::{
    cli::DaemonCommand,
    daemon::{DaemonClientOpts, DaemonConnector, DaemonError},
};

/// Runs the daemon command. As its output, it may return a 'Daemon' command
/// which, when encountered, indicates that the caller should run the daemon.
pub async fn main(command: &DaemonCommand, base: &CommandBase) -> Result<(), DaemonError> {
    let c_opts = match command {
        DaemonCommand::Status { .. } => DaemonClientOpts {
            dont_start: true,
            dont_kill: true,
        },
        DaemonCommand::Restart | DaemonCommand::Stop => DaemonClientOpts {
            dont_start: true,
            ..Default::default()
        },
        DaemonCommand::Start => Default::default(),
    };

    let connector = DaemonConnector {
        dont_start: c_opts.dont_start,
        dont_kill: c_opts.dont_kill,
        pid_file: base.daemon_file_root().join("turbod.pid"),
        sock_file: base.daemon_file_root().join("turbod.sock"),
    };

    let mut c = connector.connect().await?;

    match command {
        DaemonCommand::Restart => {
            c.restart().await?;
        }
        DaemonCommand::Start => {} // no-op
        DaemonCommand::Stop => {
            c.stop().await?;
        }
        DaemonCommand::Status { json } => {
            let status = c.status().await?;
            let status = DaemonStatus {
                uptime_ms: status.uptime_msec,
                log_file: status.log_file.into(),
                pid_file: c.connect_settings.pid_file.clone(),
                sock_file: c.connect_settings.sock_file.clone(),
            };
            if *json {
                println!("{}", serde_json::to_string_pretty(&status).unwrap());
            } else {
                println!("Daemon log file: {}", status.log_file.to_string_lossy());
                println!("Daemon uptime: {}s", status.uptime_ms / 1000);
                println!("Daemon pid file: {}", status.pid_file.to_string_lossy());
                println!("Daemon socket file: {}", status.sock_file.to_string_lossy());
            }
        }
    };

    Ok(())
}

#[derive(serde::Serialize)]
pub struct DaemonStatus {
    pub uptime_ms: u64,
    pub log_file: PathBuf,
    pub pid_file: PathBuf,
    pub sock_file: PathBuf,
}
