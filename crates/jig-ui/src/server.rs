use std::fmt::Write as _;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use subtle::ConstantTimeEq;

use crate::{PlanSnapshot, SnapshotProvider, UiQuery, html};

const REQUEST_HEAD_LIMIT: usize = 16 * 1024;
const REQUEST_HEAD_DEADLINE: Duration = Duration::from_secs(2);
const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);
const ACCEPT_RETRY_LIMIT: usize = 8;
const WORKER_COUNT: usize = 8;
const BOOTSTRAP_QUERY: &str = "__jig_bootstrap";
const SESSION_COOKIE: &str = "jig_ui_session";
const CAPABILITY_BYTES: usize = 32;

pub struct UiServer {
    listener: TcpListener,
    security: Arc<UiSecurity>,
}

struct UiSecurity {
    authority: String,
    origin: String,
    namespace: String,
    bootstrap_capability: Mutex<Option<[u8; CAPABILITY_BYTES]>>,
    session_capability: [u8; CAPABILITY_BYTES],
}

impl UiServer {
    /// Binds a new loopback-only UI server on the requested port.
    ///
    /// # Errors
    ///
    /// Returns an error when the listener cannot be bound or inspected, or
    /// secure random capability generation fails.
    pub fn bind(port: u16) -> Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).with_context(|| {
            format!("Failed to bind 127.0.0.1:{port}. Pass --port to choose a free port.")
        })?;
        let authority = listener
            .local_addr()
            .context("Failed to read Jig UI listener address")?
            .to_string();
        let origin = format!("http://{authority}");
        let namespace = format!("/jig-{}/", encode_capability(&random_capability()?));
        Ok(Self {
            listener,
            security: Arc::new(UiSecurity {
                authority,
                origin,
                namespace,
                bootstrap_capability: Mutex::new(Some(random_capability()?)),
                session_capability: random_capability()?,
            }),
        })
    }

    pub fn bootstrap_url(&self) -> String {
        let capability = {
            let guard = self
                .security
                .bootstrap_capability
                .lock()
                .expect("bootstrap capability mutex poisoned");
            encode_capability(
                guard
                    .as_ref()
                    .expect("bootstrap URL is requested before the server starts"),
            )
        };
        format!(
            "{}{}?{BOOTSTRAP_QUERY}={}",
            self.security.origin, self.security.namespace, capability
        )
    }

    pub fn origin(&self) -> &str {
        &self.security.origin
    }
    pub fn snapshot_path(&self) -> String {
        format!("{}api/snapshot", self.security.namespace)
    }

    /// Serves dashboard requests until the listener or a worker fails.
    ///
    /// # Errors
    ///
    /// Returns an error when listener configuration, request acceptance, worker
    /// execution, or worker shutdown fails.
    pub fn serve(self, provider: &dyn SnapshotProvider) -> Result<()> {
        self.listener
            .set_nonblocking(true)
            .context("Failed to make Jig UI listener wakeable")?;
        let listener = self.listener;
        serve_with_accept(
            provider,
            self.security,
            Arc::new(AtomicUsize::new(0)),
            move || match listener.accept() {
                Ok((stream, _)) => Ok(Some(stream)),
                Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(None),
                Err(error) => Err(error),
            },
        )
    }
}

