use std::collections::{HashMap, HashSet};
use std::error::Error as StdError;
use std::ffi::{OsStr, OsString};
use std::fmt;
#[cfg(test)]
use std::fs;
#[cfg(unix)]
use std::os::unix::process::{CommandExt, ExitStatusExt};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::certs;
use crate::host::{RouteHostname, TargetHost, target_host_is_loopback};
use crate::ports::{find_free_app_port_excluding, local_lan_ip_for_ipv4_listener, port_is_free};
use crate::state::{
    DevProcessIdentity, LockOutcome, ProcessRouteOwnership, STATE_LOCK_TIMEOUT, StateStore, now_ms,
    process_start_token, process_start_tokens_supported,
};
#[cfg(test)]
use crate::types::CommandSpec;
use crate::types::{AppKind, AppRunSpec, ProxySettings, Route, RouteMode};

use self::child_lifecycle::*;
use self::cleanup::{
    RunningChild, arm_owned_resources, cleanup_children, new_route_cleanup_deadline,
    select_interruption, select_primary_outcome, start_termination_cleanup_session,
    termination_requested,
};
pub(crate) use self::cleanup::{TerminationReason, force_cleanup_requested};
#[cfg(test)]
pub(crate) use self::dev_session::finalize_claimed_dev_session_result;
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
use self::dev_session::finish_preflight_cleanup;
#[cfg(test)]
use self::dev_session::normalize_preflight_result;
#[cfg(all(test, not(windows)))]
use self::dev_session::run_apps_with_interrupt_probe;
pub(crate) use self::dev_session::run_apps_with_preflight;
use self::frameworks::*;
use self::listener_owner::*;
use self::output::*;
#[cfg(test)]
use self::proxy::proxy_ready;
#[cfg(test)]
use self::proxy::{MAX_PROXY_LOG_BYTES, ensure_requested_https, open_proxy_log};
use self::proxy::{
    ensure_proxy_running_interruptible, proxy_health_failed, proxy_ready_interruptible,
};
use self::route_publication::publish_process_route_interruptible;
#[cfg(all(test, not(windows)))]
use self::route_publication::publish_process_route_interruptible_with_verifier;

mod child_lifecycle;
mod cleanup;
mod dev_session;
mod frameworks;
mod listener_owner;
mod output;
mod proxy;
mod route_publication;
#[cfg(all(test, not(windows)))]
mod startup_failure_tests;
#[cfg(any(windows, test))]
mod windows_launch;

const PROXY_HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(1);
pub(crate) const UNCONFIRMED_DEV_CLEANUP_MESSAGE: &str = "Jig dev received a stop request, but process-tree or route cleanup could not be confirmed; the session was retained for inspection";
// Keep this exact prefix aligned with the npm branch rendered for
// `generated_frontend_dev_apps` in templates/project/.jig.toml.jinja. Matching
// the complete generated form keeps ordinary repository-authored npm commands
// on the normal inherited-environment boundary.
const GENERATED_NPM_DEV_ARGV_PREFIX: [&str; 13] = [
    "npm",
    "--prefix=.",
    "--workspace=.",
    "--workspaces=true",
    "--include-workspace-root=true",
    "--global=false",
    "--location=project",
    "--if-present=false",
    "--include=dev",
    "--include=optional",
    "--include=peer",
    "run",
    "dev",
];
const MANAGED_NPM_RUN_CONFIG_KEYS: [&str; 14] = [
    "workspace",
    "workspaces",
    "include-workspace-root",
    "prefix",
    "global",
    "location",
    "if-present",
    "omit",
    "include",
    "production",
    "optional",
    "only",
    "dev",
    "also",
];
#[cfg(test)]
static TERMINATION_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
fn recover_test_mutex_guard<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
pub(super) fn termination_test_guard() -> std::sync::MutexGuard<'static, ()> {
    recover_test_mutex_guard(&TERMINATION_TEST_LOCK)
}

struct PreparedApp {
    spec: AppRunSpec,
    route_parts: Option<(RouteHostname, TargetHost)>,
    port: u16,
    argv: Vec<String>,
}

struct SpawnedChild {
    child: Child,
    output: CapturedAppOutput,
    process_lease: AppProcessLease,
}

struct SpawnChildFailure {
    error: anyhow::Error,
    cleanup_confirmed: bool,
    spawned_process: Option<DevProcessIdentity>,
}

