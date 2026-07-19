use std::collections::HashSet;
use std::convert::Infallible;
use std::error::Error;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use bytes::Bytes;
use http_body_util::{BodyExt, LengthLimitError, Limited, combinators::UnsyncBoxBody};
use hyper::body::Incoming;
use hyper::header::{CONNECTION, CONTENT_LENGTH, HeaderMap, HeaderName, HeaderValue, UPGRADE, VIA};
#[cfg(test)]
use hyper::header::{HOST, TRANSFER_ENCODING};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode, Uri, Version};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use sha1::{Digest as _, Sha1};
#[cfg(test)]
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::{sleep, timeout};
use tokio_rustls::TlsAcceptor;

use crate::certs;
use crate::host::ip_is_loopback;
use crate::ports::local_lan_ip_for_ipv4_listener;
use crate::state::{FileSignature, StateStore, file_signature};
use crate::types::{ProxySettings, Route};
mod backend;
mod error_page;
mod headers;
mod routing;
mod tls;
mod tunnel;

use self::backend::*;
use self::error_page::*;
use self::headers::*;
use self::routing::*;
use self::tls::*;
use self::tunnel::tunnel_websocket;

type ProxyBody = UnsyncBoxBody<Bytes, hyper::Error>;
type BoxBodyError = Box<dyn Error + Send + Sync>;
type ClientBody = UnsyncBoxBody<Bytes, BoxBodyError>;
type ProxyClient = Client<HttpConnector, ClientBody>;
type TlsSignature = Option<(FileSignature, FileSignature)>;

#[derive(Clone)]
struct ConnectionPermit {
    _permit: Arc<OwnedSemaphorePermit>,
}

impl ConnectionPermit {
    fn new(permit: OwnedSemaphorePermit) -> Self {
        Self {
            _permit: Arc::new(permit),
        }
    }
}

#[derive(Clone)]
struct ProxyLimits {
    connections: Arc<Semaphore>,
    requests: Arc<Semaphore>,
    websockets: Arc<Semaphore>,
}

impl ProxyLimits {
    fn new() -> Self {
        Self {
            connections: Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS)),
            requests: Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS)),
            websockets: Arc::new(Semaphore::new(MAX_CONCURRENT_WEBSOCKET_TUNNELS)),
        }
    }
}

#[derive(Clone)]
struct RequestContext {
    remote_addr: SocketAddr,
    proxy_port: u16,
    tls: bool,
    local_ip: IpAddr,
    lan_ip: Option<IpAddr>,
    health_token: Arc<str>,
}

#[derive(Clone)]
struct ListenerContext {
    proxy_port: u16,
    lan_ip: Option<IpAddr>,
    health_token: Arc<str>,
}

const ROUTE_CACHE_MAX_AGE: Duration = Duration::from_millis(500);
const MAX_CONCURRENT_CONNECTIONS: usize = 256;
const MAX_CONCURRENT_REQUESTS: usize = 1024;
const MAX_CONCURRENT_WEBSOCKET_TUNNELS: usize = 64;
const MAX_BACKEND_HEADER_BYTES: usize = 64 * 1024;
const MAX_BACKEND_HEADER_COUNT: usize = 1024;
const MAX_BACKEND_REQUEST_BODY_BYTES: usize = 100 * 1024 * 1024;
const MAX_BACKEND_DRAIN_BODY_BYTES: usize = 10 * 1024 * 1024;
const MAX_CHUNK_HEADER_BYTES: usize = 8 * 1024;
const HTTP2_MAX_CONCURRENT_STREAMS: u32 = 64;
const HTTP2_MAX_FRAME_SIZE: u32 = 16 * 1024;
const HTTP2_MAX_HEADER_LIST_SIZE: u32 = 64 * 1024;
const HTTP2_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(10);
const HTTP2_KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(5);
const BACKEND_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const BACKEND_HTTP_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const BACKEND_BODY_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const HTTP1_HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const WEBSOCKET_BACKEND_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const WEBSOCKET_UPGRADE_TIMEOUT: Duration = Duration::from_secs(5);
// Local HMR and dev-tool WebSockets can sit quiet for long stretches between
// edits, so the tunnel idle window is intentionally longer than HTTP waits.
const WEBSOCKET_TUNNEL_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(100);
const TLS_RELOAD_FILE_RETRY_DELAY: Duration = Duration::from_millis(25);
const TLS_RELOAD_FILE_ATTEMPTS: usize = 3;
const MAX_CONSECUTIVE_ACCEPT_ERRORS: u32 = 10;
const MAX_PROXY_HOPS: u8 = 8;
const TUNNEL_BUFFER_SIZE: usize = 16 * 1024;
const VIA_VALUE: &str = "1.1 jig";
const HEALTH_TOKEN_BYTES: usize = 64;

pub(crate) fn run_foreground(
    settings: ProxySettings,
    current_exe: std::path::PathBuf,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run_async(settings, current_exe))
}

