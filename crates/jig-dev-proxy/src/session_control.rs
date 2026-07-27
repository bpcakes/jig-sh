use std::fmt::Write as _;
use std::io::{self, BufRead, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use subtle::ConstantTimeEq;

const PROTOCOL_VERSION: &str = "JIG-DEV-SESSION/1";
const MAX_SESSION_ID_BYTES: usize = 128;
const TOKEN_BYTES: usize = 32;
const TOKEN_HEX_BYTES: usize = TOKEN_BYTES * 2;
const MAX_REQUEST_BYTES: usize = 256;
const MAX_RESPONSE_BYTES: usize = 64;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
const IO_TIMEOUT: Duration = Duration::from_millis(300);
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(10);

const RESPONSE_PONG: &[u8] = b"OK\tPONG\n";
const RESPONSE_STOPPED: &[u8] = b"OK\tSTOPPED\n";
const RESPONSE_REJECTED: &[u8] = b"ERR\tREJECTED\n";
const RESPONSE_FRAME: &[u8] = b"ERR\tFRAME\n";
const RESPONSE_READ: &[u8] = b"ERR\tREAD\n";

#[derive(Clone, Copy)]
enum ControlCommand {
    Ping,
    Stop,
}

enum ControlResponse {
    Pong,
    Stopped,
    Rejected,
    InvalidFrame,
    ReadFailed,
}

#[derive(Debug)]
pub(crate) struct StopRequestError {
    message: String,
    delivery_uncertain: bool,
}

impl StopRequestError {
    pub(crate) fn delivery_uncertain(&self) -> bool {
        self.delivery_uncertain
    }

    fn new(error: impl std::fmt::Display, delivery_uncertain: bool) -> Self {
        Self {
            message: error.to_string(),
            delivery_uncertain,
        }
    }
}

impl std::fmt::Display for StopRequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for StopRequestError {}

/// A loopback-only authenticated control endpoint for one foreground dev
/// session.
///
/// The token is intentionally not exposed through `Debug` or emitted by the
/// listener thread. Callers persist it only in owner-private session state.
pub(crate) struct SessionControlServer {
    port: u16,
    token: String,
    stop_requested: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl SessionControlServer {
    pub(crate) fn start(session_id: &str) -> Result<Self> {
        validate_session_id(session_id)?;
        let token = generate_token()?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .context("failed to bind the dev-session control listener")?;
        let local_addr = listener
            .local_addr()
            .context("failed to inspect the dev-session control listener")?;
        if local_addr.ip() != Ipv4Addr::LOCALHOST {
            bail!("dev-session control listener did not bind to IPv4 loopback");
        }
        let port = local_addr.port();
        if port == 0 {
            bail!("dev-session control listener received an invalid port");
        }
        listener
            .set_nonblocking(true)
            .context("failed to configure the dev-session control listener")?;

        let stop_requested = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_stop_requested = Arc::clone(&stop_requested);
        let worker_shutdown = Arc::clone(&shutdown);
        let worker_session_id = session_id.to_owned();
        let worker_token = token.clone();
        let worker = thread::Builder::new()
            .name("jig-dev-session-control".into())
            .spawn(move || {
                run_server(
                    listener,
                    &worker_session_id,
                    &worker_token,
                    &worker_stop_requested,
                    &worker_shutdown,
                );
            })
            .context("failed to start the dev-session control listener thread")?;

        Ok(Self {
            port,
            token,
            stop_requested,
            shutdown,
            worker: Some(worker),
        })
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    pub(crate) fn stop_requested(&self) -> bool {
        self.stop_requested.load(Ordering::Acquire)
    }
}

impl Drop for SessionControlServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        let address = loopback_address(self.port);
        let _ = TcpStream::connect_timeout(&address, ACCEPT_POLL_INTERVAL);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(crate) fn request_stop(
    port: u16,
    session_id: &str,
    token: &str,
) -> std::result::Result<(), StopRequestError> {
    let mut delivery_uncertain = false;
    let response = send_request(
        port,
        ControlCommand::Stop,
        session_id,
        token,
        Some(&mut delivery_uncertain),
    )
    .map_err(|error| StopRequestError::new(error, delivery_uncertain))?;
    match response {
        ControlResponse::Stopped => Ok(()),
        ControlResponse::Rejected => Err(StopRequestError::new(
            "dev-session control stop request was rejected",
            false,
        )),
        ControlResponse::Pong => Err(StopRequestError::new(
            "dev-session control endpoint returned a ping response to a stop request",
            false,
        )),
        ControlResponse::InvalidFrame => Err(StopRequestError::new(
            "dev-session control endpoint rejected the stop request frame",
            false,
        )),
        ControlResponse::ReadFailed => Err(StopRequestError::new(
            "dev-session control endpoint could not read the stop request",
            false,
        )),
    }
}

pub(crate) fn ping(port: u16, session_id: &str, token: &str) -> Result<bool> {
    match send_request(port, ControlCommand::Ping, session_id, token, None)? {
        ControlResponse::Pong => Ok(true),
        ControlResponse::Rejected => Ok(false),
        ControlResponse::Stopped => {
            bail!("dev-session control endpoint returned a stop response to a ping request")
        }
        ControlResponse::InvalidFrame => {
            bail!("dev-session control endpoint rejected the ping request frame")
        }
        ControlResponse::ReadFailed => {
            bail!("dev-session control endpoint could not read the ping request")
        }
    }
}

fn run_server(
    listener: TcpListener,
    session_id: &str,
    token: &str,
    stop_requested: &AtomicBool,
    shutdown: &AtomicBool,
) {
    while !shutdown.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, peer_addr)) => {
                if shutdown.load(Ordering::Acquire) {
                    break;
                }
                let _ =
                    handle_connection(&mut stream, peer_addr, session_id, token, stop_requested);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
}

fn handle_connection(
    stream: &mut TcpStream,
    peer_addr: SocketAddr,
    session_id: &str,
    token: &str,
    stop_requested: &AtomicBool,
) -> io::Result<()> {
    let local_addr = stream.local_addr()?;
    if !peer_addr.ip().is_loopback() || !local_addr.ip().is_loopback() {
        return Ok(());
    }
    // Some platforms inherit O_NONBLOCK from the listening socket. Each
    // accepted connection instead uses bounded blocking I/O so a request that
    // arrives just after accept is not spuriously rejected with WouldBlock.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;

    let response = match read_bounded_line(stream, MAX_REQUEST_BYTES) {
        Ok(frame) => dispatch_request(&frame, session_id, token, stop_requested),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => RESPONSE_FRAME,
        Err(_) => RESPONSE_READ,
    };
    stream.write_all(response)?;
    stream.flush()
}

fn dispatch_request(
    frame: &[u8],
    session_id: &str,
    token: &str,
    stop_requested: &AtomicBool,
) -> &'static [u8] {
    let Some((command, supplied_session_id, supplied_token)) = parse_request(frame) else {
        return RESPONSE_FRAME;
    };
    let token_matches = token_matches(token, supplied_token);
    let session_matches = supplied_session_id == session_id;
    if !token_matches || !session_matches {
        return RESPONSE_REJECTED;
    }

    match command {
        ControlCommand::Ping => RESPONSE_PONG,
        ControlCommand::Stop => {
            stop_requested.store(true, Ordering::Release);
            RESPONSE_STOPPED
        }
    }
}

fn parse_request(frame: &[u8]) -> Option<(ControlCommand, &str, &str)> {
    let line = frame.strip_suffix(b"\n")?;
    if line.ends_with(b"\r") {
        return None;
    }
    let line = std::str::from_utf8(line).ok()?;
    let mut fields = line.split('\t');
    if fields.next()? != PROTOCOL_VERSION {
        return None;
    }
    let command = match fields.next()? {
        "PING" => ControlCommand::Ping,
        "STOP" => ControlCommand::Stop,
        _ => return None,
    };
    let session_id = fields.next()?;
    let token = fields.next()?;
    if fields.next().is_some() || validate_session_id(session_id).is_err() {
        return None;
    }
    Some((command, session_id, token))
}

fn send_request(
    port: u16,
    command: ControlCommand,
    session_id: &str,
    token: &str,
    delivery_uncertain: Option<&mut bool>,
) -> Result<ControlResponse> {
    if port == 0 {
        bail!("dev-session control port must be greater than zero");
    }
    validate_session_id(session_id)?;
    validate_token(token)?;

    let address = loopback_address(port);
    let mut stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)
        .context("failed to connect to the dev-session control endpoint")?;
    let peer_addr = stream
        .peer_addr()
        .context("failed to inspect the dev-session control peer")?;
    let local_addr = stream
        .local_addr()
        .context("failed to inspect the dev-session control client")?;
    if !peer_addr.ip().is_loopback() || !local_addr.ip().is_loopback() {
        bail!("dev-session control connection was not confined to loopback");
    }
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .context("failed to configure the dev-session control read deadline")?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .context("failed to configure the dev-session control write deadline")?;

    let command = match command {
        ControlCommand::Ping => "PING",
        ControlCommand::Stop => "STOP",
    };
    let request = format!("{PROTOCOL_VERSION}\t{command}\t{session_id}\t{token}\n");
    if request.len() > MAX_REQUEST_BYTES {
        bail!("dev-session control request exceeded its protocol limit");
    }
    if let Some(delivery_uncertain) = delivery_uncertain {
        *delivery_uncertain = true;
    }
    stream
        .write_all(request.as_bytes())
        .context("failed to write the dev-session control request")?;
    stream
        .flush()
        .context("failed to flush the dev-session control request")?;

    let response = read_bounded_line(&mut stream, MAX_RESPONSE_BYTES)
        .context("failed to read the dev-session control response")?;
    match response.as_slice() {
        RESPONSE_PONG => Ok(ControlResponse::Pong),
        RESPONSE_STOPPED => Ok(ControlResponse::Stopped),
        RESPONSE_REJECTED => Ok(ControlResponse::Rejected),
        RESPONSE_FRAME => Ok(ControlResponse::InvalidFrame),
        RESPONSE_READ => Ok(ControlResponse::ReadFailed),
        _ => bail!("dev-session control endpoint returned an unknown response"),
    }
}