fn serve_with_accept(
    provider: &dyn SnapshotProvider,
    security: Arc<UiSecurity>,
    active_workers: Arc<AtomicUsize>,
    mut accept: impl FnMut() -> std::io::Result<Option<TcpStream>>,
) -> Result<()> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let (work_tx, work_rx) = mpsc::sync_channel::<TcpStream>(WORKER_COUNT * 2);
    let work_rx = Arc::new(Mutex::new(work_rx));
    let (status_tx, status_rx) = mpsc::channel::<Result<()>>();
    thread::scope(|scope| {
        let mut workers = Vec::new();
        let mut result = Ok(());
        for index in 0..WORKER_COUNT {
            let work_rx = Arc::clone(&work_rx);
            let security = Arc::clone(&security);
            let cancelled = Arc::clone(&cancelled);
            let status_tx = status_tx.clone();
            let active_workers = Arc::clone(&active_workers);
            match thread::Builder::new()
                .name(format!("jig-ui-{index}"))
                .spawn_scoped(scope, move || {
                    active_workers.fetch_add(1, Ordering::SeqCst);
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        worker_loop(provider, &security, &cancelled, &work_rx)
                    }))
                    .map_err(|_| anyhow!("Jig UI request worker {index} panicked"))
                    .and_then(|value| value);
                    active_workers.fetch_sub(1, Ordering::SeqCst);
                    let _ = status_tx.send(outcome);
                }) {
                Ok(worker) => workers.push(worker),
                Err(error) => {
                    result = Err(error).context("Failed to start Jig UI request worker");
                    break;
                }
            }
        }
        drop(status_tx);
        let mut transient_failures = 0usize;
        while result.is_ok() {
            if let Ok(worker_result) = status_rx.try_recv() {
                result = worker_result
                    .and_then(|()| Err(anyhow!("Jig UI request worker stopped unexpectedly")));
                break;
            }
            match accept() {
                Ok(Some(stream)) => {
                    transient_failures = 0;
                    match work_tx.try_send(stream) {
                        Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
                        Err(mpsc::TrySendError::Disconnected(_)) => {
                            result = Err(anyhow!("all Jig UI request workers stopped"));
                        }
                    }
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        ErrorKind::ConnectionAborted
                            | ErrorKind::ConnectionReset
                            | ErrorKind::OutOfMemory
                    ) && transient_failures < ACCEPT_RETRY_LIMIT =>
                {
                    transient_failures += 1;
                    thread::sleep(Duration::from_millis(
                        (10u64 << transient_failures.min(6)).min(500),
                    ));
                }
                Err(error) => {
                    result = Err(error)
                        .context("Jig UI listener accept failed permanently or repeatedly");
                }
            }
        }
        cancelled.store(true, Ordering::SeqCst);
        drop(work_tx);
        for worker in workers {
            if worker.join().is_err() && result.is_ok() {
                result = Err(anyhow!("Jig UI request worker panicked during shutdown"));
            }
        }
        result
    })
}

fn worker_loop(
    provider: &dyn SnapshotProvider,
    security: &UiSecurity,
    cancelled: &AtomicBool,
    work_rx: &Mutex<mpsc::Receiver<TcpStream>>,
) -> Result<()> {
    while !cancelled.load(Ordering::SeqCst) {
        let received = work_rx
            .lock()
            .map_err(|_| anyhow!("Jig UI work queue mutex poisoned"))?
            .recv_timeout(Duration::from_millis(50));
        match received {
            Ok(stream) => {
                if let Err(error) = handle_connection(provider, stream, security) {
                    eprintln!("jig ui: request failed: {error:#}");
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

fn handle_connection(
    provider: &dyn SnapshotProvider,
    mut stream: TcpStream,
    security: &UiSecurity,
) -> Result<()> {
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .context("Failed to set request-head read timeout")?;
    stream
        .set_write_timeout(Some(SOCKET_TIMEOUT))
        .context("Failed to set response write timeout")?;
    let head = read_request_head(&mut stream).context("Failed to parse HTTP request head")?;
    let text = std::str::from_utf8(&head).context("HTTP request head is not UTF-8")?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let headers = read_headers(lines)?;
    let response = match parse_request_line(request_line) {
        Ok((method, target)) => authorize_request(security, method, target, &headers)
            .unwrap_or_else(|| respond(provider, method, target, &security.namespace)),
        Err(_) => HttpResponse::text(400, "bad request\n"),
    };
    write_response(stream, &response)
}

fn read_request_head(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let deadline = Instant::now() + REQUEST_HEAD_DEADLINE;
    let mut bytes = Vec::with_capacity(1024);
    let mut chunk = [0u8; 512];
    while bytes.len() < REQUEST_HEAD_LIMIT {
        if Instant::now() >= deadline {
            bail!("request head deadline exceeded");
        }
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                bytes.extend_from_slice(&chunk[..read]);
                if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                    return Ok(bytes);
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error).context("Failed while reading request head"),
        }
    }
    if bytes.len() >= REQUEST_HEAD_LIMIT {
        bail!("request head exceeds {REQUEST_HEAD_LIMIT} bytes");
    }
    bail!("incomplete request head")
}

fn read_headers<'a>(lines: impl Iterator<Item = &'a str>) -> Result<Vec<(String, String)>> {
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            bail!("malformed HTTP header");
        };
        if name.is_empty()
            || name
                .bytes()
                .any(|b| !b.is_ascii_alphanumeric() && b != b'-')
        {
            bail!("malformed HTTP header name");
        }
        headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
    }
    Ok(headers)
}