async fn run_async(settings: ProxySettings, current_exe: std::path::PathBuf) -> Result<()> {
    let store = StateStore::resolve(settings.state_dir.clone())?;
    let owns_runtime = Arc::new(AtomicBool::new(false));

    #[cfg(unix)]
    let result = {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .context("Failed to listen for SIGTERM")?;
        tokio::select! {
            result = run_bound(settings, store.clone(), current_exe, owns_runtime.clone()) => result,
            signal = tokio::signal::ctrl_c() => {
                clear_runtime_files_if_owned(&store, &owns_runtime);
                signal.context("Failed to listen for Ctrl-C")?;
                anyhow::bail!("Jig proxy interrupted");
            }
            _ = terminate.recv() => {
                clear_runtime_files_if_owned(&store, &owns_runtime);
                anyhow::bail!("Jig proxy terminated");
            }
        }
    };

    #[cfg(windows)]
    let result = {
        let mut ctrl_break =
            tokio::signal::windows::ctrl_break().context("Failed to listen for Ctrl-Break")?;
        let mut ctrl_close =
            tokio::signal::windows::ctrl_close().context("Failed to listen for console close")?;
        let mut ctrl_shutdown =
            tokio::signal::windows::ctrl_shutdown().context("Failed to listen for shutdown")?;
        tokio::select! {
            result = run_bound(settings, store.clone(), current_exe, owns_runtime.clone()) => result,
            signal = tokio::signal::ctrl_c() => {
                clear_runtime_files_if_owned(&store, &owns_runtime);
                signal.context("Failed to listen for Ctrl-C")?;
                anyhow::bail!("Jig proxy interrupted");
            }
            _ = ctrl_break.recv() => {
                clear_runtime_files_if_owned(&store, &owns_runtime);
                anyhow::bail!("Jig proxy interrupted");
            }
            _ = ctrl_close.recv() => {
                clear_runtime_files_if_owned(&store, &owns_runtime);
                anyhow::bail!("Jig proxy console closed");
            }
            _ = ctrl_shutdown.recv() => {
                clear_runtime_files_if_owned(&store, &owns_runtime);
                anyhow::bail!("Jig proxy shutdown requested");
            }
        }
    };

    #[cfg(not(any(unix, windows)))]
    let result = tokio::select! {
        result = run_bound(settings, store.clone(), current_exe, owns_runtime.clone()) => result,
        signal = tokio::signal::ctrl_c() => {
            clear_runtime_files_if_owned(&store, &owns_runtime);
            signal.context("Failed to listen for Ctrl-C")?;
            anyhow::bail!("Jig proxy interrupted");
        }
    };
    result
}

async fn run_bound(
    settings: ProxySettings,
    store: StateStore,
    current_exe: std::path::PathBuf,
    owns_runtime: Arc<AtomicBool>,
) -> Result<()> {
    let bind_host = if settings.lan { "0.0.0.0" } else { "127.0.0.1" };
    let http_listener = bind_listener(bind_host, settings.http_port).await?;
    let http_port = http_listener.local_addr()?.port();

    let https_listener = if settings.https {
        certs::ensure(&settings)?;
        let port = settings.https_port.unwrap_or(1443);
        let listener = bind_listener(bind_host, port).await?;
        Some(listener)
    } else {
        None
    };
    let https_port = https_listener
        .as_ref()
        .map(|listener| listener.local_addr().map(|addr| addr.port()))
        .transpose()?;

    let health_token = match write_runtime_files(&store, &current_exe, http_port, https_port) {
        Ok(token) => token,
        Err(error) => {
            store.clear_runtime_files();
            return Err(error);
        }
    };
    let health_token: Arc<str> = Arc::from(health_token);
    owns_runtime.store(true, Ordering::SeqCst);
    let lan_ip = if settings.lan {
        local_lan_ip_for_ipv4_listener()
    } else {
        None
    };
    eprintln!("jig proxy listening on http://{bind_host}:{http_port}");
    if settings.lan {
        eprintln!(
            "jig proxy LAN mode exposes process-owned loopback routes to devices that can reach this host; aliases remain loopback-client only. Use it only on trusted networks."
        );
    }
    if settings.lan {
        if let Some(ip) = lan_ip {
            eprintln!("jig proxy LAN address: http://{ip}:{http_port}");
        } else {
            eprintln!(
                "jig proxy LAN mode enabled, but no non-loopback IPv4 LAN address was detected for the IPv4 listener"
            );
        }
    }
    if let Some(actual) = https_port {
        eprintln!("jig proxy listening on https://{bind_host}:{actual}");
        if settings.lan {
            if let Some(ip) = lan_ip {
                eprintln!("jig proxy LAN TLS address: https://{ip}:{actual}");
            } else {
                eprintln!(
                    "jig proxy LAN TLS mode enabled, but no non-loopback IPv4 LAN address was detected for the IPv4 listener"
                );
            }
        }
    }

    let runtime_store = store.clone();
    let route_cache = RouteCache::new(store.clone());
    let limits = ProxyLimits::new();
    let listener_context = ListenerContext {
        proxy_port: http_port,
        lan_ip,
        health_token,
    };
    let http = serve_http(
        http_listener,
        route_cache.clone(),
        limits.clone(),
        listener_context.clone(),
    );
    let result = if let Some(listener) = https_listener {
        let https_port = https_port.unwrap_or(settings.https_port.unwrap_or(1443));
        let tls_cache = TlsCache::new(store, settings.http2);
        let https_context = ListenerContext {
            proxy_port: https_port,
            ..listener_context
        };
        tokio::select! {
            result = http => result,
            result = serve_https(listener, route_cache, tls_cache, limits, settings.http2, https_context) => result,
        }
    } else {
        http.await
    };
    clear_runtime_files_if_owned(&runtime_store, &owns_runtime);
    result
}

