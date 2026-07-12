use std::collections::{HashMap, HashSet};
use std::error::Error as StdError;
use std::fmt;
#[cfg(test)]
use std::fs;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::certs;
use crate::host::{RouteHostname, TargetHost, target_host_is_loopback};
use crate::ports::{find_free_app_port_excluding, local_lan_ip_for_ipv4_listener, port_is_free};
use crate::state::{StateStore, now_ms, process_start_tokens_supported};
#[cfg(test)]
use crate::types::CommandSpec;
use crate::types::{AppKind, AppRunSpec, ProxySettings, Route, RouteMode};
mod child_lifecycle;
mod cleanup;
mod frameworks;
mod listener_owner;
mod output;
mod proxy;

use self::child_lifecycle::*;
use self::cleanup::*;
use self::frameworks::*;
use self::listener_owner::*;
use self::output::*;
pub(crate) use self::proxy::ensure_proxy_running;
#[cfg(test)]
use self::proxy::{MAX_PROXY_LOG_BYTES, ensure_requested_https, open_proxy_log};
use self::proxy::{proxy_health_failed, proxy_ready};

const PROXY_HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(1);
static CTRL_C_REQUESTED: AtomicBool = AtomicBool::new(false);
static CTRL_C_HANDLER: OnceLock<()> = OnceLock::new();

struct PreparedApp {
    spec: AppRunSpec,
    route_parts: Option<(RouteHostname, TargetHost)>,
    port: u16,
    argv: Vec<String>,
}

struct SpawnedChild {
    child: Child,
    output: CapturedAppOutput,
}

#[derive(Debug)]
struct Interrupted;

impl fmt::Display for Interrupted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Interrupted")
    }
}

impl StdError for Interrupted {}

fn interruption_error() -> anyhow::Error {
    Interrupted.into()
}

fn is_interruption(error: &anyhow::Error) -> bool {
    error.downcast_ref::<Interrupted>().is_some()
}

pub(crate) fn run_app(
    spec: AppRunSpec,
    settings: &ProxySettings,
    current_exe: &Path,
) -> Result<Value> {
    start_ctrlc_cleanup_session();
    let store = StateStore::resolve(settings.state_dir.clone())?;
    let route_parts = if spec.proxy {
        ensure_process_routes_supported()?;
        let route_parts = process_route_parts(settings, &spec)?;
        preflight_process_routes(&store, std::slice::from_ref(&spec))?;
        prepare_certs_for_hosts(settings, std::slice::from_ref(&spec.hostname))?;
        ensure_proxy_running(settings, current_exe)?;
        Some(route_parts)
    } else {
        None
    };
    ensure_not_interrupted()?;

    let port = choose_app_port(spec.explicit_port, &spec.target_host, &mut HashSet::new())?;
    let argv = command_argv(&spec.command, &spec.kind, port)?;
    if argv.is_empty() {
        bail!("No command configured for app '{}'", spec.name);
    }
    let dev_env = dev_app_environment([(&spec, port)], settings, &store)?;

    let SpawnedChild {
        mut child,
        mut output,
    } = spawn_child(&spec, &argv, port, settings, &dev_env)?;
    let pid = child.id();
    let owner_start_token = if spec.proxy {
        match wait_for_app_ready(&spec, port, &mut child) {
            Ok(token) => token,
            Err(error) => {
                terminate_and_reap_logged(
                    &mut child,
                    "could not clean up app after readiness failure",
                );
                if is_interruption(&error) {
                    output.discard();
                } else {
                    output.print_failure(&spec.name);
                }
                return Err(error);
            }
        }
    } else {
        None
    };
    output.finish_progress();
    if spec.proxy {
        let Some(owner_start_token) = owner_start_token else {
            terminate_and_reap_logged(
                &mut child,
                "could not clean up app after missing owner identity",
            );
            bail!(
                "Could not verify start identity for child process {pid}; refusing to publish process route"
            );
        };
        let Some((hostname, target_host)) = route_parts else {
            terminate_and_reap_logged(
                &mut child,
                "could not clean up app after route preparation failure",
            );
            bail!(
                "Could not prepare process route for child process {pid}; refusing to publish route"
            );
        };
        let route = Route {
            hostname,
            target_host,
            target_port: port,
            owner_pid: Some(pid),
            owner_start_token: Some(owner_start_token.clone()),
            mode: RouteMode::Process,
            created_at_ms: now_ms(),
        };
        if let Err(error) = store.add_verified_route(route, || {
            verify_process_route_owner(
                &spec.name,
                &spec.target_host,
                port,
                pid,
                Some(&owner_start_token),
            )
        }) {
            terminate_and_reap_logged(
                &mut child,
                "could not clean up app after route verification failure",
            );
            return Err(error);
        }
    }

    let display = match app_display(&spec, settings, port, pid, &store) {
        Ok(display) => display,
        Err(error) => {
            terminate_and_reap_logged(&mut child, "could not clean up app after display failure");
            if spec.proxy {
                remove_route_best_effort(&store, &spec.hostname, &spec.name);
            }
            return Err(error);
        }
    };
    print_dev_table(std::slice::from_ref(&display));

    let status = loop {
        if ctrl_c_requested() {
            terminate_and_reap_logged(&mut child, "could not clean up interrupted app");
            output.discard();
            if spec.proxy {
                remove_route_best_effort(&store, &spec.hostname, &spec.name);
            }
            return Err(interruption_error());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(error) => {
                if spec.proxy {
                    remove_route_best_effort(&store, &spec.hostname, &spec.name);
                }
                terminate_and_reap_logged(&mut child, "could not clean up app after wait failure");
                return Err(error.into());
            }
        }
    };
    if spec.proxy {
        store.remove_route(&spec.hostname)?;
    }

    if !status.success() {
        // The direct wrapper may exit before descendants that inherited its
        // output pipes. Shut down the whole group before finalizing the tail.
        terminate_and_reap_logged(&mut child, "could not finalize failed app process group");
        output.print_failure(&spec.name);
    }

    let exit_status = status.code().unwrap_or(1);
    Ok(json!({
        "ok": status.success(),
        "app": spec.name,
        "hostname": spec.hostname,
        "port": port,
        "exit_status": exit_status,
    }))
}