fn parse_request_line(line: &str) -> Result<(&str, &str)> {
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        bail!("malformed request line");
    };
    if parts.next().is_some() || version != "HTTP/1.1" || !target.starts_with('/') {
        bail!("unsupported request target or HTTP version");
    }
    Ok((method, target))
}

fn authorize_request(
    security: &UiSecurity,
    method: &str,
    target: &str,
    headers: &[(String, String)],
) -> Option<HttpResponse> {
    let hosts = header_values(headers, "host");
    if hosts.len() != 1 || hosts[0] != security.authority {
        return Some(HttpResponse::text(403, "forbidden\n"));
    }
    let origins = header_values(headers, "origin");
    if origins.len() > 1
        || origins
            .first()
            .is_some_and(|origin| *origin != security.origin)
    {
        return Some(HttpResponse::text(403, "forbidden\n"));
    }
    if !target.starts_with(&security.namespace) {
        return Some(HttpResponse::text(403, "forbidden\n"));
    }

    if method == "GET" && target.starts_with(&format!("{}?", security.namespace)) {
        if let Some(candidate) = query_value(target, BOOTSTRAP_QUERY) {
            let Ok(mut bootstrap) = security.bootstrap_capability.lock() else {
                return Some(HttpResponse::text(500, "authorization state unavailable\n"));
            };
            let valid = bootstrap
                .as_ref()
                .is_some_and(|expected| capability_matches(candidate, expected));
            if valid {
                *bootstrap = None;
                return Some(HttpResponse::redirect(&security.namespace).with_header(
                    "Set-Cookie",
                    format!(
                        "{SESSION_COOKIE}={}; HttpOnly; SameSite=Strict; Path={}",
                        encode_capability(&security.session_capability),
                        security.namespace
                    ),
                ));
            }
            return Some(HttpResponse::text(403, "forbidden\n"));
        }
    }
    let authorized = header_values(headers, "cookie")
        .into_iter()
        .flat_map(|h| h.split(';'))
        .filter_map(|c| c.trim().split_once('='))
        .any(|(name, value)| {
            name == SESSION_COOKIE && capability_matches(value, &security.session_capability)
        });
    (!authorized).then(|| HttpResponse::text(403, "forbidden\n"))
}

fn header_values<'a>(headers: &'a [(String, String)], name: &str) -> Vec<&'a str> {
    headers
        .iter()
        .filter(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
        .collect()
}
fn query_value<'a>(target: &'a str, name: &str) -> Option<&'a str> {
    target.split_once('?')?.1.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == name).then_some(v)
    })
}

fn random_capability() -> Result<[u8; CAPABILITY_BYTES]> {
    #[derive(Debug)]
    struct RandomSourceError(getrandom::Error);
    impl std::fmt::Display for RandomSourceError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.0.fmt(f)
        }
    }
    impl std::error::Error for RandomSourceError {}
    let mut capability = [0u8; CAPABILITY_BYTES];
    getrandom::fill(&mut capability)
        .map_err(RandomSourceError)
        .context("Failed to generate Jig UI capability")?;
    Ok(capability)
}
fn encode_capability(capability: &[u8; CAPABILITY_BYTES]) -> String {
    let mut encoded = String::with_capacity(CAPABILITY_BYTES * 2);
    for byte in capability {
        write!(&mut encoded, "{byte:02x}").expect("writing a capability into a String cannot fail");
    }
    encoded
}
fn capability_matches(candidate: &str, expected: &[u8; CAPABILITY_BYTES]) -> bool {
    bool::from(
        candidate
            .as_bytes()
            .ct_eq(encode_capability(expected).as_bytes()),
    )
}