fn write_runtime_files(
    store: &StateStore,
    current_exe: &std::path::Path,
    http_port: u16,
    https_port: Option<u16>,
) -> Result<String> {
    store.replace_runtime_files(current_exe, http_port, https_port)
}

fn clear_runtime_files_if_owned(store: &StateStore, owns_runtime: &AtomicBool) {
    if owns_runtime.swap(false, Ordering::SeqCst) {
        store.clear_runtime_files();
    }
}

async fn bind_listener(bind_host: &str, port: u16) -> Result<TcpListener> {
    TcpListener::bind((bind_host, port))
        .await
        .map_err(|error| {
            if port < 1024 && port != 0 && error.kind() == std::io::ErrorKind::PermissionDenied {
                anyhow::anyhow!(
                    "Failed to bind privileged port {port}: {error}. Choose a port >= 1024, run the proxy as a root-owned service, or grant the Jig binary bind privileges first. On Linux, use `sudo setcap 'cap_net_bind_service=+ep' <path-to-jig>` for the installed binary; on macOS, use a root LaunchDaemon or a local port-forward from 80/443 to an unprivileged Jig proxy port."
                )
            } else {
                error.into()
            }
        })
}

#[derive(Clone)]
struct RouteCache {
    store: StateStore,
    client: ProxyClient,
    inner: Arc<RwLock<CachedRoutes>>,
}

struct CachedRoutes {
    loaded_at: Option<Instant>,
    signature: FileSignature,
    routes: Arc<Vec<Route>>,
}

impl RouteCache {
    fn new(store: StateStore) -> Self {
        let mut connector = HttpConnector::new();
        connector.set_connect_timeout(Some(BACKEND_CONNECT_TIMEOUT));
        let client = Client::builder(TokioExecutor::new())
            .pool_max_idle_per_host(0)
            .pool_idle_timeout(Duration::from_secs(30))
            .build(connector);
        Self {
            store,
            client,
            inner: Arc::new(RwLock::new(CachedRoutes {
                loaded_at: None,
                signature: None,
                routes: Arc::new(Vec::new()),
            })),
        }
    }

    fn client(&self) -> ProxyClient {
        self.client.clone()
    }

    async fn proxy_listener_ports(&self) -> Result<Vec<u16>> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            let mut ports = Vec::new();
            if let Some(port) = store.read_http_port()? {
                ports.push(port);
            }
            if let Some(port) = store.read_https_port()? {
                ports.push(port);
            }
            ports.sort_unstable();
            ports.dedup();
            Ok::<_, anyhow::Error>(ports)
        })
        .await
        .context("Proxy listener port loading task failed")?
    }

    async fn routes(&self) -> Result<Arc<Vec<Route>>> {
        let signature = current_routes_signature(&self.store).await?;
        {
            let cache = self
                .inner
                .read()
                .map_err(|_| anyhow::anyhow!("jig proxy route cache read lock was poisoned"))?;
            if signature.is_some()
                && cache.signature == signature
                && cache
                    .loaded_at
                    .is_some_and(|loaded_at| loaded_at.elapsed() <= ROUTE_CACHE_MAX_AGE)
            {
                return Ok(Arc::clone(&cache.routes));
            }
        }

        let store = self.store.clone();
        let (routes, signature) =
            tokio::task::spawn_blocking(move || best_effort_routes_snapshot(&store))
                .await
                .context("Route loading task failed")??;

        let mut cache = self
            .inner
            .write()
            .map_err(|_| anyhow::anyhow!("jig proxy route cache write lock was poisoned"))?;
        if signature.is_some()
            && cache.signature == signature
            && cache
                .loaded_at
                .is_some_and(|loaded_at| loaded_at.elapsed() <= ROUTE_CACHE_MAX_AGE)
        {
            return Ok(Arc::clone(&cache.routes));
        }
        if signature.is_some() {
            cache.signature = signature;
            cache.loaded_at = Some(Instant::now());
            cache.routes = Arc::clone(&routes);
        } else {
            cache.signature = None;
            cache.loaded_at = None;
            cache.routes = Arc::new(Vec::new());
        }
        Ok(routes)
    }
}

fn best_effort_routes_snapshot(store: &StateStore) -> Result<(Arc<Vec<Route>>, FileSignature)> {
    let mut signature = store.routes_signature();
    let mut routes = Arc::new(store.read_routes(true)?);
    let after_read = store.routes_signature();
    if after_read != signature {
        signature = after_read;
        routes = Arc::new(store.read_routes(true)?);
        if store.routes_signature() != signature {
            // Route writes raced both reads. Return the latest routes but no
            // signature so callers can serve this request without caching a
            // snapshot whose freshness was not proven.
            return Ok((routes, None));
        }
    }
    Ok((routes, signature))
}

#[derive(Clone)]
struct TlsCache {
    store: StateStore,
    http2: bool,
    inner: Arc<RwLock<CachedTls>>,
}