fn read_bounded_line(stream: &mut TcpStream, limit: usize) -> io::Result<Vec<u8>> {
    let mut reader = io::BufReader::with_capacity(limit.saturating_add(1), stream);
    let mut frame = Vec::with_capacity(limit.min(128));
    let read = {
        let mut limited = reader.by_ref().take(limit.saturating_add(1) as u64);
        limited.read_until(b'\n', &mut frame)?
    };
    if read == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "control peer closed before sending a frame",
        ));
    }
    if frame.len() > limit || frame.last() != Some(&b'\n') {
        drain_rejected_line(&mut reader, limit)?;
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "control frame exceeded its protocol limit",
        ));
    }
    Ok(frame)
}

fn drain_rejected_line(reader: &mut impl BufRead, limit: usize) -> io::Result<()> {
    let mut discarded = Vec::with_capacity(limit.min(128));
    let mut limited = reader.take(limit.saturating_add(1) as u64);
    let _ = limited.read_until(b'\n', &mut discarded)?;
    Ok(())
}

fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.is_empty()
        || session_id.len() > MAX_SESSION_ID_BYTES
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        bail!(
            "dev-session id must be 1 to {MAX_SESSION_ID_BYTES} ASCII letters, digits, dots, dashes, or underscores"
        );
    }
    Ok(())
}