struct HttpResponse {
    status: u16,
    content_type: &'static str,
    body: String,
    headers: Vec<(&'static str, String)>,
}
impl HttpResponse {
    fn text(status: u16, body: &str) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8",
            body: body.into(),
            headers: vec![],
        }
    }
    const fn json(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "application/json",
            body,
            headers: vec![],
        }
    }
    const fn html(body: String) -> Self {
        Self {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body,
            headers: vec![],
        }
    }
    fn redirect(location: &str) -> Self {
        Self::text(303, "redirecting\n").with_header("Location", location.into())
    }
    fn with_header(mut self, name: &'static str, value: String) -> Self {
        self.headers.push((name, value));
        self
    }
}

fn respond(
    provider: &dyn SnapshotProvider,
    method: &str,
    target: &str,
    namespace: &str,
) -> HttpResponse {
    if method != "GET" {
        return HttpResponse::text(405, "method not allowed\n");
    }
    let Some(relative) = target.strip_prefix(namespace) else {
        return HttpResponse::text(404, "not found\n");
    };
    let (path, query) = relative
        .split_once('?')
        .map_or((relative, None), |(p, q)| (p, Some(q)));
    let ui_query = UiQuery::from_query(query);
    match path {
        "" => match provider.dashboard_snapshot(ui_query) {
            Ok(s) => HttpResponse::html(html::render_dashboard(&s, namespace)),
            Err(e) => HttpResponse::text(500, &format!("failed to build snapshot: {e:#}\n")),
        },
        "api/snapshot" => typed_json(provider.dashboard_snapshot(ui_query)),
        _ => respond_plan_routes(provider, path, namespace),
    }
}
fn typed_json<T: serde::Serialize>(result: Result<T>) -> HttpResponse {
    match result.and_then(|v| serde_json::to_string(&v).context("Failed to serialize UI response"))
    {
        Ok(body) => HttpResponse::json(200, body),
        Err(e) => HttpResponse::json(
            500,
            serde_json::json!({"ok":false,"error":format!("{e:#}")}).to_string(),
        ),
    }
}
fn respond_plan_routes(
    provider: &dyn SnapshotProvider,
    path: &str,
    namespace: &str,
) -> HttpResponse {
    let (id, json) = if let Some(id) = path.strip_prefix("api/plan/") {
        (id, true)
    } else if let Some(id) = path.strip_prefix("plan/") {
        (id, false)
    } else {
        return HttpResponse::text(404, "not found\n");
    };
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return HttpResponse::text(404, "not found\n");
    }
    match provider.plan_snapshot(id) {
        Ok(Some(plan)) if json => typed_json::<PlanSnapshot>(Ok(plan)),
        Ok(Some(plan)) => HttpResponse::html(html::render_plan_page(&plan, namespace)),
        Ok(None) => HttpResponse::text(404, "plan not found\n"),
        Err(e) if json => typed_json::<PlanSnapshot>(Err(e)),
        Err(e) => HttpResponse::text(500, &format!("failed to build plan view: {e:#}\n")),
    }
}