struct CachedTls {
    signature: TlsSignature,
    loaded_at: Option<Instant>,
    acceptor: Option<TlsAcceptor>,
}

impl TlsCache {
    fn new(store: StateStore, http2: bool) -> Self {
        Self {
            store,
            http2,
            inner: Arc::new(RwLock::new(CachedTls {
                signature: None,
                loaded_at: None,
                acceptor: None,
            })),
        }
    }

    async fn acceptor(&self) -> Result<TlsAcceptor> {
        // Match the route-cache TTL so same-size cert rewrites on coarse-mtime
        // filesystems cannot leave the old certificate pinned indefinitely.
        let signature = current_tls_signature(&self.store).await?;
        {
            let cache = self
                .inner
                .read()
                .map_err(|_| anyhow::anyhow!("jig proxy TLS cache read lock was poisoned"))?;
            if cache.signature == signature
                && cache
                    .loaded_at
                    .is_some_and(|loaded_at| loaded_at.elapsed() <= ROUTE_CACHE_MAX_AGE)
            {
                if let Some(acceptor) = &cache.acceptor {
                    return Ok(acceptor.clone());
                }
            }
        }

        {
            let cache = self
                .inner
                .write()
                .map_err(|_| anyhow::anyhow!("jig proxy TLS cache write lock was poisoned"))?;
            if cache.signature == signature
                && cache
                    .loaded_at
                    .is_some_and(|loaded_at| loaded_at.elapsed() <= ROUTE_CACHE_MAX_AGE)
            {
                if let Some(acceptor) = &cache.acceptor {
                    return Ok(acceptor.clone());
                }
            }
        }

        let store = self.store.clone();
        let http2 = self.http2;
        let acceptor = tokio::task::spawn_blocking(move || tls_acceptor(&store, http2))
            .await
            .context("TLS certificate loading task failed")??;
        let mut cache = self
            .inner
            .write()
            .map_err(|_| anyhow::anyhow!("jig proxy TLS cache write lock was poisoned"))?;
        cache.signature = signature;
        cache.loaded_at = Some(Instant::now());
        cache.acceptor = Some(acceptor.clone());
        Ok(acceptor)
    }
}

fn tls_signature(store: &StateStore) -> TlsSignature {
    Some((
        file_signature(&store.leaf_path()),
        file_signature(&store.leaf_key_path()),
    ))
}

async fn current_routes_signature(store: &StateStore) -> Result<FileSignature> {
    let store = store.clone();
    tokio::task::spawn_blocking(move || store.routes_signature())
        .await
        .context("Route signature task failed")
}

async fn current_tls_signature(store: &StateStore) -> Result<TlsSignature> {
    let store = store.clone();
    tokio::task::spawn_blocking(move || tls_signature(&store))
        .await
        .context("TLS certificate signature task failed")
}

async fn serve_http(
    listener: TcpListener,
    route_cache: RouteCache,
    limits: ProxyLimits,
    listener_context: ListenerContext,
) -> Result<()> {
    let mut consecutive_accept_errors = 0u32;
    loop {
        let permit = match limits.connections.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => bail!("jig proxy http connection limiter closed"),
        };
        let (stream, remote_addr) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                drop(permit);
                consecutive_accept_errors += 1;
                eprintln!("jig proxy http accept error: {error}");
                if consecutive_accept_errors >= MAX_CONSECUTIVE_ACCEPT_ERRORS {
                    bail!(
                        "jig proxy http listener failed {consecutive_accept_errors} consecutive accepts; exiting"
                    );
                }
                sleep(ACCEPT_ERROR_BACKOFF).await;
                continue;
            }
        };
        consecutive_accept_errors = 0;
        let local_ip = stream
            .local_addr()
            .map(|addr| addr.ip())
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        let route_cache = route_cache.clone();
        let limits = limits.clone();
        let listener_context = listener_context.clone();
        tokio::spawn(async move {
            let connection_permit = ConnectionPermit::new(permit);
            let request_context = RequestContext {
                remote_addr,
                proxy_port: listener_context.proxy_port,
                tls: false,
                local_ip,
                lan_ip: listener_context.lan_ip,
                health_token: listener_context.health_token,
            };
            let service = service_fn(move |req| {
                handle_request(
                    req,
                    request_context.clone(),
                    route_cache.clone(),
                    limits.clone(),
                )
            });
            let _connection_permit = connection_permit;
            let io = TokioIo::new(stream);
            let mut http1 = hyper::server::conn::http1::Builder::new();
            http1.timer(TokioTimer::new());
            http1.header_read_timeout(HTTP1_HEADER_READ_TIMEOUT);
            // Keep HTTP/1 single-request so idle clients cannot pin connection permits.
            http1.keep_alive(false);
            let result = http1.serve_connection(io, service).with_upgrades().await;
            if let Err(error) = result {
                if !is_disconnect_error(&error) {
                    eprintln!("jig proxy http connection error: {error}");
                }
            }
        });
    }
}