fn validate_token(token: &str) -> Result<()> {
    if token.len() != TOKEN_HEX_BYTES || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("dev-session control token has an invalid format");
    }
    Ok(())
}

fn token_matches(expected: &str, supplied: &str) -> bool {
    if expected.len() != TOKEN_HEX_BYTES || supplied.len() != TOKEN_HEX_BYTES {
        return false;
    }
    bool::from(expected.as_bytes().ct_eq(supplied.as_bytes()))
}

fn generate_token() -> Result<String> {
    let mut random = [0_u8; TOKEN_BYTES];
    getrandom::fill(&mut random).map_err(|error| {
        anyhow::anyhow!("failed to generate a dev-session control token: {error}")
    })?;
    let mut token = String::with_capacity(TOKEN_HEX_BYTES);
    for byte in random {
        write!(&mut token, "{byte:02x}")?;
    }
    Ok(token)
}

fn loopback_address(port: u16) -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::time::Instant;

    use super::*;

    const SESSION_ID: &str = "01JIGDEVSESSIONTEST";

    #[test]
    fn ping_authenticates_without_requesting_stop() {
        let server = SessionControlServer::start(SESSION_ID).unwrap();

        assert_eq!(server.token().len(), TOKEN_HEX_BYTES);
        assert!(server.token().bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(ping(server.port(), SESSION_ID, server.token()).unwrap());
        assert!(!server.stop_requested());
    }

    #[test]
    fn authentication_rejects_wrong_session_and_token() {
        let server = SessionControlServer::start(SESSION_ID).unwrap();
        let wrong_token = different_token(server.token());

        assert!(!ping(server.port(), SESSION_ID, &wrong_token).unwrap());
        assert!(!ping(server.port(), "01JIGDEVSESSIONOTHER", server.token()).unwrap());
        let error = request_stop(server.port(), SESSION_ID, &wrong_token)
            .unwrap_err()
            .to_string();
        assert!(!error.contains(server.token()));
        assert!(!error.contains(&wrong_token));
        assert!(!server.stop_requested());
    }

    #[test]
    fn repeated_stop_requests_are_idempotent() {
        let server = SessionControlServer::start(SESSION_ID).unwrap();

        request_stop(server.port(), SESSION_ID, server.token()).unwrap();
        request_stop(server.port(), SESSION_ID, server.token()).unwrap();

        assert!(server.stop_requested());
        assert!(ping(server.port(), SESSION_ID, server.token()).unwrap());
    }

    #[test]
    fn request_and_session_id_bounds_are_enforced() {
        assert!(SessionControlServer::start("").is_err());
        assert!(SessionControlServer::start("contains spaces").is_err());
        assert!(SessionControlServer::start(&"a".repeat(MAX_SESSION_ID_BYTES + 1)).is_err());

        let server = SessionControlServer::start(SESSION_ID).unwrap();
        let mut oversized = vec![b'x'; MAX_REQUEST_BYTES + 1];
        oversized.push(b'\n');
        let response = send_raw(server.port(), &oversized);

        assert_eq!(response, RESPONSE_FRAME);
        assert!(ping(server.port(), SESSION_ID, server.token()).unwrap());
    }

    #[test]
    fn drop_retires_listener_thread_and_closes_port() {
        let server = SessionControlServer::start(SESSION_ID).unwrap();
        let address = loopback_address(server.port());
        let mut stalled = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).unwrap();
        stalled.write_all(b"partial").unwrap();

        let started = Instant::now();
        drop(server);

        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).is_err());
    }

    fn different_token(token: &str) -> String {
        let mut bytes = token.as_bytes().to_vec();
        bytes[0] = if bytes[0] == b'0' { b'1' } else { b'0' };
        String::from_utf8(bytes).unwrap()
    }

    fn send_raw(port: u16, request: &[u8]) -> Vec<u8> {
        let address = loopback_address(port);
        let mut stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).unwrap();
        stream.set_read_timeout(Some(IO_TIMEOUT)).unwrap();
        stream.set_write_timeout(Some(IO_TIMEOUT)).unwrap();
        stream.write_all(request).unwrap();
        stream.flush().unwrap();

        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        response
    }
}
