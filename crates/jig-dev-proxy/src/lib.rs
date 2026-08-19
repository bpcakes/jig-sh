//! Internal local-development proxy runtime for the `jig-sh` CLI.
//!
//! This crate is published only so the CLI can split and test the proxy
//! runtime independently. Its `anyhow::Result<serde_json::Value>` API is not a
//! stable third-party integration surface outside the matching `jig-sh`
//! release. See `crates/jig-dev-proxy/AGENTS.md` for the security and platform
//! invariants that maintainers should preserve when editing this crate.

mod certs;
mod dev_api;
mod dev_sessions;
mod file_ops;
mod host;
mod ports;
mod processes;
mod server;
mod service;
mod session_control;
mod session_id;
mod state;
mod types;
#[cfg(any(windows, test))]
mod windows_system;
mod workspace;

use std::collections::HashMap;
use std::fs;
use std::net::IpAddr;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

#[cfg(test)]
pub(crate) fn test_tempdir() -> std::io::Result<tempfile::TempDir> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::Once;

        static TEST_UMASK: Once = Once::new();
        TEST_UMASK.call_once(|| {
            // SAFETY: this is test-only process setup; the test process exits
            // after the suite, so the private umask does not escape to callers.
            unsafe { libc::umask(0o077) };
        });

        tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
    }

    #[cfg(not(unix))]
    {
        tempfile::tempdir()
    }
}

use crate::host::{RouteHostname, TargetHost};
use crate::ports::{is_tcp_listening, jig_proxy_http_pid, local_lan_ip_for_ipv4_listener};
use crate::state::{PidObservation, StateStore, now_ms, observe_pid};
use crate::types::{Route, RouteMode};

pub use crate::dev_api::{dev, dev_resolved, dev_status, dev_stop};
pub use crate::state::MAX_ROUTES_FILE_BYTES;
pub use crate::types::{
    AppKind, AppRunSpec, CommandSpec, DevRequest, DevStatusRequest, DevStopRequest,
    ProxyAliasRequest, ProxyCertRequest, ProxyListRequest, ProxyPruneRequest, ProxyRunRequest,
    ProxyServiceRequest, ProxySettings, ProxyStartRequest, ProxyStopRequest,
};

/// Internal compatibility hook used to prove that generated Jig configuration
/// stays aligned with the dev runtime's narrow npm environment boundary.
#[doc(hidden)]
pub fn is_generated_npm_dev_argv(argv: &[String]) -> bool {
    processes::is_generated_npm_dev_argv(argv)
}

/// The outcome of a caller-owned development preflight.
#[derive(Debug)]
pub enum DevPreflightError {
    /// The preflight observed the supplied cancellation probe and stopped.
    Cancelled,
    /// The preflight failed independently of a termination request.
    Failed(anyhow::Error),
    /// The preflight failed and could not confirm cleanup of its owned process
    /// tree.
    CleanupUnconfirmed(anyhow::Error),
}

impl DevPreflightError {
    pub const fn cancelled() -> Self {
        Self::Cancelled
    }

    pub const fn failed(error: anyhow::Error) -> Self {
        Self::Failed(error)
    }

    pub const fn cleanup_unconfirmed(error: anyhow::Error) -> Self {
        Self::CleanupUnconfirmed(error)
    }

    pub(crate) const fn cleanup_was_confirmed(&self) -> bool {
        !matches!(self, Self::CleanupUnconfirmed(_))
    }
}

impl From<anyhow::Error> for DevPreflightError {
    fn from(error: anyhow::Error) -> Self {
        Self::Failed(error)
    }
}

pub type DevPreflightResult = std::result::Result<(), DevPreflightError>;

/// A development launch plan after workspace discovery, app selection, and
/// no-proxy overrides have been applied.
pub struct ResolvedDevRequest {
    repo_name: String,
    root: std::path::PathBuf,
    settings: ProxySettings,
    apps: Vec<AppRunSpec>,
    replace: bool,
}

impl ResolvedDevRequest {
    pub fn apps(&self) -> &[AppRunSpec] {
        &self.apps
    }
}