async fn serve_https(
    listener: TcpListener,
    route_cache: RouteCache,
    tls_cache: TlsCache,
    limits: ProxyLimits,
    http2: bool,
    listener_context: ListenerContext,
) -> Result<()> {
    let mut consecutive_accept_errors = 0u32;
    loop {
        let permit = match limits.connections.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => bail!("jig proxy https connection limiter closed"),
        };
        let (stream, remote_addr) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                drop(permit);
                consecutive_accept_errors += 1;
                eprintln!("jig proxy https accept error: {error}");
                if consecutive_accept_errors >= MAX_CONSECUTIVE_ACCEPT_ERRORS {
                    bail!(
                        "jig proxy https listener failed {consecutive_accept_errors} consecutive accepts; exiting"
                    );
                }
                sleep(ACCEPT_ERROR_BACKOFF).await;
                continue;
            }
        };
        consecutive_accept_errors = 0;
        let local_ip = stream
            .local_addr()
            .map(|addr| addr.ip())
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        let route_cache = route_cache.clone();
        let tls_cache = tls_cache.clone();
        let limits = limits.clone();
        let listener_context = listener_context.clone();
        tokio::spawn(async move {
            let connection_permit = ConnectionPermit::new(permit);
            // This includes certificate reload and the first ClientHello, so
            // stale or idle clients cannot pin connection permits indefinitely.
            let handshake = async {
                let tls = tls_cache
                    .acceptor()
                    .await
                    .context("failed to load TLS certificate")?;
                tls.accept(stream).await.context("TLS handshake failed")
            };
            let stream = match timeout(TLS_HANDSHAKE_TIMEOUT, handshake).await {
                Ok(Ok(stream)) => stream,
                Ok(Err(error)) => {
                    eprintln!("jig proxy TLS handshake from {remote_addr} failed: {error}");
                    return;
                }
                Err(_) => {
                    eprintln!("jig proxy TLS handshake timed out");
                    return;
                }
            };
            let request_context = RequestContext {
                remote_addr,
                proxy_port: listener_context.proxy_port,
                tls: true,
                local_ip,
                lan_ip: listener_context.lan_ip,
                health_token: listener_context.health_token,
            };
            let service = service_fn(move |req| {
                handle_request(
                    req,
                    request_context.clone(),
                    route_cache.clone(),
                    limits.clone(),
                )
            });
            let _connection_permit = connection_permit;
            let io = TokioIo::new(stream);
            if http2 {
                let mut builder =
                    hyper_util::server::conn::auto::Builder::new(TokioExecutor::new());
                builder
                    .http1()
                    .timer(TokioTimer::new())
                    .header_read_timeout(HTTP1_HEADER_READ_TIMEOUT)
                    // HTTP/2 has explicit stream and keepalive limits below.
                    .keep_alive(false);
                builder
                    .http2()
                    .timer(TokioTimer::new())
                    .max_concurrent_streams(Some(HTTP2_MAX_CONCURRENT_STREAMS))
                    .keep_alive_interval(Some(HTTP2_KEEP_ALIVE_INTERVAL))
                    .keep_alive_timeout(HTTP2_KEEP_ALIVE_TIMEOUT)
                    .max_frame_size(Some(HTTP2_MAX_FRAME_SIZE))
                    .max_header_list_size(HTTP2_MAX_HEADER_LIST_SIZE);
                let result = builder.serve_connection_with_upgrades(io, service).await;
                if let Err(error) = result {
                    if !is_disconnect_error(error.as_ref()) {
                        eprintln!("jig proxy https connection error: {error}");
                    }
                }
            } else {
                let mut http1 = hyper::server::conn::http1::Builder::new();
                http1.timer(TokioTimer::new());
                http1.header_read_timeout(HTTP1_HEADER_READ_TIMEOUT);
                // Keep HTTP/1 single-request so idle clients cannot pin connection permits.
                http1.keep_alive(false);
                let result = http1.serve_connection(io, service).with_upgrades().await;
                if let Err(error) = result {
                    if !is_disconnect_error(&error) {
                        eprintln!("jig proxy https connection error: {error}");
                    }
                }
            }
        });
    }
}

async fn handle_request(
    req: Request<Incoming>,
    context: RequestContext,
    route_cache: RouteCache,
    limits: ProxyLimits,
) -> Result<Response<ProxyBody>, Infallible> {
    let request_permit = match limits.requests.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return Ok(error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Too many proxy requests are active.",
            ));
        }
    };
    let response = match route_request(
        req,
        context.clone(),
        route_cache,
        request_permit,
        limits.websockets.clone(),
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!(
                "jig proxy request from {} failed: {error:#}",
                context.remote_addr
            );
            error_response(StatusCode::BAD_GATEWAY, "Bad gateway.")
        }
    };
    Ok(response)
}