pub(crate) fn run_apps(
    specs: Vec<AppRunSpec>,
    settings: &ProxySettings,
    current_exe: &Path,
) -> Result<Value> {
    start_ctrlc_cleanup_session();
    if specs.is_empty() {
        bail!("No development apps were configured or discovered.");
    }
    validate_explicit_ports(&specs)?;
    let uses_proxy = specs.iter().any(|spec| spec.proxy);
    let store = StateStore::resolve(settings.state_dir.clone())?;
    if uses_proxy {
        ensure_process_routes_supported()?;
        validate_process_routes(settings, &specs)?;
        preflight_process_routes(&store, &specs)?;
        let hostnames: Vec<String> = specs
            .iter()
            .filter(|spec| spec.proxy)
            .map(|spec| spec.hostname.clone())
            .collect();
        prepare_certs_for_hosts(settings, &hostnames)?;
        ensure_proxy_running(settings, current_exe)?;
    }
    ensure_not_interrupted()?;
    let mut children = Vec::new();
    let mut routes = Vec::new();
    let mut assigned_ports = HashSet::new();
    let mut display_rows = Vec::new();
    let mut prepared_apps = Vec::new();

    for spec in specs {
        let route_parts = if spec.proxy {
            Some(process_route_parts(settings, &spec)?)
        } else {
            None
        };
        let port = match choose_app_port(spec.explicit_port, &spec.target_host, &mut assigned_ports)
        {
            Ok(port) => port,
            Err(error) => {
                return Err(error);
            }
        };
        let argv = match command_argv(&spec.command, &spec.kind, port) {
            Ok(argv) if !argv.is_empty() => argv,
            Ok(_) => {
                bail!("No command configured for app '{}'", spec.name);
            }
            Err(error) => {
                return Err(error);
            }
        };
        prepared_apps.push(PreparedApp {
            spec,
            route_parts,
            port,
            argv,
        });
    }
    let dev_env = dev_app_environment(
        prepared_apps
            .iter()
            .map(|prepared| (&prepared.spec, prepared.port)),
        settings,
        &store,
    )?;

    for prepared in prepared_apps {
        ensure_not_interrupted()?;
        let PreparedApp {
            spec,
            route_parts,
            port,
            argv,
        } = prepared;
        let SpawnedChild {
            mut child,
            mut output,
        } = match spawn_child(&spec, &argv, port, settings, &dev_env) {
            Ok(spawned) => spawned,
            Err(error) => {
                cleanup_children(&mut children);
                return Err(error);
            }
        };
        let child_pid = child.id();
        let owner_start_token = if spec.proxy {
            match wait_for_app_ready(&spec, port, &mut child) {
                Ok(token) => token,
                Err(error) => {
                    terminate_and_reap_logged(
                        &mut child,
                        "could not clean up app after readiness failure",
                    );
                    if is_interruption(&error) {
                        output.discard();
                    } else {
                        output.print_failure(&spec.name);
                    }
                    cleanup_children(&mut children);
                    return Err(error);
                }
            }
        } else {
            None
        };
        output.finish_progress();
        if spec.proxy && owner_start_token.is_none() {
            terminate_and_reap_logged(
                &mut child,
                "could not clean up app after missing owner identity",
            );
            cleanup_children(&mut children);
            bail!(
                "Could not verify start identity for child process {child_pid}; refusing to publish process route"
            );
        }
        if spec.proxy {
            let Some((hostname, target_host)) = route_parts else {
                terminate_and_reap_logged(
                    &mut child,
                    "could not clean up app after route preparation failure",
                );
                cleanup_children(&mut children);
                bail!(
                    "Could not prepare process route for child process {child_pid}; refusing to publish route"
                );
            };
            let route = Route {
                hostname,
                target_host,
                target_port: port,
                owner_pid: Some(child_pid),
                owner_start_token,
                mode: RouteMode::Process,
                created_at_ms: now_ms(),
            };
            if let Err(error) = store.add_verified_route(route.clone(), || {
                verify_process_route_owner(
                    &spec.name,
                    &spec.target_host,
                    port,
                    child_pid,
                    route.owner_start_token.as_deref(),
                )
            }) {
                terminate_and_reap_logged(
                    &mut child,
                    "could not clean up app after route verification failure",
                );
                cleanup_children(&mut children);
                return Err(error);
            }
            routes.push(route);
        }
        let display = match app_display(&spec, settings, port, child_pid, &store) {
            Ok(display) => display,
            Err(error) => {
                terminate_and_reap_logged(
                    &mut child,
                    "could not clean up app after display failure",
                );
                if spec.proxy {
                    remove_route_best_effort(&store, &spec.hostname, &spec.name);
                }
                cleanup_children(&mut children);
                return Err(error);
            }
        };
        display_rows.push(display);
        children.push(RunningChild {
            name: spec.name,
            hostname: spec.hostname,
            proxied: spec.proxy,
            store: store.clone(),
            child,
            output,
            cleanup_armed: true,
        });
    }

    print_dev_table(&display_rows);

    let mut first_exit = None;
    let mut proxy_stopped = false;
    let mut interrupted = false;
    let mut failed_child = None;
    let mut proxy_health_misses = 0u8;
    let mut next_proxy_health_check = Instant::now() + PROXY_HEALTH_CHECK_INTERVAL;
    while first_exit.is_none() {
        if ctrl_c_requested() {
            first_exit = Some(("interrupt".to_string(), 130));
            interrupted = true;
            break;
        }
        for (index, running) in children.iter_mut().enumerate() {
            match running.child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        failed_child = Some(index);
                    }
                    first_exit = Some((running.name.clone(), status.code().unwrap_or(1)));
                    break;
                }
                Ok(None) => {}
                Err(error) => {
                    cleanup_children(&mut children);
                    return Err(error.into());
                }
            }
        }
        if first_exit.is_none() && uses_proxy && Instant::now() >= next_proxy_health_check {
            next_proxy_health_check = Instant::now() + PROXY_HEALTH_CHECK_INTERVAL;
            if proxy_health_failed(&mut proxy_health_misses, proxy_ready(&store, settings)?) {
                first_exit = Some(("jig proxy".to_string(), 1));
                proxy_stopped = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    cleanup_children(&mut children);
    if interrupted {
        for running in &mut children {
            running.output.discard();
        }
    } else if let Some(index) = failed_child {
        let failed = &mut children[index];
        failed.output.print_failure(&failed.name);
    }

    if proxy_stopped {
        eprintln!("Jig proxy stopped responding; shutting down development session");
    } else if interrupted {
        eprintln!("Interrupted; stopping development session");
    } else if let Some((name, code)) = &first_exit {
        eprintln!("{name} exited with status {code}; stopping development session");
    }

    if interrupted {
        return Err(interruption_error());
    }

    Ok(json!({
        "ok": first_exit.as_ref().map(|(_, code)| *code == 0).unwrap_or(false),
        "first_exit": first_exit.map(|(name, code)| json!({ "app": name, "exit_status": code })),
        "proxy_failed": proxy_stopped,
        "routes": routes,
    }))
}

fn prepare_certs_for_hosts(settings: &ProxySettings, hostnames: &[String]) -> Result<()> {
    if !settings.https {
        return Ok(());
    }
    certs::ensure_for_hosts(settings, hostnames).with_context(|| {
        "Failed to prepare HTTPS proxy certificates. Likely fix: run `scripts/jig proxy cert generate --force`, trust the CA with `scripts/jig proxy cert trust --accept-trust-scope`, or disable [dev].https for HTTP-only local development."
    })?;
    Ok(())
}

fn validate_explicit_ports(specs: &[AppRunSpec]) -> Result<()> {
    let mut explicit_ports = HashSet::new();
    for spec in specs {
        let Some(port) = spec.explicit_port else {
            continue;
        };
        if port == 0 {
            bail!(
                "Explicit development app ports must be greater than 0. Likely fix: remove the [[dev.apps]].port override or set it to an available nonzero port."
            );
        }
        if !explicit_ports.insert(port) {
            bail!(
                "Multiple development apps requested port {port}. Likely fix: assign each [[dev.apps]] entry a unique port or remove explicit port overrides."
            );
        }
    }
    Ok(())
}

fn ensure_process_routes_supported() -> Result<()> {
    if process_start_tokens_supported() {
        return Ok(());
    }
    bail!(
        "Process routes require process start-token verification on this platform. Use `scripts/jig proxy alias` for an already-running app, or run with --no-proxy."
    )
}

fn validate_process_routes(settings: &ProxySettings, specs: &[AppRunSpec]) -> Result<()> {
    for spec in specs {
        if spec.proxy {
            process_route_parts(settings, spec)?;
        }
    }
    Ok(())
}

fn preflight_process_routes(store: &StateStore, specs: &[AppRunSpec]) -> Result<()> {
    ensure_unique_process_route_hostnames(specs)?;
    store.ensure_no_live_process_routes_for_hostnames(
        specs
            .iter()
            .filter(|spec| spec.proxy)
            .map(|spec| spec.hostname.as_str()),
    )
}

fn ensure_not_interrupted() -> Result<()> {
    ensure_not_interrupted_with(ctrl_c_requested)
}

fn ensure_not_interrupted_with(interrupted: impl FnOnce() -> bool) -> Result<()> {
    if interrupted() {
        return Err(interruption_error());
    }
    Ok(())
}

fn ensure_unique_process_route_hostnames(specs: &[AppRunSpec]) -> Result<()> {
    let mut seen = HashMap::new();
    for spec in specs.iter().filter(|spec| spec.proxy) {
        let hostname = spec.hostname.to_ascii_lowercase();
        if let Some(previous_name) = seen.insert(hostname, spec.name.as_str()) {
            bail!(
                "Multiple proxied development apps requested hostname '{}': '{}' and '{}'. Likely fix: give each proxied [[dev.apps]] entry a unique hostname or disable proxy routing for one app.",
                spec.hostname,
                previous_name,
                spec.name
            );
        }
    }
    Ok(())
}

fn process_route_parts(
    settings: &ProxySettings,
    spec: &AppRunSpec,
) -> Result<(RouteHostname, TargetHost)> {
    let hostname = RouteHostname::new(&spec.hostname)?;
    let target_host = TargetHost::ip_literal(&spec.target_host).with_context(|| {
        format!(
            "Process route '{}' target host '{}' must be an IP literal",
            spec.name, spec.target_host
        )
    })?;
    if settings.lan && !target_host_is_loopback(&spec.target_host) {
        bail!(
            "LAN process route '{}' may only target loopback IP literals. Refusing to expose '{}' through the LAN listener. Likely fix: bind the app to 127.0.0.1 and let Jig proxy LAN traffic, or disable [dev].lan.",
            spec.name,
            spec.target_host
        );
    }
    Ok((hostname, target_host))
}

fn spawn_child(
    spec: &AppRunSpec,
    argv: &[String],
    port: u16,
    settings: &ProxySettings,
    dev_env: &[(String, String)],
) -> Result<SpawnedChild> {
    // App commands are trusted repo-configured dev processes and intentionally
    // inherit the caller's environment; only the background proxy clears env.
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(&spec.dir)
        .envs(dev_env.iter().map(|(key, value)| (key, value)))
        .env("PORT", port.to_string())
        .env("HOST", &spec.target_host);
    configure_app_child_process_group(&mut command);
    if spec.kind == AppKind::Vite || command_looks_like_vite(argv) {
        // Vite validates the browser-facing Host header even though Jig binds
        // the app to loopback. Vite's internal allowed-hosts escape hatch keeps
        // routed dev hostnames working while still injecting --host 127.0.0.1;
        // keep this isolated because Vite can rename the variable.
        let allowed_hosts = vite_allowed_hosts(spec, settings).with_context(|| {
            format!(
                "Failed to configure Vite allowed hosts for app '{}'. Likely fix: keep [dev].tld and the app route hostname as valid DNS names, or set [[dev.apps]].kind to env-port for non-Vite commands.",
                spec.name
            )
        })?;
        command.env("__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS", allowed_hosts);
    }
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            anyhow::Error::new(error).context(format!(
                "Failed to run command '{}' for dev app '{}' in {}: executable was not found in PATH. Likely fix: run the repo bootstrap command or install the package manager/tool used by [[dev.apps]].argv.",
                argv[0],
                spec.name,
                spec.dir.display()
            ))
        } else {
            anyhow::Error::new(error).context(format!(
                "Failed to run command '{}' for dev app '{}' in {}. Likely fix: run the repo bootstrap command and verify [[dev.apps]].dir and argv.",
                argv[0],
                spec.name,
                spec.dir.display()
            ))
        }
    })?;
    let output = CapturedAppOutput::from_child(&mut child, &spec.name)?;
    Ok(SpawnedChild { child, output })
}