/// Resolves workspace discovery, application selection, and repository paths.
///
/// # Errors
///
/// Returns an error when the repository or app paths are invalid, workspace
/// discovery fails, a requested app is unavailable, or resolved paths escape
/// the repository.
pub fn resolve_dev_request(request: DevRequest) -> Result<ResolvedDevRequest> {
    let root = fs::canonicalize(&request.root).with_context(|| {
        format!(
            "Failed to canonicalize repo root {}",
            request.root.display()
        )
    })?;
    let mut specs = request.apps;
    if request.discover_workspace {
        specs.extend(workspace::discover(
            &root,
            &request.repo_name,
            &request.settings.tld,
            &request.package_manager,
        )?);
    }
    if !request.selected_apps.is_empty() {
        let available_apps: Vec<_> = specs.iter().map(|spec| spec.name.clone()).collect();
        let mut unavailable_apps = Vec::new();
        for name in &request.selected_apps {
            if !available_apps.contains(name) && !unavailable_apps.contains(name) {
                unavailable_apps.push(name.clone());
            }
        }
        if !unavailable_apps.is_empty() {
            bail!(
                "No development apps matched --app filter '{}'. Available apps: {}",
                unavailable_apps.join(", "),
                if available_apps.is_empty() {
                    "<none>".into()
                } else {
                    available_apps.join(", ")
                }
            );
        }
        specs.retain(|spec| request.selected_apps.iter().any(|name| name == &spec.name));
    }
    if request.no_proxy {
        for spec in &mut specs {
            spec.proxy = false;
        }
    }
    if specs.is_empty() {
        bail!("No development apps were configured or discovered.");
    }
    ensure_unique_specs(&specs)?;
    resolve_selected_app_directories(&root, &mut specs)?;
    Ok(ResolvedDevRequest {
        repo_name: request.repo_name,
        root,
        settings: request.settings,
        apps: specs,
        replace: request.replace,
    })
}

fn resolve_selected_app_directories(root: &Path, specs: &mut [AppRunSpec]) -> Result<()> {
    for spec in specs {
        let candidate = if spec.dir.is_absolute() {
            spec.dir.clone()
        } else {
            root.join(&spec.dir)
        };
        let resolved = fs::canonicalize(&candidate).with_context(|| {
            format!(
                "development app '{}' directory {} must exist",
                spec.name,
                candidate.display()
            )
        })?;
        if !resolved.starts_with(root) {
            bail!(
                "development app '{}' directory {} resolves outside repo root {}",
                spec.name,
                candidate.display(),
                root.display()
            );
        }
        spec.dir = resolved;
    }
    Ok(())
}

/// Runs a resolved development plan under one foreground termination session,
/// including a caller-owned preflight that can poll for cancellation.
///
/// # Errors
///
/// Returns an error when executable discovery, preflight, process supervision,
/// app readiness, proxy routing, or cleanup fails.
pub fn dev_resolved_with_preflight(
    request: ResolvedDevRequest,
    preflight: impl FnOnce(&[AppRunSpec], &dyn Fn() -> bool) -> DevPreflightResult,
) -> Result<Value> {
    let current_exe = current_exe()?;
    dev_api::normalize_dev_result(processes::run_apps_with_preflight(
        &request.repo_name,
        &request.root,
        request.apps,
        &request.settings,
        &current_exe,
        request.replace,
        preflight,
    ))
}

fn normalize_proxy_run_result(result: Result<Value>, app: &str, hostname: &str) -> Result<Value> {
    match result {
        Err(error) => {
            let Some(reason) = processes::interruption_reason(&error) else {
                return Err(error);
            };
            Ok(json!({
                "ok": false,
                "interrupted": true,
                "exit_status": reason.exit_status(),
                "exit_signal": reason.signal(),
                "termination_signal": reason.label(),
                "app": app,
                "hostname": hostname,
                "port": null,
            }))
        }
        result => result,
    }
}

fn normalize_proxy_start_result(result: Result<()>) -> Result<Option<Value>> {
    match result {
        Err(error) => {
            let Some(reason) = processes::interruption_reason(&error) else {
                return Err(error);
            };
            Ok(Some(json!({
                "ok": false,
                "interrupted": true,
                "exit_status": reason.exit_status(),
                "exit_signal": reason.signal(),
                "termination_signal": reason.label(),
                "foreground": false,
                "http_port": null,
                "https_port": null,
            })))
        }
        Ok(()) => Ok(None),
    }
}

/// Starts the local proxy in the foreground or as a managed background process.
///
/// # Errors
///
/// Returns an error when executable or state resolution, certificate loading,
/// listener startup, process supervision, or runtime-state inspection fails.
pub fn proxy_start(request: ProxyStartRequest) -> Result<Value> {
    let current_exe = current_exe()?;
    if request.foreground {
        // Foreground mode owns the process until shutdown. This return value is
        // only reached if the listener loop exits cleanly.
        server::run_foreground(request.settings, current_exe)?;
        return Ok(json!({ "ok": true, "foreground": true }));
    }
    if let Some(interrupted) = normalize_proxy_start_result(processes::ensure_proxy_running(
        &request.settings,
        &current_exe,
    ))? {
        return Ok(interrupted);
    }
    let store = StateStore::resolve(request.settings.state_dir.clone())?;
    Ok(json!({
        "ok": true,
        "foreground": false,
        "http_port": store.read_http_port()?,
        "https_port": store.read_https_port()?,
        "bind_host": if request.settings.lan { "0.0.0.0" } else { "127.0.0.1" },
        "lan_host": if request.settings.lan { local_lan_ip_for_ipv4_listener().map(|ip| ip.to_string()) } else { None },
        "state_dir": store.root(),
    }))
}