async fn route_request(
    req: Request<Incoming>,
    context: RequestContext,
    route_cache: RouteCache,
    request_permit: OwnedSemaphorePermit,
    websocket_limit: Arc<Semaphore>,
) -> Result<Response<ProxyBody>> {
    if req.method() == Method::CONNECT {
        return Ok(error_response(
            StatusCode::METHOD_NOT_ALLOWED,
            "CONNECT requests are not supported by Jig proxy.",
        ));
    }
    if req.uri().path() == "/__jig_proxy_health" {
        // Health requests intentionally bypass normal route host validation,
        // but only after the loopback local/remote address and token checks
        // below succeed. Keep those checks together with this early path.
        if !health_request_allowed(
            &req,
            context.remote_addr.ip(),
            context.local_ip,
            &context.health_token,
        ) {
            return Ok(error_response(StatusCode::FORBIDDEN, "Forbidden."));
        }
        return Ok(health_response());
    }
    let host = match request_host_or_bad_request(&req) {
        Ok(host) => host,
        Err(response) => return Ok(*response),
    };
    let routes = route_cache.routes().await?;
    let Some(route) = find_route(&routes, &host).cloned() else {
        return Ok(not_found_response(
            &routes,
            &host,
            context.proxy_port,
            context.tls,
            ip_is_loopback(context.remote_addr.ip()),
        ));
    };
    if !route_allowed_for_remote_client(&route, context.remote_addr.ip()) {
        return Ok(not_found_response(
            &routes,
            &host,
            context.proxy_port,
            context.tls,
            false,
        ));
    }
    let proxy_ports = route_cache.proxy_listener_ports().await?;
    if route_targets_active_proxy_listener(&route, context.local_ip, context.lan_ip, &proxy_ports) {
        return Ok(error_response(
            StatusCode::BAD_GATEWAY,
            "Jig proxy loop detected.",
        ));
    }

    if is_websocket(req.headers()) {
        if req.method() != Method::GET {
            return Ok(error_response(
                StatusCode::METHOD_NOT_ALLOWED,
                "WebSocket proxying requires a GET upgrade request.",
            ));
        }
        if req.version() != Version::HTTP_11 {
            return Ok(error_response(
                StatusCode::BAD_REQUEST,
                "WebSocket proxying is supported for HTTP/1.1 upgrade requests only.",
            ));
        }
        // The request permit covers the HTTP upgrade handshake only. Once the
        // tunnel is established, the WebSocket semaphore tracks long-lived
        // upgraded connections so they cannot exhaust regular HTTP capacity.
        let version = req.version();
        return websocket(
            req,
            context.remote_addr,
            route,
            context.tls,
            version,
            websocket_limit,
        )
        .await;
    }

    proxy_http(
        req,
        context.remote_addr,
        route_cache.client(),
        route,
        context.tls,
        request_permit,
    )
    .await
}

async fn proxy_http(
    req: Request<Incoming>,
    remote_addr: SocketAddr,
    client: ProxyClient,
    route: Route,
    tls: bool,
    request_permit: OwnedSemaphorePermit,
) -> Result<Response<ProxyBody>> {
    let (mut parts, body) = req.into_parts();
    let inbound_version = parts.version;
    let path = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let uri: Uri = format!(
        "http://{}:{}{}",
        target_authority_host(&route.target_host),
        route.target_port,
        path
    )
    .parse()?;
    parts.uri = uri;
    // The legacy Hyper client is intentionally configured with `build_http()`,
    // so backend requests are normalized to HTTP/1.1 even when the client side
    // accepted HTTP/2 over TLS.
    parts.version = Version::HTTP_11;
    if let Err(error) = rewrite_proxy_headers(
        &mut parts.headers,
        remote_addr,
        &route,
        tls,
        inbound_version,
    ) {
        return Ok(error_response(
            StatusCode::BAD_REQUEST,
            &format!("Invalid Jig proxy headers: {error}"),
        ));
    }
    match request_content_length(&parts.headers) {
        Ok(Some(length)) if length > MAX_BACKEND_REQUEST_BODY_BYTES as u64 => {
            return Ok(error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Request body exceeds Jig proxy forwarding limit.",
            ));
        }
        Ok(_) => {}
        Err(_) => {
            return Ok(error_response(
                StatusCode::BAD_REQUEST,
                "Invalid request Content-Length.",
            ));
        }
    }
    let body = limited_request_body(body);
    let proxied = Request::from_parts(parts, body);
    let response = match timeout(BACKEND_HTTP_RESPONSE_TIMEOUT, client.request(proxied)).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) if error_chain_contains_length_limit(&error) => {
            return Ok(error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Request body exceeds Jig proxy forwarding limit.",
            ));
        }
        Ok(Err(error)) => return Err(error.into()),
        Err(_) => anyhow::bail!("Timed out waiting for backend response"),
    };
    let (mut parts, body) = response.into_parts();
    remove_hop_by_hop_headers(&mut parts.headers);
    remove_jig_proxy_headers(&mut parts.headers);
    append_via(&mut parts.headers, inbound_version);
    parts
        .headers
        .insert("x-jig-proxy", HeaderValue::from_static("1"));
    Ok(Response::from_parts(
        parts,
        body_with_request_permit(body, request_permit),
    ))
}

fn body_with_request_permit(body: Incoming, permit: OwnedSemaphorePermit) -> ProxyBody {
    let permit = Arc::new(permit);
    body.map_frame(move |frame| {
        let _permit = &permit;
        frame
    })
    .boxed_unsync()
}

fn limited_request_body(body: Incoming) -> ClientBody {
    Limited::new(body, MAX_BACKEND_REQUEST_BODY_BYTES).boxed_unsync()
}