fn remove_route_best_effort(store: &StateStore, hostname: &str, app_name: &str) {
    if let Err(error) = store.remove_route(hostname) {
        eprintln!(
            "jig proxy could not remove route '{hostname}' while cleaning up app '{app_name}': {error}"
        );
    }
}

#[cfg(unix)]
fn configure_app_child_process_group(command: &mut Command) {
    unsafe {
        // SAFETY: pre_exec runs in the child after fork and before exec. The
        // closure only calls setsid and reads errno for its return value.
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

#[cfg(windows)]
fn configure_app_child_process_group(command: &mut Command) {
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn configure_app_child_process_group(_command: &mut Command) {}

fn choose_app_port(
    explicit: Option<u16>,
    target_host: &str,
    assigned_ports: &mut HashSet<u16>,
) -> Result<u16> {
    let port = if let Some(port) = explicit {
        if port == 0 {
            bail!(
                "Explicit development app ports must be greater than 0. Likely fix: remove the [[dev.apps]].port override or set it to an available nonzero port."
            );
        }
        if assigned_ports.contains(&port) {
            bail!(
                "Multiple development apps requested port {port}. Likely fix: assign each [[dev.apps]] entry a unique port or remove explicit port overrides."
            );
        }
        if !port_is_free(target_host, port)? {
            bail!(
                "Requested development app port {port} is already in use on {target_host}. Likely fix: stop the process using that port or configure a different [[dev.apps]].port."
            );
        }
        port
    } else {
        find_free_app_port_excluding(target_host, assigned_ports)?
    };
    if !assigned_ports.insert(port) {
        bail!(
            "Multiple development apps requested port {port}. Likely fix: retry the dev command or assign explicit unique [[dev.apps]].port values."
        );
    }
    Ok(port)
}

#[derive(Clone, Debug)]
struct AppDisplay {
    name: String,
    url: String,
    pid: u32,
    lan_note: Option<String>,
}

fn app_display(
    spec: &AppRunSpec,
    settings: &ProxySettings,
    port: u16,
    pid: u32,
    store: &StateStore,
) -> Result<AppDisplay> {
    if spec.proxy {
        let (scheme, proxy_port) = proxy_origin(settings, store)?;
        let lan_note = if settings.lan {
            if let Some(ip) = local_lan_ip_for_ipv4_listener() {
                Some(format!(
                    "{} LAN -> {scheme}://{}:{} with Host header {} or a local DNS/hosts entry",
                    spec.name, ip, proxy_port, spec.hostname
                ))
            } else {
                Some(format!(
                    "{} LAN -> no non-loopback IPv4 LAN address detected for the IPv4 listener; configure DNS/hosts once an address is available",
                    spec.name
                ))
            }
        } else {
            None
        };
        return Ok(AppDisplay {
            name: spec.name.clone(),
            url: format!("{scheme}://{}:{proxy_port}", spec.hostname),
            pid,
            lan_note,
        });
    }
    Ok(AppDisplay {
        name: spec.name.clone(),
        url: format!("http://{}:{port}", spec.target_host),
        pid,
        lan_note: None,
    })
}

fn dev_app_environment<'a>(
    apps: impl IntoIterator<Item = (&'a AppRunSpec, u16)>,
    settings: &ProxySettings,
    store: &StateStore,
) -> Result<Vec<(String, String)>> {
    let mut env = Vec::new();
    let mut prefixes = HashMap::new();
    for (spec, port) in apps {
        let prefix = jig_core::dev_app_env_prefix(&spec.name);
        if let Some(previous) = prefixes.insert(prefix.clone(), spec.name.as_str()) {
            bail!(
                "dev apps '{}' and '{}' share derived environment prefix {prefix}; rename one app so punctuation-normalized names are unique",
                previous,
                spec.name
            );
        }
        env.push((format!("{prefix}_HOST"), spec.target_host.clone()));
        env.push((format!("{prefix}_PORT"), port.to_string()));
        let origin = app_origin(spec, settings, port, store)?;
        env.push((format!("{prefix}_ORIGIN"), origin.clone()));
        env.push((format!("{prefix}_URL"), origin));
    }
    Ok(env)
}

fn app_origin(
    spec: &AppRunSpec,
    settings: &ProxySettings,
    port: u16,
    store: &StateStore,
) -> Result<String> {
    if !spec.proxy {
        return Ok(format!("http://{}:{port}", spec.target_host));
    }

    let (scheme, proxy_port) = proxy_origin(settings, store)?;
    Ok(format!("{scheme}://{}:{proxy_port}", spec.hostname))
}

fn proxy_origin(settings: &ProxySettings, store: &StateStore) -> Result<(&'static str, u16)> {
    if settings.https
        && let Some(port) = store.read_https_port()?
    {
        return Ok(("https", port));
    }
    Ok((
        "http",
        store.read_http_port()?.unwrap_or(settings.http_port),
    ))
}

fn print_dev_table(rows: &[AppDisplay]) {
    for line in format_dev_table(rows) {
        eprintln!("{line}");
    }
    for note in rows.iter().filter_map(|row| row.lan_note.as_deref()) {
        eprintln!("{note}");
    }
}

fn format_dev_table(rows: &[AppDisplay]) -> Vec<String> {
    if rows.is_empty() {
        return Vec::new();
    }
    let name_width = rows
        .iter()
        .map(|row| row.name.len())
        .max()
        .unwrap_or(3)
        .max(3);
    let url_width = rows
        .iter()
        .map(|row| row.url.len())
        .max()
        .unwrap_or(3)
        .max(3);
    const RUNNING_STATUS: &str = "running";
    let status_width = RUNNING_STATUS.len().max("STATUS".len());
    let pid_width = rows
        .iter()
        .map(|row| row.pid.to_string().len())
        .max()
        .unwrap_or(3)
        .max(3);
    let mut lines = vec![format!(
        "{:<name_width$}  {:<url_width$}  {:<status_width$}  {:>pid_width$}",
        "APP", "URL", "STATUS", "PID"
    )];
    for row in rows {
        lines.push(format!(
            "{:<name_width$}  {:<url_width$}  {:<status_width$}  {:>pid_width$}",
            row.name, row.url, RUNNING_STATUS, row.pid
        ));
    }
    lines
}

#[cfg(test)]
mod tests;