/// Stops the matching local proxy when its ownership can be verified.
///
/// # Errors
///
/// Returns an error when state or service inspection fails, proxy ownership is
/// uncertain, termination fails, or runtime cleanup cannot be confirmed.
pub fn proxy_stop(request: ProxyStopRequest) -> Result<Value> {
    proxy_stop_with_service_probe(request, proxy_service_status_snapshot)
}

fn proxy_stop_with_service_probe(
    request: ProxyStopRequest,
    service_probe: impl FnOnce(
        &ProxySettings,
        &std::path::Path,
    ) -> Result<Option<service::ServiceStatusSnapshot>>,
) -> Result<Value> {
    proxy_stop_with_service_probe_and_terminator(request, service_probe, terminate_proxy_pid)
}

fn proxy_stop_with_service_probe_and_terminator(
    request: ProxyStopRequest,
    service_probe: impl FnOnce(
        &ProxySettings,
        &std::path::Path,
    ) -> Result<Option<service::ServiceStatusSnapshot>>,
    mut terminate_proxy: impl FnMut(u32) -> bool,
) -> Result<Value> {
    if let Some(path) = missing_state_dir(&request.settings)? {
        let service_status =
            ProbedServiceStatus::from_result(service_probe(&request.settings, &path));
        let service_degrades_stop_ok = service_status.degrades_stop_ok();
        return Ok(json!({
            "ok": !service_degrades_stop_ok,
            "stopped": false,
            "runtime_files_cleared": false,
            "pid": null,
            "pid_alive": false,
            "pid_observation": null,
            "probed_http_port": null,
            "health_pid": null,
            "handshake_ok": false,
            "pid_matches_proxy": false,
            "service_keepalive_active": service_status.keepalive_active(),
            "service_status_uncertain": service_status.is_uncertain(),
            "service": service_status.value(),
            "warning": missing_state_warning(
                &path,
                service_status.keepalive_active(),
                service_status.is_uncertain(),
            ),
            "state_dir": path,
        }));
    }
    let store = StateStore::resolve(request.settings.state_dir.clone())?;
    let status = proxy_runtime_status(&store)?;
    let pid = status.pid;
    let pid_alive = status.pid_is_alive();
    let pid_may_be_alive = status.pid_may_be_alive();
    let probed_http_port = status.http_port;
    let health_pid = status.health_pid;
    let handshake_ok = status.handshake_ok;
    let pid_matches_proxy = status.pid_matches_proxy;
    let service_status =
        ProbedServiceStatus::from_result(service_probe(&request.settings, store.root()));
    let service_degrades_stop_ok = service_status.degrades_stop_ok();
    let mut stopped = false;
    if !service_degrades_stop_ok {
        if let Some(stop_pid) = proxy_stop_target_pid(pid, pid_may_be_alive, pid_matches_proxy) {
            stopped = terminate_proxy(stop_pid);
        }
    }
    let authenticated_proxy_still_running = handshake_ok && !stopped;
    let should_clear_runtime_files = !service_degrades_stop_ok
        && (stopped
            || (!authenticated_proxy_still_running && (pid.is_none() || !pid_may_be_alive)));
    let runtime_files_cleared = if should_clear_runtime_files {
        match store.try_clear_runtime_files() {
            Ok(()) => true,
            Err(error) => {
                eprintln!(
                    "jig proxy could not clear runtime files in {}: {error}",
                    store.root().display()
                );
                false
            }
        }
    } else {
        false
    };
    // ok=false is used for actionable stop caveats: callers should treat the
    // JSON warning as the result instead of assuming the proxy will stay down.
    let ok = runtime_files_cleared && !service_degrades_stop_ok;
    Ok(json!({
        "ok": ok,
        "stopped": stopped,
        "runtime_files_cleared": runtime_files_cleared,
        "pid": pid,
        "pid_alive": pid_alive,
        "pid_observation": status.pid_observation.map(PidObservation::label),
        "probed_http_port": probed_http_port,
        "health_pid": health_pid,
        "handshake_ok": handshake_ok,
        "pid_matches_proxy": pid_matches_proxy,
        "service_keepalive_active": service_status.keepalive_active(),
        "service_status_uncertain": service_status.is_uncertain(),
        "service": service_status.value(),
        "warning": stop_warning(StopWarningStatus {
            pid,
            health_pid,
            pid_observation: status.pid_observation,
            handshake_ok,
            pid_matches_proxy,
            stopped,
            service_keepalive_active: service_status.keepalive_active(),
            service_status_uncertain: service_status.is_uncertain(),
        }),
        "state_dir": store.root(),
    }))
}