impl From<anyhow::Error> for SpawnChildFailure {
    fn from(error: anyhow::Error) -> Self {
        Self {
            error,
            cleanup_confirmed: true,
            spawned_process: None,
        }
    }
}

pub(crate) fn ensure_proxy_running(settings: &ProxySettings, current_exe: &Path) -> Result<()> {
    let _termination_session = start_termination_cleanup_session()?;
    let cancelled = || termination_requested().is_some();
    let result = (|| {
        let store = lock_outcome_or_interruption(
            StateStore::resolve_interruptible(settings.state_dir.clone(), &cancelled)?,
            &termination_requested,
        )?;
        lock_outcome_or_interruption(
            ensure_proxy_running_interruptible(&store, settings, current_exe, &cancelled)?,
            &termination_requested,
        )?;
        ensure_not_interrupted_with(termination_requested)
    })();

    // Once an authenticated shared proxy is ready, or an ordinary startup
    // error has won, a later signal must not replace that primary outcome.
    // Interrupted lock/startup paths select interruption themselves so their
    // original signal remains available to the structured CLI result.
    if !matches!(&result, Err(error) if is_interruption(error)) {
        select_primary_outcome();
    }
    result
}

#[derive(Debug)]
struct Interrupted {
    reason: TerminationReason,
    cleanup: InterruptionCleanup,
}

#[derive(Debug)]
struct DevErrorWithRecoveries {
    source: anyhow::Error,
    recoveries: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InterruptionCleanup {
    Confirmed,
    Unconfirmed,
}

impl fmt::Display for Interrupted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Interrupted by {}", self.reason.label())
    }
}

impl StdError for Interrupted {}

impl fmt::Display for DevErrorWithRecoveries {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if formatter.alternate() {
            write!(formatter, "{:#}", self.source)
        } else {
            write!(formatter, "{}", self.source)
        }
    }
}

impl StdError for DevErrorWithRecoveries {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.source.as_ref())
    }
}

pub(crate) fn interruption_error(reason: TerminationReason) -> anyhow::Error {
    interruption_error_with_state(reason, InterruptionCleanup::Confirmed)
}

pub(crate) fn interruption_error_with_unconfirmed_cleanup(
    reason: TerminationReason,
    recoveries: Option<Value>,
) -> anyhow::Error {
    let error = interruption_error_with_state(reason, InterruptionCleanup::Unconfirmed);
    match recoveries {
        Some(recoveries) => dev_error_with_recoveries(error, recoveries),
        None => error,
    }
}

fn interruption_error_with_state(
    reason: TerminationReason,
    cleanup: InterruptionCleanup,
) -> anyhow::Error {
    Interrupted { reason, cleanup }.into()
}

pub(crate) fn dev_error_with_recoveries(source: anyhow::Error, recoveries: Value) -> anyhow::Error {
    DevErrorWithRecoveries { source, recoveries }.into()
}

pub(crate) fn dev_error_parts(error: &anyhow::Error) -> (&anyhow::Error, Option<&Value>) {
    error
        .downcast_ref::<DevErrorWithRecoveries>()
        .map_or((error, None), |context| {
            (&context.source, Some(&context.recoveries))
        })
}

fn dev_error_source(error: &anyhow::Error) -> &anyhow::Error {
    dev_error_parts(error).0
}

pub(crate) fn is_interruption(error: &anyhow::Error) -> bool {
    dev_error_source(error)
        .downcast_ref::<Interrupted>()
        .is_some()
}

pub(crate) fn interruption_reason(error: &anyhow::Error) -> Option<TerminationReason> {
    dev_error_source(error)
        .downcast_ref::<Interrupted>()
        .map(|error| error.reason)
}

pub(crate) fn interruption_cleanup_unconfirmed(error: &anyhow::Error) -> bool {
    dev_error_source(error)
        .downcast_ref::<Interrupted>()
        .is_some_and(|error| error.cleanup == InterruptionCleanup::Unconfirmed)
}

pub(crate) fn run_app(
    spec: AppRunSpec,
    settings: &ProxySettings,
    current_exe: &Path,
) -> Result<Value> {
    let _termination_session = start_termination_cleanup_session()?;
    run_app_with_interrupt_probe(spec, settings, current_exe, termination_requested)
}

