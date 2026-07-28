use std::time::Duration;

use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{Instant as TokioInstant, sleep_until};

use super::{TUNNEL_BUFFER_SIZE, WEBSOCKET_TUNNEL_IDLE_TIMEOUT};

pub(super) async fn tunnel_websocket(
    client: TokioIo<hyper::upgrade::Upgraded>,
    backend: TcpStream,
) {
    tunnel_bidirectional(client, backend, WEBSOCKET_TUNNEL_IDLE_TIMEOUT).await;
}

async fn tunnel_bidirectional<C, B>(client: C, backend: B, idle_timeout: Duration)
where
    C: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let (mut client_read, mut client_write) = tokio::io::split(client);
    let (mut backend_read, mut backend_write) = tokio::io::split(backend);
    let mut client_buffer = [0u8; TUNNEL_BUFFER_SIZE];
    let mut backend_buffer = [0u8; TUNNEL_BUFFER_SIZE];
    let mut client_to_backend_open = true;
    let mut backend_to_client_open = true;
    let idle = sleep_until(TokioInstant::now() + idle_timeout);
    tokio::pin!(idle);

    while client_to_backend_open || backend_to_client_open {
        tokio::select! {
            result = client_read.read(&mut client_buffer), if client_to_backend_open => {
                let n = match result {
                    Ok(n) => n,
                    Err(error) => {
                        log_tunnel_error("client read", &error);
                        break;
                    }
                };
                if n == 0 {
                    client_to_backend_open = false;
                    let _ = backend_write.shutdown().await;
                    continue;
                }
                if let Err(error) = backend_write.write_all(&client_buffer[..n]).await {
                    log_tunnel_error("backend write", &error);
                    break;
                }
                idle.as_mut().reset(TokioInstant::now() + idle_timeout);
            }
            result = backend_read.read(&mut backend_buffer), if backend_to_client_open => {
                let n = match result {
                    Ok(n) => n,
                    Err(error) => {
                        log_tunnel_error("backend read", &error);
                        break;
                    }
                };
                if n == 0 {
                    backend_to_client_open = false;
                    let _ = client_write.shutdown().await;
                    continue;
                }
                if let Err(error) = client_write.write_all(&backend_buffer[..n]).await {
                    log_tunnel_error("client write", &error);
                    break;
                }
                idle.as_mut().reset(TokioInstant::now() + idle_timeout);
            }
            () = &mut idle => {
                log_tunnel_idle_timeout();
                break;
            }
        }
    }

    let _ = client_write.shutdown().await;
    let _ = backend_write.shutdown().await;
}

fn log_tunnel_error(context: &str, error: &std::io::Error) {
    #[cfg(debug_assertions)]
    eprintln!("jig proxy websocket tunnel {context} failed: {error}");
    #[cfg(not(debug_assertions))]
    let _ = (context, error);
}

fn log_tunnel_idle_timeout() {
    #[cfg(debug_assertions)]
    eprintln!("jig proxy websocket tunnel closed after idle timeout");
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, duplex};
    use tokio::time::timeout;

    use super::*;

    #[tokio::test]
    async fn tunnel_closes_both_sides_after_idle_timeout() {
        let (client, mut client_peer) = duplex(64);
        let (backend, mut backend_peer) = duplex(64);

        timeout(
            Duration::from_secs(1),
            tunnel_bidirectional(client, backend, Duration::from_millis(20)),
        )
        .await
        .expect("tunnel did not close after idle timeout");

        let mut byte = [0u8; 1];
        assert_eq!(client_peer.read(&mut byte).await.unwrap(), 0);
        assert_eq!(backend_peer.read(&mut byte).await.unwrap(), 0);
    }
}