const fn proxy_stop_target_pid(
    pid: Option<u32>,
    pid_may_be_alive: bool,
    pid_matches_proxy: bool,
) -> Option<u32> {
    if pid_may_be_alive && pid_matches_proxy {
        return pid;
    }
    None
}

fn missing_state_warning(
    state_dir: &std::path::Path,
    service_keepalive_active: bool,
    service_status_uncertain: bool,
) -> Option<String> {
    if service_keepalive_active {
        return Some(format!(
            "Jig proxy state dir {} is missing, but a proxy user service may start the proxy. Run `jig proxy service uninstall` to disable the keepalive service, or `jig proxy service status` to inspect it.",
            state_dir.display()
        ));
    }
    if service_status_uncertain {
        return Some(format!(
            "Jig proxy state dir {} is missing, and Jig could not inspect the proxy user service status. Run `jig proxy service status` to verify whether an installed service may start the proxy; if status reports missing state-dir metadata, reinstall it with `jig proxy service install --accept-service-scope`.",
            state_dir.display()
        ));
    }
    None
}

#[derive(Clone, Copy, Debug)]
struct StopWarningStatus {
    pid: Option<u32>,
    health_pid: Option<u32>,
    pid_observation: Option<PidObservation>,
    handshake_ok: bool,
    pid_matches_proxy: bool,
    stopped: bool,
    service_keepalive_active: bool,
    service_status_uncertain: bool,
}

fn stop_warning(status: StopWarningStatus) -> Option<String> {
    let StopWarningStatus {
        pid,
        health_pid,
        pid_observation,
        handshake_ok,
        pid_matches_proxy,
        stopped,
        service_keepalive_active,
        service_status_uncertain,
    } = status;
    let pid_may_be_alive = pid_observation.is_some_and(PidObservation::may_be_alive);

    if stopped {
        if service_keepalive_active {
            return Some(
                "Jig proxy process stopped, but a proxy user service may restart it immediately. Run `jig proxy service uninstall` to disable the keepalive service, or `jig proxy service status` to inspect it."
                    .into(),
            );
        }
        if service_status_uncertain {
            return Some(
                "Jig proxy process stopped, but Jig could not inspect the proxy user service status. Run `jig proxy service status` to verify whether an installed service may restart it; if status reports missing state-dir metadata, reinstall it with `jig proxy service install --accept-service-scope`."
                    .into(),
            );
        }
        return None;
    }
    if pid.is_none() {
        if service_keepalive_active {
            return Some(
                "No Jig proxy PID file was present, but a proxy user service may start the proxy. Run `jig proxy service uninstall` to disable the keepalive service, or `jig proxy service status` to inspect it."
                    .into(),
            );
        }
        if service_status_uncertain {
            return Some(
                "No Jig proxy PID file was present, and Jig could not inspect the proxy user service status. Run `jig proxy service status` to verify whether an installed service may start the proxy; if status reports missing state-dir metadata, reinstall it with `jig proxy service install --accept-service-scope`."
                    .into(),
            );
        }
    }
    if handshake_ok && !stopped && !pid_may_be_alive {
        let health_pid = health_pid?;
        let pid_detail = pid
            .map(|pid| format!("stale PID file points at {pid}"))
            .unwrap_or_else(|| "no PID file was present".to_string());
        return Some(format!(
            "A Jig proxy answered on the stored port as PID {health_pid}, but {pid_detail}. Runtime files were kept because the authenticated proxy could not be stopped."
        ));
    }
    let pid = pid?;
    if service_keepalive_active {
        return Some(format!(
            "Jig proxy PID file points at process {pid}, but no process was stopped and runtime files were kept because a proxy user service may restart the proxy. Run `jig proxy service uninstall` to disable the keepalive service, or `jig proxy service status` to inspect it."
        ));
    }
    if service_status_uncertain {
        return Some(format!(
            "Jig proxy PID file points at process {pid}, but no process was stopped and runtime files were kept because Jig could not inspect the proxy user service status. Run `jig proxy service status` to verify whether an installed service may restart it; if status reports missing state-dir metadata, reinstall it with `jig proxy service install --accept-service-scope`."
        ));
    }
    let service_suffix = if service_keepalive_active {
        " A proxy user service may restart the proxy; run `jig proxy service uninstall` to disable it."
    } else if service_status_uncertain {
        " Jig could not inspect the proxy user service status; run `jig proxy service status` to verify whether an installed service may restart it. If status reports missing state-dir metadata, reinstall it with `jig proxy service install --accept-service-scope`."
    } else {
        ""
    };
    if pid_observation == Some(PidObservation::Uncertain) && !handshake_ok {
        return Some(format!(
            "PID file points at process {pid}, but Jig could not classify whether that PID is still alive and it did not answer the Jig proxy health check. Runtime files were kept to avoid hiding or terminating an unrelated process; inspect the PID or remove the state dir after confirming it is stale.{service_suffix}"
        ));
    }
    if pid_may_be_alive && !handshake_ok {
        return Some(format!(
            "PID file points at process {pid}, but it did not answer the Jig proxy health check. Runtime files were kept to avoid hiding or terminating an unrelated process; inspect the PID or remove the state dir after confirming it is stale.{service_suffix}"
        ));
    }
    if pid_may_be_alive && handshake_ok && !pid_matches_proxy {
        let Some(health_pid) = health_pid else {
            return Some(format!(
                "A Jig proxy answered the stored health check, but the health response did not include a PID while the PID file points at {pid}. Runtime files were kept to avoid terminating an unrelated process.{service_suffix}"
            ));
        };
        return Some(format!(
            "A Jig proxy answered on the stored port as PID {health_pid}, but the PID file points at {pid}. Runtime files were kept to avoid terminating an unrelated process; use the matching JIG_PROXY_STATE_DIR or stop the other proxy explicitly.{service_suffix}"
        ));
    }
    if pid_may_be_alive && handshake_ok {
        return Some(format!(
            "Jig proxy process {pid} answered the health check but did not exit after stop signals. Runtime files were kept because the process may still own its ports.{service_suffix}"
        ));
    }
    if !pid_may_be_alive {
        return Some(format!(
            "Stale Jig proxy PID file for process {pid} was found and cleared.{service_suffix}"
        ));
    }
    None
}