async fn websocket(
    req: Request<Incoming>,
    remote_addr: SocketAddr,
    route: Route,
    tls: bool,
    version: Version,
    websocket_limit: Arc<Semaphore>,
) -> Result<Response<ProxyBody>> {
    if websocket_request_has_body(req.headers()) {
        return Ok(error_response(
            StatusCode::BAD_REQUEST,
            "WebSocket upgrade requests with request bodies are not supported.",
        ));
    }
    let websocket_permit = match websocket_limit.try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return Ok(error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Too many WebSocket tunnels are already open.",
            ));
        }
    };
    let path = req
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    if path.chars().any(|ch| ch.is_control() || ch == ' ') {
        return Ok(error_response(
            StatusCode::BAD_REQUEST,
            "WebSocket request path contains invalid characters.",
        ));
    }
    let websocket_accept = match websocket_accept_for_request(req.headers()) {
        Ok(value) => value,
        Err(error) => {
            return Ok(error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid WebSocket upgrade request: {error}"),
            ));
        }
    };
    let target_ip = route
        .target_host
        .parse::<IpAddr>()
        .with_context(|| format!("Route target '{}' must be an IP literal", route.target_host))?;
    let mut backend = timeout(
        BACKEND_CONNECT_TIMEOUT,
        TcpStream::connect(SocketAddr::new(target_ip, route.target_port)),
    )
    .await
    .context("Timed out connecting to WebSocket backend")??;
    let mut headers = req.headers().clone();
    if let Err(error) = rewrite_proxy_headers(&mut headers, remote_addr, &route, tls, version) {
        return Ok(error_response(
            StatusCode::BAD_REQUEST,
            &format!("Invalid Jig proxy headers: {error}"),
        ));
    }
    headers.remove(CONTENT_LENGTH);
    headers.insert(CONNECTION, HeaderValue::from_static("Upgrade"));
    headers.insert(UPGRADE, HeaderValue::from_static("websocket"));
    let mut raw = format!("{} {} HTTP/1.1\r\n", req.method(), path);
    for (name, value) in &headers {
        if let Ok(value) = value.to_str() {
            // This manual HTTP/1.1 upgrade request is assembled from hyper
            // values: Method is typed, path_and_query was parsed by hyper, and
            // copied header names are already validated HeaderName values.
            // HeaderValue::to_str rejects CR/LF before copied values are used.
            if value.contains('\r') || value.contains('\n') {
                continue;
            }
            raw.push_str(name.as_str());
            raw.push_str(": ");
            raw.push_str(value);
            raw.push_str("\r\n");
        }
    }
    raw.push_str("\r\n");
    timeout(
        WEBSOCKET_BACKEND_WRITE_TIMEOUT,
        backend.write_all(raw.as_bytes()),
    )
    .await
    .context("Timed out writing WebSocket upgrade request to backend")??;

    let (status, headers, buffered) = timeout(
        BACKEND_HTTP_RESPONSE_TIMEOUT,
        read_backend_headers(&mut backend),
    )
    .await
    .context("Timed out waiting for WebSocket backend response headers")??;
    if status != StatusCode::SWITCHING_PROTOCOLS {
        // Non-101 WebSocket responses are usually small auth/error pages.
        // Drain them with explicit byte and time bounds before replying.
        let body = match timeout(
            BACKEND_BODY_DRAIN_TIMEOUT,
            complete_backend_body(&mut backend, &headers, buffered),
        )
        .await
        {
            Ok(result) => result?,
            Err(_) => anyhow::bail!("Timed out draining backend non-upgrade WebSocket response"),
        };
        let connection_headers = raw_connection_header_names(&headers);
        let mut builder = Response::builder().status(status);
        for (name, value) in headers {
            if raw_header_is_hop_by_hop(&name, &connection_headers)
                || name == CONTENT_LENGTH
                || is_jig_proxy_header(&name)
            {
                continue;
            }
            builder = builder.header(name, value);
        }
        builder = builder
            .header(CONTENT_LENGTH, body.len().to_string())
            .header(VIA, via_value(version));
        let mut response = builder.body(full_body(body))?;
        response
            .headers_mut()
            .insert("x-jig-proxy", HeaderValue::from_static("1"));
        return Ok(response);
    }
    if !websocket_accept_matches(&headers, &websocket_accept) {
        return Ok(error_response(
            StatusCode::BAD_GATEWAY,
            "WebSocket backend returned an invalid upgrade response.",
        ));
    }
    if !websocket_extensions_allowed(req.headers(), &headers) {
        return Ok(error_response(
            StatusCode::BAD_GATEWAY,
            "WebSocket backend negotiated unsupported extensions.",
        ));
    }
    if !websocket_subprotocol_allowed(req.headers(), &headers) {
        return Ok(error_response(
            StatusCode::BAD_GATEWAY,
            "WebSocket backend negotiated an unsupported subprotocol.",
        ));
    }
    let upgrade = hyper::upgrade::on(req);
    tokio::spawn(async move {
        let _websocket_permit = websocket_permit;
        let upgraded = match timeout(WEBSOCKET_UPGRADE_TIMEOUT, upgrade).await {
            Ok(Ok(upgraded)) => upgraded,
            Ok(Err(error)) => {
                eprintln!("jig proxy websocket upgrade failed after 101 response: {error}");
                return;
            }
            Err(_) => {
                eprintln!("jig proxy websocket upgrade timed out after 101 response");
                return;
            }
        };
        let mut client = TokioIo::new(upgraded);
        if !buffered.is_empty() {
            match timeout(WEBSOCKET_BACKEND_WRITE_TIMEOUT, client.write_all(&buffered)).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    eprintln!(
                        "jig proxy websocket failed to flush buffered backend bytes: {error}"
                    );
                    return;
                }
                Err(_) => {
                    eprintln!("jig proxy websocket timed out flushing buffered backend bytes");
                    return;
                }
            }
        }
        tunnel_websocket(client, backend).await;
    });

    let connection_headers = raw_connection_header_names(&headers);
    let mut builder = Response::builder().status(status);
    for (name, value) in headers {
        if raw_header_is_hop_by_hop(&name, &connection_headers) || is_jig_proxy_header(&name) {
            continue;
        }
        builder = builder.header(name, value);
    }
    let mut response = builder.body(empty_body())?;
    let headers = response.headers_mut();
    headers.insert(CONNECTION, HeaderValue::from_static("Upgrade"));
    headers.insert(UPGRADE, HeaderValue::from_static("websocket"));
    headers.append(VIA, header_value_or_default(via_value(version), VIA_VALUE));
    headers.insert("x-jig-proxy", HeaderValue::from_static("1"));
    Ok(response)
}