fn run_app_with_interrupt_probe(
    spec: AppRunSpec,
    settings: &ProxySettings,
    current_exe: &Path,
    interrupt_requested: impl Fn() -> Option<TerminationReason>,
) -> Result<Value> {
    let cancelled = || interrupt_requested().is_some();
    let store = lock_outcome_or_interruption(
        StateStore::resolve_interruptible(settings.state_dir.clone(), &cancelled)?,
        &interrupt_requested,
    )?;
    let route_parts = if spec.proxy {
        ensure_process_routes_supported()?;
        let route_parts = process_route_parts(settings, &spec)?;
        preflight_process_routes(&store, std::slice::from_ref(&spec), &interrupt_requested)?;
        prepare_certs_for_hosts_interruptible(
            settings,
            std::slice::from_ref(&spec.hostname),
            &interrupt_requested,
        )?;
        lock_outcome_or_interruption(
            ensure_proxy_running_interruptible(&store, settings, current_exe, &cancelled)?,
            &interrupt_requested,
        )?;
        Some(route_parts)
    } else {
        None
    };
    ensure_not_interrupted_with(&interrupt_requested)?;

    let port = choose_app_port(spec.explicit_port, &spec.target_host, &mut HashSet::new())?;
    let argv = command_argv(&spec.command, &spec.kind, port)?;
    if argv.is_empty() {
        bail!("No command configured for app '{}'", spec.name);
    }
    let dev_env = lock_outcome_or_interruption(
        dev_app_environment_interruptible([(&spec, port)], settings, &store, &cancelled)?,
        &interrupt_requested,
    )?;

    arm_owned_resources()?;
    let SpawnedChild {
        mut child,
        mut output,
        process_lease: _process_lease,
    } = match spawn_child(&spec, &argv, port, settings, &dev_env) {
        Ok(spawned) => spawned,
        Err(error) => {
            select_primary_outcome();
            return Err(error);
        }
    };
    let pid = child.id();
    let owner_start_token = if spec.proxy {
        match wait_for_app_ready(&spec, port, &mut child) {
            Ok(token) => token,
            Err(error) => {
                let disposition = if is_interruption(&error) {
                    select_interruption();
                    StartupOutputDisposition::Interrupted
                } else {
                    select_primary_outcome();
                    StartupOutputDisposition::Failure
                };
                terminate_and_reap_logged(
                    &mut child,
                    "could not clean up app after readiness failure",
                );
                output.finish_start_failure(disposition);
                return Err(error);
            }
        }
    } else {
        None
    };
    output.finish_progress();
    let mut route_ownership = None;
    if spec.proxy {
        let Some(owner_start_token) = owner_start_token else {
            select_primary_outcome();
            terminate_and_reap_logged(
                &mut child,
                "could not clean up app after missing owner identity",
            );
            output.finish_start_failure(StartupOutputDisposition::Failure);
            bail!(
                "Could not verify start identity for child process {pid}; refusing to publish process route"
            );
        };
        let Some((hostname, target_host)) = route_parts else {
            select_primary_outcome();
            terminate_and_reap_logged(
                &mut child,
                "could not clean up app after route preparation failure",
            );
            output.finish_start_failure(StartupOutputDisposition::Failure);
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
        let ownership = ProcessRouteOwnership::new(route.hostname.clone(), pid, owner_start_token);
        if let Err(error) = publish_process_route_interruptible(
            &store,
            route,
            &spec.name,
            &mut child,
            &interrupt_requested,
        ) {
            let interrupted = is_interruption(&error);
            if interrupted {
                select_interruption();
            } else {
                select_primary_outcome();
            }
            let process_cleaned = terminate_and_reap_logged(
                &mut child,
                "could not clean up app after route verification failure",
            );
            remove_route_best_effort(&store, &ownership, &spec.name, process_cleaned);
            output.finish_start_failure(if interrupted {
                StartupOutputDisposition::Interrupted
            } else {
                StartupOutputDisposition::Failure
            });
            return Err(error);
        }
        if let Err(error) = ensure_not_interrupted_with(&interrupt_requested) {
            select_interruption();
            let process_cleaned = terminate_and_reap_logged(
                &mut child,
                "could not clean up app interrupted after route publication",
            );
            output.finish_start_failure(StartupOutputDisposition::Interrupted);
            remove_route_best_effort(&store, &ownership, &spec.name, process_cleaned);
            return Err(error);
        }
        route_ownership = Some(ownership);
    }

    let display_result = app_display_interruptible(&spec, settings, port, pid, &store, &cancelled)
        .and_then(|outcome| lock_outcome_or_interruption(outcome, &interrupt_requested));
    let display = match display_result {
        Ok(display) => display,
        Err(error) => {
            let interrupted = is_interruption(&error);
            if interrupted {
                select_interruption();
            } else {
                select_primary_outcome();
            }
            let process_cleaned = terminate_and_reap_logged(
                &mut child,
                "could not clean up app after display failure",
            );
            if let Some(ownership) = route_ownership.as_ref() {
                remove_route_best_effort(&store, ownership, &spec.name, process_cleaned);
            }
            output.finish_start_failure(if interrupted {
                StartupOutputDisposition::Interrupted
            } else {
                StartupOutputDisposition::Failure
            });
            return Err(error);
        }
    };
    print_dev_table(std::slice::from_ref(&display));

    let status = loop {
        match try_wait_preserving_process_group(&mut child) {
            Ok(Some(status)) => {
                select_primary_outcome();
                break status;
            }
            Ok(None) => {}
            Err(error) => {
                select_primary_outcome();
                let process_cleaned = terminate_and_reap_logged(
                    &mut child,
                    "could not clean up app after wait failure",
                );
                if let Some(ownership) = route_ownership.as_ref() {
                    remove_route_best_effort(&store, ownership, &spec.name, process_cleaned);
                }
                return Err(error.into());
            }
        }
        if let Some(reason) = interrupt_requested() {
            // Prefer a child status that became observable immediately before
            // the signal; once selected, it must not be replaced during cleanup.
            match try_wait_preserving_process_group(&mut child) {
                Ok(Some(status)) => {
                    select_primary_outcome();
                    break status;
                }
                Ok(None) => {}
                Err(error) => {
                    select_primary_outcome();
                    let process_cleaned = terminate_and_reap_logged(
                        &mut child,
                        "could not clean up app after wait failure",
                    );
                    if let Some(ownership) = route_ownership.as_ref() {
                        remove_route_best_effort(&store, ownership, &spec.name, process_cleaned);
                    }
                    return Err(error.into());
                }
            }
            select_interruption();
            let process_cleaned =
                terminate_and_reap_logged(&mut child, "could not clean up interrupted app");
            output.discard();
            if let Some(ownership) = route_ownership.as_ref() {
                remove_route_best_effort(&store, ownership, &spec.name, process_cleaned);
            }
            return Err(interruption_error(reason));
        }
        thread::sleep(Duration::from_millis(100));
    };
    let mut cleanup_error = None;
    // The direct wrapper can exit while descendants remain in its detached
    // process group. Reap the whole tree before route cleanup and output
    // finalization, regardless of the wrapper's status.
    let process_cleaned =
        terminate_and_reap_with_retry_logged(&mut child, "could not finalize app process group");
    if let Some(ownership) = route_ownership.as_ref() {
        let route_cleanup_deadline = Instant::now() + STATE_LOCK_TIMEOUT;
        let mut route_cleanup = store.remove_process_route_if_owned_cancelable_until(
            ownership,
            route_cleanup_deadline,
            || process_cleaned && force_cleanup_requested(),
        );
        if !matches!(route_cleanup, Ok(true)) {
            route_cleanup = store.remove_process_route_if_owned_cancelable_until(
                ownership,
                route_cleanup_deadline,
                || process_cleaned && force_cleanup_requested(),
            );
        }
        match route_cleanup {
            Ok(true) => {}
            Ok(false) => {
                cleanup_error = Some(anyhow::anyhow!(
                    "Development app route '{}' could not be removed after bounded retries; the dead process route is inert and can be pruned later",
                    ownership.hostname()
                ));
            }
            Err(error) => cleanup_error = Some(error),
        }
    }
    finalize_single_app_cleanup(status.success(), process_cleaned, cleanup_error, || {
        output.print_failure();
    })?;

    let exit_status = child_exit_status(&status);
    Ok(json!({
        "ok": status.success(),
        "app": spec.name,
        "hostname": spec.hostname,
        "port": port,
        "exit_status": exit_status,
    }))
}

fn finalize_single_app_cleanup(
    status_success: bool,
    process_cleaned: bool,
    cleanup_error: Option<anyhow::Error>,
    print_failure: impl FnOnce(),
) -> Result<()> {
    if !status_success {
        print_failure();
        if !process_cleaned {
            eprintln!("jig proxy app process-tree cleanup was not confirmed after bounded retries");
        }
        if let Some(error) = cleanup_error {
            eprintln!("jig proxy app route cleanup also failed: {error:#}");
        }
        return Ok(());
    }
    if !process_cleaned {
        if let Some(error) = cleanup_error {
            bail!(
                "Development app exited successfully, but process-tree cleanup was not confirmed and route cleanup failed: {error:#}"
            );
        }
        bail!(
            "Development app exited successfully, but process-tree cleanup was not confirmed after bounded retries"
        );
    }
    if let Some(error) = cleanup_error {
        return Err(error);
    }
    Ok(())
}

fn require_cleanup_for_success(cleanup_complete: bool, primary_failed: bool) -> Result<()> {
    if cleanup_complete || primary_failed {
        return Ok(());
    }
    bail!(
        "Development session apps exited successfully, but one or more owned process trees or routes could not be cleaned up after bounded retries"
    )
}

fn terminate_and_reap_with_retry_logged(child: &mut Child, context: &str) -> bool {
    terminate_and_reap_logged(child, context) || terminate_and_reap_logged(child, context)
}

fn prepare_certs_for_hosts_interruptible(
    settings: &ProxySettings,
    hostnames: &[String],
    interrupt_requested: &impl Fn() -> Option<TerminationReason>,
) -> Result<()> {
    if !settings.https {
        return Ok(());
    }
    let cancelled = || interrupt_requested().is_some();
    let outcome = certs::ensure_for_hosts_interruptible(settings, hostnames, &cancelled)
        .with_context(|| {
            "Failed to prepare HTTPS proxy certificates. Likely fix: run `scripts/jig proxy cert generate --force`, trust the CA with `scripts/jig proxy cert trust --accept-trust-scope`, or disable [dev].https for HTTP-only local development."
        })?;
    lock_outcome_or_interruption(outcome, interrupt_requested)?;
    Ok(())
}

#[cfg(all(test, unix))]
fn prepare_certs_for_hosts(settings: &ProxySettings, hostnames: &[String]) -> Result<()> {
    prepare_certs_for_hosts_interruptible(settings, hostnames, &|| None)
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

fn validate_dev_specs_for_session(specs: &[AppRunSpec], settings: &ProxySettings) -> Result<()> {
    if specs.is_empty() {
        bail!("No development apps were configured or discovered.");
    }
    validate_explicit_ports(specs)?;
    if specs.iter().any(|spec| spec.proxy) {
        ensure_process_routes_supported()?;
        validate_process_routes(settings, specs)?;
        ensure_unique_process_route_hostnames(specs)?;
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

fn preflight_process_routes(
    store: &StateStore,
    specs: &[AppRunSpec],
    interrupt_requested: &impl Fn() -> Option<TerminationReason>,
) -> Result<()> {
    ensure_unique_process_route_hostnames(specs)?;
    let cancelled = || interrupt_requested().is_some();
    let outcome = store.ensure_no_live_process_routes_for_hostnames_interruptible(
        specs
            .iter()
            .filter(|spec| spec.proxy)
            .map(|spec| spec.hostname.as_str()),
        &cancelled,
    )?;
    lock_outcome_or_interruption(outcome, interrupt_requested)
}

fn lock_outcome_or_interruption<T>(
    outcome: LockOutcome<T>,
    interrupt_requested: &impl Fn() -> Option<TerminationReason>,
) -> Result<T> {
    match outcome {
        LockOutcome::Acquired(value) => Ok(value),
        LockOutcome::Cancelled => match interrupt_requested() {
            Some(reason) => {
                select_interruption();
                Err(interruption_error(reason))
            }
            None => bail!("foreground state wait was cancelled without a termination request"),
        },
    }
}

fn ensure_not_interrupted_with(
    interrupted: impl FnOnce() -> Option<TerminationReason>,
) -> Result<()> {
    if let Some(reason) = interrupted() {
        return Err(interruption_error(reason));
    }
    Ok(())
}

fn child_exit_status(status: &ExitStatus) -> i32 {
    if let Some(status) = status.code() {
        return status;
    }
    #[cfg(unix)]
    if let Some(signal) = status.signal() {
        return 128 + signal;
    }
    1
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
    spawn_child_with_cleanup_report(spec, argv, port, settings, dev_env)
        .map_err(|failure| failure.error)
}

fn spawn_child_with_cleanup_report(
    spec: &AppRunSpec,
    argv: &[String],
    port: u16,
    settings: &ProxySettings,
    dev_env: &[(String, String)],
) -> std::result::Result<SpawnedChild, SpawnChildFailure> {
    ensure_app_supervision_supported(cfg!(unix), cfg!(windows))?;
    // App commands are trusted repo-configured dev processes and intentionally
    // inherit the caller's environment. Derived app coordinates are replaced
    // below; only the background proxy clears the broader environment.
    #[cfg(windows)]
    let working_directory = child_working_directory(&spec.dir).with_context(|| {
        format!(
            "Development app '{}' directory {} cannot be represented safely for a child process",
            spec.name,
            spec.dir.display()
        )
    })?;
    #[cfg(not(windows))]
    let working_directory = child_working_directory(&spec.dir);
    #[cfg(windows)]
    let mut command =
        build_app_child_command(argv, &working_directory, dev_env).with_context(|| {
            format!(
                "Failed to prepare command '{}' for dev app '{}'",
                argv[0], spec.name
            )
        })?;
    #[cfg(not(windows))]
    let mut command = build_app_child_command(argv, &working_directory, dev_env);
    command.current_dir(&working_directory);
    apply_dev_child_environment(
        &mut command,
        argv,
        std::env::vars_os().map(|(key, _)| key),
        dev_env,
    );
    command
        // Astro 7 detects agent environments and may background its dev server.
        // Its documented zero value keeps Jig's child in the foreground-owned
        // process group; non-Astro commands ignore it.
        .env("ASTRO_DEV_BACKGROUND", "0")
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
    let spawned_process = DevProcessIdentity {
        pid: child.id(),
        start_token: process_start_token(child.id()),
    };
    #[cfg(windows)]
    let process_lease = match register_app_child(&mut child) {
        Ok(lease) => lease,
        Err(error) => {
            let cleanup_confirmed = terminate_and_reap_logged(
                &mut child,
                "could not clean up app after process-tree registration failure",
            );
            return Err(SpawnChildFailure {
                error: error.context(format!(
                    "Failed to establish an owned process-tree boundary for dev app '{}'",
                    spec.name
                )),
                cleanup_confirmed,
                spawned_process: Some(spawned_process),
            });
        }
    };
    #[cfg(not(windows))]
    let process_lease = register_app_child(&mut child);
    let output = match CapturedAppOutput::from_child(&mut child, &spec.name) {
        Ok(output) => output,
        Err(failure) => {
            return Err(SpawnChildFailure {
                error: failure.error,
                cleanup_confirmed: failure.cleanup_confirmed,
                spawned_process: Some(spawned_process),
            });
        }
    };
    Ok(SpawnedChild {
        child,
        output,
        process_lease,
    })
}

fn apply_dev_app_environment(
    command: &mut Command,
    inherited_keys: impl IntoIterator<Item = OsString>,
    dev_env: &[(String, String)],
) {
    // Derived app coordinates describe only the current Jig selection. Do not
    // let a value inherited from an earlier or narrower session impersonate a
    // currently managed app. Other caller environment remains project-owned.
    for key in inherited_keys {
        if is_runtime_owned_dev_app_environment_key(&key) {
            command.env_remove(key);
        }
    }
    command.envs(dev_env.iter().map(|(key, value)| (key, value)));
}

fn apply_dev_child_environment(
    command: &mut Command,
    argv: &[String],
    inherited_keys: impl IntoIterator<Item = OsString>,
    dev_env: &[(String, String)],
) {
    let inherited_keys = inherited_keys.into_iter().collect::<Vec<_>>();
    apply_dev_app_environment(command, inherited_keys.iter().cloned(), dev_env);
    if !is_generated_npm_dev_argv(argv) {
        return;
    }
    for key in inherited_keys {
        if is_managed_npm_run_environment_key(&key) {
            command.env_remove(key);
        }
    }
}

pub(crate) fn is_generated_npm_dev_argv(argv: &[String]) -> bool {
    if argv.len() < GENERATED_NPM_DEV_ARGV_PREFIX.len()
        || !argv
            .iter()
            .zip(GENERATED_NPM_DEV_ARGV_PREFIX)
            .all(|(actual, expected)| actual == expected)
    {
        return false;
    }
    let suffix = &argv[GENERATED_NPM_DEV_ARGV_PREFIX.len()..];
    suffix.is_empty()
        || (suffix.len() == 6
            && suffix[0] == "--"
            && suffix[1] == "--port"
            && suffix[2].parse::<u16>().is_ok_and(|port| port > 0)
            && suffix[3] == "--strictPort"
            && suffix[4] == "--host"
            && suffix[5] == "127.0.0.1")
}

fn is_managed_npm_run_environment_key(key: &OsStr) -> bool {
    let Some(key) = key.to_str() else {
        return false;
    };
    let prefix_len = "npm_config_".len();
    let Some(prefix) = key.get(..prefix_len) else {
        return false;
    };
    if !prefix.eq_ignore_ascii_case("npm_config_") {
        return false;
    }
    let config_key = key[prefix_len..].replace('_', "-").to_ascii_lowercase();
    MANAGED_NPM_RUN_CONFIG_KEYS.contains(&config_key.as_str())
}

fn is_runtime_owned_dev_app_environment_key(key: &OsStr) -> bool {
    let Some(key) = key.to_str() else {
        return false;
    };
    #[cfg(windows)]
    let normalized = key.to_ascii_uppercase();
    #[cfg(not(windows))]
    let normalized = key;
    let Some(derived) = normalized.strip_prefix("JIG_DEV_") else {
        return false;
    };

    ["_HOST", "_PORT", "_ORIGIN", "_URL"]
        .into_iter()
        .filter_map(|suffix| derived.strip_suffix(suffix))
        .any(|app| {
            !app.is_empty()
                && app
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        })
}

fn ensure_app_supervision_supported(unix: bool, windows: bool) -> Result<()> {
    if unix || windows {
        return Ok(());
    }
    bail!(
        "Development app process-tree supervision is unsupported on this platform; refusing to spawn an app that Jig cannot reliably clean up"
    )
}

#[cfg(windows)]
fn child_working_directory(path: &Path) -> Result<std::path::PathBuf> {
    windows_launch::windows_command_compatible_path(path).map_err(anyhow::Error::new)
}

#[cfg(not(windows))]
fn child_working_directory(path: &Path) -> std::path::PathBuf {
    path.to_path_buf()
}

#[cfg(windows)]
fn build_app_child_command(
    argv: &[String],
    working_directory: &Path,
    dev_env: &[(String, String)],
) -> Result<Command> {
    windows_launch::build_windows_app_command(argv, working_directory, dev_env)
}

#[cfg(not(windows))]
fn build_app_child_command(
    argv: &[String],
    _working_directory: &Path,
    _dev_env: &[(String, String)],
) -> Command {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    command
}

fn remove_route_best_effort(
    store: &StateStore,
    ownership: &ProcessRouteOwnership,
    app_name: &str,
    process_cleaned: bool,
) {
    match store.remove_process_route_if_owned_cancelable(ownership, || {
        process_cleaned && force_cleanup_requested()
    }) {
        Ok(true) => {}
        Ok(false) => eprintln!(
            "jig proxy skipped a contended route rewrite for '{}' after forced cleanup; the dead process route is inert and can be pruned later",
            ownership.hostname()
        ),
        Err(error) => {
            eprintln!(
                "jig proxy could not remove route '{}' while cleaning up app '{app_name}': {error}",
                ownership.hostname()
            );
        }
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
    const CREATE_SUSPENDED: u32 = 0x0000_0004;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED);
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

#[cfg(test)]
fn app_display(
    spec: &AppRunSpec,
    settings: &ProxySettings,
    port: u16,
    pid: u32,
    store: &StateStore,
) -> Result<AppDisplay> {
    match app_display_interruptible(spec, settings, port, pid, store, &|| false)? {
        LockOutcome::Acquired(display) => Ok(display),
        LockOutcome::Cancelled => bail!("uncancelled app display preparation was cancelled"),
    }
}

fn app_display_interruptible(
    spec: &AppRunSpec,
    settings: &ProxySettings,
    port: u16,
    pid: u32,
    store: &StateStore,
    cancelled: &impl Fn() -> bool,
) -> Result<LockOutcome<AppDisplay>> {
    if spec.proxy {
        let (scheme, proxy_port) = match proxy_origin_interruptible(settings, store, cancelled)? {
            LockOutcome::Acquired(origin) => origin,
            LockOutcome::Cancelled => return Ok(LockOutcome::Cancelled),
        };
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
        return Ok(LockOutcome::Acquired(AppDisplay {
            name: spec.name.clone(),
            url: format!("{scheme}://{}:{proxy_port}", spec.hostname),
            pid,
            lan_note,
        }));
    }
    Ok(LockOutcome::Acquired(AppDisplay {
        name: spec.name.clone(),
        url: format!("http://{}:{port}", spec.target_host),
        pid,
        lan_note: None,
    }))
}

#[cfg(test)]
fn dev_app_environment<'a>(
    apps: impl IntoIterator<Item = (&'a AppRunSpec, u16)>,
    settings: &ProxySettings,
    store: &StateStore,
) -> Result<Vec<(String, String)>> {
    match dev_app_environment_interruptible(apps, settings, store, &|| false)? {
        LockOutcome::Acquired(environment) => Ok(environment),
        LockOutcome::Cancelled => bail!("uncancelled app environment preparation was cancelled"),
    }
}

fn dev_app_environment_interruptible<'a>(
    apps: impl IntoIterator<Item = (&'a AppRunSpec, u16)>,
    settings: &ProxySettings,
    store: &StateStore,
    cancelled: &impl Fn() -> bool,
) -> Result<LockOutcome<Vec<(String, String)>>> {
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
        let origin = match app_origin_interruptible(spec, settings, port, store, cancelled)? {
            LockOutcome::Acquired(origin) => origin,
            LockOutcome::Cancelled => return Ok(LockOutcome::Cancelled),
        };
        env.push((format!("{prefix}_ORIGIN"), origin.clone()));
        env.push((format!("{prefix}_URL"), origin));
    }
    Ok(LockOutcome::Acquired(env))
}

#[cfg(test)]
fn app_origin(
    spec: &AppRunSpec,
    settings: &ProxySettings,
    port: u16,
    store: &StateStore,
) -> Result<String> {
    match app_origin_interruptible(spec, settings, port, store, &|| false)? {
        LockOutcome::Acquired(origin) => Ok(origin),
        LockOutcome::Cancelled => bail!("uncancelled app origin preparation was cancelled"),
    }
}

fn app_origin_interruptible(
    spec: &AppRunSpec,
    settings: &ProxySettings,
    port: u16,
    store: &StateStore,
    cancelled: &impl Fn() -> bool,
) -> Result<LockOutcome<String>> {
    if !spec.proxy {
        return Ok(LockOutcome::Acquired(format!(
            "http://{}:{port}",
            spec.target_host
        )));
    }

    let (scheme, proxy_port) = match proxy_origin_interruptible(settings, store, cancelled)? {
        LockOutcome::Acquired(origin) => origin,
        LockOutcome::Cancelled => return Ok(LockOutcome::Cancelled),
    };
    Ok(LockOutcome::Acquired(format!(
        "{scheme}://{}:{proxy_port}",
        spec.hostname
    )))
}
fn proxy_origin_interruptible(
    settings: &ProxySettings,
    store: &StateStore,
    cancelled: &impl Fn() -> bool,
) -> Result<LockOutcome<(&'static str, u16)>> {
    if settings.https {
        let port = match store.read_https_port_interruptible(cancelled)? {
            LockOutcome::Acquired(port) => port,
            LockOutcome::Cancelled => return Ok(LockOutcome::Cancelled),
        };
        if let Some(port) = port {
            return Ok(LockOutcome::Acquired(("https", port)));
        }
    }
    let port = match store.read_http_port_interruptible(cancelled)? {
        LockOutcome::Acquired(port) => port,
        LockOutcome::Cancelled => return Ok(LockOutcome::Cancelled),
    };
    Ok(LockOutcome::Acquired((
        "http",
        port.unwrap_or(settings.http_port),
    )))
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