#[cfg(unix)]
fn terminate_proxy_pid(pid: u32) -> bool {
    let Some(unix_pid) = unix_pid(pid) else {
        return false;
    };
    match signal_proxy_pid(unix_pid, libc::SIGTERM) {
        SignalResult::Sent => {
            if wait_for_pid_exit(pid, Duration::from_secs(2)) {
                return true;
            }
        }
        SignalResult::NotFound => return true,
        SignalResult::Failed => {}
    }
    match signal_proxy_pid(unix_pid, libc::SIGKILL) {
        SignalResult::Sent => wait_for_pid_exit(pid, Duration::from_secs(1)),
        SignalResult::NotFound => true,
        SignalResult::Failed => false,
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SignalResult {
    Sent,
    NotFound,
    Failed,
}

#[cfg(unix)]
fn signal_proxy_pid(pid: i32, signal: i32) -> SignalResult {
    let result = unsafe {
        // SAFETY: pid was range-checked by unix_pid and signal is one of the
        // libc constants used for process termination.
        libc::kill(pid, signal)
    };
    if result == 0 {
        SignalResult::Sent
    } else {
        classify_signal_error(std::io::Error::last_os_error().raw_os_error())
    }
}

#[cfg(unix)]
fn classify_signal_error(raw_os_error: Option<i32>) -> SignalResult {
    if raw_os_error == Some(libc::ESRCH) {
        SignalResult::NotFound
    } else {
        SignalResult::Failed
    }
}

#[cfg(unix)]
pub(crate) fn unix_pid(pid: u32) -> Option<i32> {
    i32::try_from(pid).ok()
}

#[cfg(not(any(unix, windows)))]
fn terminate_proxy_pid(_pid: u32) -> bool {
    false
}

#[cfg(windows)]
fn terminate_proxy_pid(pid: u32) -> bool {
    let Ok(taskkill) = crate::windows_system::native_system_executable("taskkill.exe") else {
        return false;
    };
    let Ok(status) = std::process::Command::new(&taskkill)
        .env_clear()
        .args(["/PID", &pid.to_string(), "/T"])
        .status()
    else {
        return false;
    };
    if status.success() && wait_for_pid_exit(pid, Duration::from_secs(2)) {
        return true;
    }
    let Ok(status) = std::process::Command::new(taskkill)
        .env_clear()
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
    else {
        return false;
    };
    status.success() && wait_for_pid_exit(pid, Duration::from_secs(1))
}

fn wait_for_pid_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if observe_pid(pid).is_absent() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    observe_pid(pid).is_absent()
}

/// Lists proxy routes and runtime status.
///
/// # Errors
///
/// Returns an error when proxy state, route data, process identity, or service
/// status cannot be read or validated safely.
pub fn proxy_list(request: ProxyListRequest) -> Result<Value> {
    proxy_list_with_service_probe(request, proxy_service_status_snapshot)
}

fn proxy_list_with_service_probe(
    request: ProxyListRequest,
    service_probe: impl FnOnce(
        &ProxySettings,
        &std::path::Path,
    ) -> Result<Option<service::ServiceStatusSnapshot>>,
) -> Result<Value> {
    if let Some(path) = missing_state_dir(&request.settings)? {
        let service_status =
            ProbedServiceStatus::from_result(service_probe(&request.settings, &path));
        return Ok(json!({
            "ok": true,
            "state_dir": path,
            "http_port": null,
            "https_port": null,
            "pid": null,
            "pid_alive": false,
            "pid_observation": null,
            "health_pid": null,
            "handshake_ok": false,
            "pid_matches_proxy": false,
            "running": false,
            "proxy_exe": null,
            "proxy_exe_note": null,
            "proxy_exe_warning": null,
            "service_keepalive_active": service_status.keepalive_active(),
            "service_status_uncertain": service_status.is_uncertain(),
            "service": service_status.value(),
            "raw": request.raw,
            "routes": [],
        }));
    }
    let store = StateStore::resolve(request.settings.state_dir.clone())?;
    let routes = store.read_routes(!request.raw)?;
    let proxy_exe = store.read_proxy_exe_status()?;
    let status = proxy_runtime_status(&store)?;
    let service_status =
        ProbedServiceStatus::from_result(service_probe(&request.settings, store.root()));
    // HTTP is the identity and health-check port. HTTPS is informational state
    // for clients that want to display the TLS listener.
    let https_port = store.read_https_port()?;
    let proxy_exe_note = if proxy_exe.path.is_some() || proxy_exe.warning.is_some() {
        Some(proxy_exe_note())
    } else {
        None
    };
    Ok(json!({
        "ok": true,
        "state_dir": store.root(),
        "http_port": status.http_port,
        "https_port": https_port,
        "pid": status.pid,
        "pid_alive": status.pid_is_alive(),
        "pid_observation": status.pid_observation.map(PidObservation::label),
        "health_pid": status.health_pid,
        "handshake_ok": status.handshake_ok,
        "pid_matches_proxy": status.pid_matches_proxy,
        "running": status.handshake_ok && status.pid_matches_proxy,
        "proxy_exe": proxy_exe.path,
        "proxy_exe_note": proxy_exe_note,
        "proxy_exe_warning": proxy_exe.warning,
        "service_keepalive_active": service_status.keepalive_active(),
        "service_status_uncertain": service_status.is_uncertain(),
        "service": service_status.value(),
        "raw": request.raw,
        "routes": routes,
    }))
}

#[derive(Debug)]
struct ProxyRuntimeStatus {
    pid: Option<u32>,
    pid_observation: Option<PidObservation>,
    http_port: Option<u16>,
    health_pid: Option<u32>,
    handshake_ok: bool,
    pid_matches_proxy: bool,
}

impl ProxyRuntimeStatus {
    fn pid_is_alive(&self) -> bool {
        self.pid_observation.is_some_and(PidObservation::is_alive)
    }

    fn pid_may_be_alive(&self) -> bool {
        self.pid_observation
            .is_some_and(PidObservation::may_be_alive)
    }
}

fn proxy_runtime_status(store: &StateStore) -> Result<ProxyRuntimeStatus> {
    let pid = store.read_pid()?;
    let pid_observation = pid.map(observe_pid);
    let http_port = store.read_http_port()?;
    let health_token = store.read_health_token()?;
    let health_pid = match (http_port, health_token.as_deref()) {
        (Some(port), Some(token)) => jig_proxy_http_pid("127.0.0.1", port, Some(token)),
        _ => None,
    };
    let handshake_ok = health_pid.is_some();
    let pid_matches_proxy = pid
        .zip(health_pid)
        .is_some_and(|(pid, health_pid)| pid == health_pid);
    Ok(ProxyRuntimeStatus {
        pid,
        pid_observation,
        http_port,
        health_pid,
        handshake_ok,
        pid_matches_proxy,
    })
}

#[derive(Clone, Debug)]
struct ProbedServiceStatus {
    snapshot: Option<service::ServiceStatusSnapshot>,
}

impl ProbedServiceStatus {
    fn from_result(result: Result<Option<service::ServiceStatusSnapshot>>) -> Self {
        let snapshot = match result {
            Ok(snapshot) => snapshot,
            Err(error) => Some(service::ServiceStatusSnapshot::uncertain(error)),
        };
        Self { snapshot }
    }

    fn value(&self) -> Option<&Value> {
        self.snapshot
            .as_ref()
            .map(service::ServiceStatusSnapshot::json_value)
    }

    fn keepalive_active(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(service::ServiceStatusSnapshot::keepalive_active)
    }

    fn is_uncertain(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(service::ServiceStatusSnapshot::is_uncertain)
    }

    fn degrades_stop_ok(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(service::ServiceStatusSnapshot::degrades_stop_ok)
    }
}

fn proxy_service_status_snapshot(
    settings: &ProxySettings,
    state_dir: &std::path::Path,
) -> Result<Option<service::ServiceStatusSnapshot>> {
    service::status_if_installed_for_state_dir(settings, state_dir)
}

const fn proxy_exe_note() -> &'static str {
    "If this path came from JIG_DEV_BIN, restart the long-running proxy after rebuilding or replacing that binary."
}

