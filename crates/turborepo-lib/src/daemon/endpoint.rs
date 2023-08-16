use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use futures::Stream;
use sysinfo::{PidExt, ProcessExt, ProcessRefreshKind, RefreshKind, SystemExt};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tonic::transport::server::Connected;

///
pub async fn get_channel(
    path: PathBuf,
) -> Result<tonic::transport::Channel, tonic::transport::Error> {
    let arc = Arc::new(path);
    tonic::transport::Endpoint::try_from("http://[::]:50051")?
        .connect_with_connector(tower::service_fn(move |_| {
            // we clone the reference counter here and move it into the async closure
            let arc = arc.clone();
            #[cfg(unix)]
            {
                async move { tokio::net::UnixStream::connect::<&Path>(arc.as_path()).await }
            }

            #[cfg(windows)]
            {
                async move { uds_windows::UnixStream::connect(arc.as_path()) }
            }
        }))
        .await
}

/// Gets a stream of incoming connections from a Unix socket.
/// On windows, this will use the `uds_windows` crate, and
/// poll the result in another thread.
pub async fn open_socket(
    path: PathBuf,
) -> Result<
    impl Stream<Item = Result<impl Connected + AsyncWrite + AsyncRead, std::io::Error>>,
    std::io::Error,
> {
    let pid_path = path.join("turbod.pid");
    let sock_path = path.join("turbod.sock");
    let mut lock = pidlock::Pidlock::new(pid_path.to_str().unwrap());

    // another daemon running, exit
    if lock.get_owner().is_some() {
        panic!("Another daemon is already running");
    } else {
        // delete the socket just in case
        // we ignore the error in case it exists
        std::fs::remove_file(&sock_path).ok();
        lock.acquire().unwrap();
    }

    #[cfg(unix)]
    {
        Ok(tokio_stream::wrappers::UnixListenerStream::new(
            tokio::net::UnixListener::bind(sock_path)?,
        ))
    }

    #[cfg(windows)]
    {
        let listener = uds_windows::UnixListener::bind(path);
        futures::stream::unfold(listener, |listener| async move {
            match listener.accept().await {
                Ok((stream, _)) => Some((Ok(stream), listener)),
                Err(err) => Some((Err(err), listener)),
            }
        })
    }
}

/// An adaptor over uds_windows that implements AsyncRead and AsyncWrite.
#[cfg(windows)]
struct UdsWindowsStream(uds_windows::UnixStream);

#[cfg(windows)]
impl AsyncRead for UdsWindowsStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.0.poll_read(cx, buf)
    }
}

#[cfg(windows)]
impl AsyncWrite for UdsWindowsStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.0.poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.0.poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.0.poll_shutdown(cx)
    }
}