fn write_response(mut stream: TcpStream, response: &HttpResponse) -> Result<()> {
    let reason = match response.status {
        200 => "OK",
        303 => "See Other",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    };
    let mut extra = String::new();
    for (name, value) in &response.headers {
        write!(&mut extra, "{name}: {value}\r\n")
            .expect("writing response headers into a String cannot fail");
    }
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'\r\n{}\r\n",
        response.status,
        reason,
        response.content_type,
        response.body.len(),
        extra
    );
    stream
        .write_all(head.as_bytes())
        .context("Failed to write HTTP response headers")?;
    stream
        .write_all(response.body.as_bytes())
        .context("Failed to write HTTP response body")?;
    stream.flush().context("Failed to flush HTTP response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::SocketAddr;

    struct FakeProvider;
    impl SnapshotProvider for FakeProvider {
        fn dashboard_snapshot(&self, query: UiQuery) -> Result<crate::DashboardSnapshot> {
            serde_json::from_value(serde_json::json!({
                "ok":true,"command":"ui snapshot","generated_at_ms":1,
                "repo":{"name":"demo","default_branch":"main","source_commit":null,"source_path":null},
                "harness":{"runtime_version":"test","contract_version":1},
                "current_session_id":null,
                "counts":{"sessions":0,"session_events":0,"plans":0,"plan_events":0,"open_plans":0,"decisions":0},
                "open_plans":[],"history":[],"failures":[],"tool_stats":[],
                "loops":null,"loops_error":null,"timeline":[],
                "timeline_show":query.show.as_str(),"timeline_limit":query.limit
            })).map_err(Into::into)
        }
        fn plan_snapshot(&self, _: &str) -> Result<Option<PlanSnapshot>> {
            Ok(None)
        }
    }

    fn security(authority: String) -> UiSecurity {
        UiSecurity {
            origin: format!("http://{authority}"),
            authority,
            namespace: "/jig-secret/".into(),
            bootstrap_capability: Mutex::new(Some([1; CAPABILITY_BYTES])),
            session_capability: [2; CAPABILITY_BYTES],
        }
    }
    fn request(addr: SocketAddr, text: String) -> String {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.write_all(text.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn capability_codec_is_exact() {
        let e = [0xab; CAPABILITY_BYTES];
        let s = encode_capability(&e);
        assert!(capability_matches(&s, &e));
        assert!(!capability_matches(&s.to_uppercase(), &e));
    }

    #[test]
    fn namespace_scopes_cookie_and_rejects_routes_outside_it() {
        let security = security("127.0.0.1:5440".into());
        let bootstrap = format!(
            "/jig-secret/?{BOOTSTRAP_QUERY}={}",
            encode_capability(&[1; CAPABILITY_BYTES])
        );
        let response = authorize_request(
            &security,
            "GET",
            &bootstrap,
            &[("host".into(), security.authority.clone())],
        )
        .unwrap();
        assert_eq!(response.status, 303);
        let cookie = response
            .headers
            .iter()
            .find(|(name, _)| *name == "Set-Cookie")
            .unwrap()
            .1
            .clone();
        assert!(cookie.contains("Path=/jig-secret/"));
        let bearer = cookie.split(';').next().unwrap().to_string();
        let headers = vec![
            ("host".into(), security.authority.clone()),
            ("cookie".into(), bearer),
        ];
        assert_eq!(
            authorize_request(&security, "GET", "/api/snapshot", &headers)
                .unwrap()
                .status,
            403
        );
        assert!(
            authorize_request(&security, "GET", "/jig-secret/api/snapshot", &headers).is_none()
        );
    }

    #[test]
    fn slow_connection_does_not_starve_another_request() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let security = Arc::new(security(addr.to_string()));
        let worker_security = Arc::clone(&security);
        let worker = thread::spawn(move || {
            thread::scope(|scope| {
                for _ in 0..2 {
                    let (stream, _) = listener.accept().unwrap();
                    let security = Arc::clone(&worker_security);
                    scope.spawn(move || {
                        let _ = handle_connection(&FakeProvider, stream, &security);
                    });
                }
            });
        });
        let mut slow = TcpStream::connect(addr).unwrap();
        slow.write_all(b"G").unwrap();
        let cookie = format!(
            "{SESSION_COOKIE}={}",
            encode_capability(&security.session_capability)
        );
        let started = Instant::now();
        let response = request(
            addr,
            format!(
                "GET /jig-secret/api/snapshot HTTP/1.1\r\nHost: {addr}\r\nCookie: {cookie}\r\n\r\n"
            ),
        );
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(slow);
        worker.join().unwrap();
    }

    #[test]
    fn accept_failure_cancels_and_joins_all_workers() {
        let active = Arc::new(AtomicUsize::new(0));
        let started = Instant::now();
        let error = serve_with_accept(
            &FakeProvider,
            Arc::new(security("127.0.0.1:5440".into())),
            Arc::clone(&active),
            || {
                Err(std::io::Error::new(
                    ErrorKind::PermissionDenied,
                    "forced accept failure",
                ))
            },
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("forced accept failure"));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }
}