/// Removes routes whose owning processes are no longer live.
///
/// # Errors
///
/// Returns an error when state resolution, route locking, route validation, or
/// the atomic state rewrite fails.
pub fn proxy_prune(request: ProxyPruneRequest) -> Result<Value> {
    if missing_state_dir(&request.settings)?.is_some() {
        return Ok(json!({ "ok": true, "routes": [] }));
    }
    let store = StateStore::resolve(request.settings.state_dir)?;
    let routes = store.prune()?;
    Ok(json!({ "ok": true, "routes": routes }))
}

/// Runs one proxied application under foreground process supervision.
///
/// # Errors
///
/// Returns an error when executable discovery, app startup, listener ownership
/// verification, route publication, proxy startup, or cleanup fails.
pub fn proxy_run_foreground(request: ProxyRunRequest) -> Result<Value> {
    let current_exe = current_exe()?;
    let app = request.spec.name.clone();
    let hostname = request.spec.hostname.clone();
    normalize_proxy_run_result(
        processes::run_app(request.spec, &request.settings, &current_exe),
        &app,
        &hostname,
    )
}

/// Registers a static proxy alias after validating its target and exposure.
///
/// # Errors
///
/// Returns an error when the target or hostname is invalid, required exposure
/// acknowledgement is absent, state mutation fails, or certificates cannot be
/// refreshed safely.
pub fn proxy_alias(request: ProxyAliasRequest) -> Result<Value> {
    if request.target_port == 0 {
        bail!("Proxy alias target port must be greater than 0");
    }
    let target_ip = validate_alias_target_host(&request.target_host)?;
    if request.settings.lan && !host::ip_is_loopback(target_ip) {
        bail!(
            "LAN proxy aliases may only target loopback IP literals. Refusing to expose '{}' through the LAN listener.",
            request.target_host
        );
    }
    if !host::ip_is_loopback(target_ip) && !request.accept_non_loopback_target {
        bail!(
            "Proxy alias target host '{}' is not loopback. Pass --accept-non-loopback-target to acknowledge that local browser requests can be proxied to that address.",
            request.target_host
        );
    }
    let store = StateStore::resolve(request.settings.state_dir.clone())?;
    let hostname = host::route_hostname(&request.name, &request.repo_name, &request.settings.tld)?;
    let https_active = proxy_https_listener_is_current_jig_proxy(&store)?;
    if https_active {
        // This is a freshness hint, not an atomic claim about the listener.
        // Route publication is independent, and a restarted proxy will pick up
        // cert state through the normal cache invalidation path.
        certs::ensure_for_hosts(&request.settings, std::slice::from_ref(&hostname))?;
    }
    store.add_alias_route(Route {
        hostname: RouteHostname::new(&hostname)?,
        target_host: TargetHost::ip_literal(&request.target_host)?,
        target_port: request.target_port,
        owner_pid: None,
        owner_start_token: None,
        mode: RouteMode::Alias,
        created_at_ms: now_ms(),
    })?;
    Ok(json!({
        "ok": true,
        "hostname": hostname,
        "target_port": request.target_port,
        "state_dir": store.root(),
    }))
}