fn websocket_accept_for_request(headers: &HeaderMap) -> Result<String> {
    let mut keys = headers.get_all("sec-websocket-key").iter();
    let key = match (keys.next(), keys.next()) {
        (Some(key), None) => key,
        (None, _) => bail!("WebSocket request missing Sec-WebSocket-Key"),
        (Some(_), Some(_)) => bail!("WebSocket request has conflicting Sec-WebSocket-Key headers"),
    }
    .to_str()
    .context("WebSocket request has non-UTF8 Sec-WebSocket-Key")?;
    Ok(websocket_accept_for_key(key))
}

fn websocket_accept_for_key(key: &str) -> String {
    let mut sha1 = Sha1::new();
    sha1.update(key.as_bytes());
    sha1.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    BASE64_STANDARD.encode(sha1.finalize())
}

fn websocket_accept_matches(headers: &[(HeaderName, HeaderValue)], expected: &str) -> bool {
    let mut actual = None;
    for (_, value) in headers
        .iter()
        .filter(|(name, _)| name.as_str().eq_ignore_ascii_case("sec-websocket-accept"))
    {
        let Ok(value) = value.to_str() else {
            return false;
        };
        if actual.replace(value.trim()).is_some() {
            return false;
        }
    }
    actual == Some(expected)
}

fn websocket_extensions_allowed(
    request_headers: &HeaderMap,
    response_headers: &[(HeaderName, HeaderValue)],
) -> bool {
    let requested = websocket_extension_names(
        request_headers
            .get_all("sec-websocket-extensions")
            .iter()
            .filter_map(|value| value.to_str().ok()),
    );
    response_headers
        .iter()
        .filter(|(name, _)| {
            name.as_str()
                .eq_ignore_ascii_case("sec-websocket-extensions")
        })
        .all(|(_, value)| {
            value.to_str().ok().is_some_and(|value| {
                websocket_extension_names(std::iter::once(value))
                    .iter()
                    .all(|extension| requested.contains(extension))
            })
        })
}

fn websocket_extension_names<'a>(values: impl IntoIterator<Item = &'a str>) -> HashSet<String> {
    values
        .into_iter()
        .flat_map(|value| value.split(','))
        .filter_map(|extension| extension.split(';').next())
        .map(str::trim)
        .filter(|extension| !extension.is_empty())
        .map(|extension| extension.to_ascii_lowercase())
        .collect()
}

fn websocket_subprotocol_allowed(
    request_headers: &HeaderMap,
    response_headers: &[(HeaderName, HeaderValue)],
) -> bool {
    let requested = websocket_token_values(
        request_headers
            .get_all("sec-websocket-protocol")
            .iter()
            .filter_map(|value| value.to_str().ok()),
    );
    let mut selected = Vec::new();
    for (_, value) in response_headers
        .iter()
        .filter(|(name, _)| name.as_str().eq_ignore_ascii_case("sec-websocket-protocol"))
    {
        let Ok(value) = value.to_str() else {
            return false;
        };
        selected.extend(websocket_token_values(std::iter::once(value)));
    }
    match selected.as_slice() {
        [] => true,
        [protocol] => requested.iter().any(|item| item == protocol),
        _ => false,
    }
}

fn websocket_token_values<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    values
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .collect()
}

fn is_disconnect_error(error: &(dyn Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if let Some(io_error) = error.downcast_ref::<std::io::Error>() {
            if matches!(
                io_error.kind(),
                std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::ConnectionReset
            ) {
                return true;
            }
        }
        current = error.source();
    }
    false
}

fn error_chain_contains_length_limit(error: &(dyn Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if error.downcast_ref::<LengthLimitError>().is_some() {
            return true;
        }
        current = error.source();
    }
    false
}

#[cfg(test)]
mod tests;
