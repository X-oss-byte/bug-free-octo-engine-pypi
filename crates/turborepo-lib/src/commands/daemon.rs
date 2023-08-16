use std::{path::PathBuf, time::Duration};

use super::CommandBase;
use crate::{cli::DaemonCommand, daemon::DaemonConnector};

/// Runs the daemon command.
pub async fn main(command: &DaemonCommand, base: &CommandBase) -> anyhow::Result<()> {
    let (can_start_server, can_kill_server) = match command {
        DaemonCommand::Status { .. } => (false, false),
        DaemonCommand::Restart | DaemonCommand::Stop => (false, true),
        DaemonCommand::Start => (true, true),
    };

    let connector = DaemonConnector {
        can_start_server,
        can_kill_server,
        pid_file: base.daemon_file_root().join(
            turborepo_paths::ForwardRelativePath::new("turbod.pid").expect("forward relative"),
        ),
        sock_file: base.daemon_file_root().join(
            turborepo_paths::ForwardRelativePath::new("turbod.sock").expect("forward relative"),
        ),
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
                pid_file: client.pid_file().to_owned(),
                sock_file: client.sock_file().to_owned(),
            };
            if *json {
                println!("{}", serde_json::to_string_pretty(&status)?);
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

#[derive(serde::Serialize)]
pub struct DaemonStatus {
    pub uptime_ms: u64,
    // this comes from the daemon server, so we trust that
    // it is correct
    pub log_file: PathBuf,
    pub pid_file: turborepo_paths::AbsoluteNormalizedPathBuf,
    pub sock_file: turborepo_paths::AbsoluteNormalizedPathBuf,
}