fn proxy_https_listener_is_current_jig_proxy(store: &StateStore) -> Result<bool> {
    let Some(https_port) = store.read_https_port()? else {
        return Ok(false);
    };
    if !is_tcp_listening("127.0.0.1", https_port) {
        return Ok(false);
    }
    let Some(http_port) = store.read_http_port()? else {
        return Ok(false);
    };
    let Some(health_token) = store.read_health_token()? else {
        return Ok(false);
    };
    let Some(health_pid) = jig_proxy_http_pid("127.0.0.1", http_port, Some(&health_token)) else {
        return Ok(false);
    };
    Ok(store.read_pid()? == Some(health_pid))
}

/// Executes an explicit local certificate operation.
///
/// # Errors
///
/// Returns an error when certificate state is invalid, key or certificate I/O
/// fails, trust acknowledgement is absent, or the platform trust command fails.
pub fn proxy_cert(request: ProxyCertRequest) -> Result<Value> {
    match request {
        ProxyCertRequest::Generate { settings, force } => certs::generate(&settings, force),
        ProxyCertRequest::Status { settings } => certs::status(&settings),
        ProxyCertRequest::Trust {
            settings,
            accept_trust_scope,
        } => certs::trust(&settings, accept_trust_scope),
        ProxyCertRequest::Untrust {
            settings,
            accept_trust_scope,
        } => certs::untrust(&settings, accept_trust_scope),
    }
}

/// Executes an explicit user-service operation for the local proxy.
///
/// # Errors
///
/// Returns an error when service scope acknowledgement is absent, service files
/// are invalid, or platform service installation, removal, or inspection fails.
pub fn proxy_service(request: ProxyServiceRequest) -> Result<Value> {
    match request {
        ProxyServiceRequest::Install {
            settings,
            current_exe,
            repo_root,
            accept_service_scope,
        } => service::install(&settings, current_exe, repo_root, accept_service_scope),
        ProxyServiceRequest::Uninstall { settings } => service::uninstall(&settings),
        ProxyServiceRequest::Status { settings } => service::status(&settings),
    }
}

/// Resolves the canonical path of the running Jig executable.
///
/// # Errors
///
/// Returns an error when the current executable path cannot be read or
/// canonicalized.
pub fn current_exe() -> Result<std::path::PathBuf> {
    let path = std::env::current_exe()?;
    path.canonicalize().with_context(|| {
        format!(
            "Failed to canonicalize current executable {}",
            path.display()
        )
    })
}

/// Resolves the proxy state directory from an override, environment, or home.
///
/// # Errors
///
/// Returns an error when no explicit or environment override is present and a
/// home directory cannot be resolved.
pub fn resolve_state_dir(explicit: Option<std::path::PathBuf>) -> Result<std::path::PathBuf> {
    if let Some(path) = explicit {
        Ok(path)
    } else if let Ok(path) = std::env::var("JIG_PROXY_STATE_DIR") {
        Ok(std::path::PathBuf::from(path))
    } else {
        Ok(dirs::home_dir()
            .context("Could not resolve home directory for Jig proxy state")?
            .join(".jig/proxy"))
    }
}

fn missing_state_dir(settings: &ProxySettings) -> Result<Option<std::path::PathBuf>> {
    let path = resolve_state_dir(settings.state_dir.clone())?;
    let exists = path
        .try_exists()
        .with_context(|| format!("Failed to inspect Jig proxy state dir {}", path.display()))?;
    Ok((!exists).then_some(path))
}

// Keep these helpers public for the sibling `jig` crate's config translation,
// but hide them from the documented facade. The supported external surface is
// the request/command API.
#[doc(hidden)]
pub fn app_hostname(name: &str, repo_name: &str, tld: &str) -> Result<String> {
    host::route_hostname(name, repo_name, tld)
}

#[doc(hidden)]
pub fn dns_label(value: &str) -> Result<String> {
    host::sanitize_label(value)
}

#[doc(hidden)]
pub fn validate_tld(tld: &str) -> Result<()> {
    host::validate_tld(tld)
}

#[doc(hidden)]
pub fn ip_is_loopback(ip: IpAddr) -> bool {
    host::ip_is_loopback(ip)
}

fn ensure_unique_specs(specs: &[AppRunSpec]) -> Result<()> {
    let mut names = HashMap::new();
    let mut hostnames = HashMap::new();
    for spec in specs {
        if let Some(previous_dir) = names.insert(spec.name.as_str(), spec.dir.as_path()) {
            bail!(
                "Duplicate development app name '{}' in {} and {}",
                spec.name,
                previous_dir.display(),
                spec.dir.display()
            );
        }
        if let Some(previous_dir) = hostnames.insert(spec.hostname.as_str(), spec.dir.as_path()) {
            bail!(
                "Duplicate development app hostname '{}' in {} and {}",
                spec.hostname,
                previous_dir.display(),
                spec.dir.display()
            );
        }
    }
    Ok(())
}

fn validate_alias_target_host(host: &str) -> Result<IpAddr> {
    parse_ip_literal(host)
        .with_context(|| format!("Proxy alias target host '{host}' must be an IP literal"))
}

#[doc(hidden)]
pub fn parse_ip_literal(host: &str) -> Result<IpAddr> {
    host.parse::<IpAddr>()
        .with_context(|| format!("target host '{host}' must be an IP literal"))
}

#[cfg(test)]
// Crate-root tests need an explicit path; otherwise `mod tests` resolves to
// `src/tests.rs`, not the `src/lib/tests.rs` file used for this split.
#[path = "lib/tests.rs"]
mod tests;
