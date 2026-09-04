use std::collections::HashSet;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;

use super::*;

pub(super) fn prepare_proxy_for_apps(
    specs: &[AppRunSpec],
    settings: &ProxySettings,
    current_exe: &Path,
    store: &StateStore,
    interrupt_requested: &impl Fn() -> Option<TerminationReason>,
    cancelled: &impl Fn() -> bool,
) -> Result<bool> {
    let uses_proxy = specs.iter().any(|spec| spec.proxy);
    if !uses_proxy {
        return Ok(false);
    }
    ensure_process_routes_supported()?;
    validate_process_routes(settings, specs)?;
    preflight_process_routes(store, specs, interrupt_requested)?;
    let hostnames = specs
        .iter()
        .filter(|spec| spec.proxy)
        .map(|spec| spec.hostname.clone())
        .collect::<Vec<_>>();
    prepare_certs_for_hosts_interruptible(settings, &hostnames, interrupt_requested)?;
    lock_outcome_or_interruption(
        ensure_proxy_running_interruptible(store, settings, current_exe, cancelled)?,
        interrupt_requested,
    )?;
    Ok(true)
}

pub(super) fn mark_dev_session_running(
    session: &DevSessionRuntime,
    cancelled: &impl Fn() -> bool,
    interrupt_requested: &impl Fn() -> Option<TerminationReason>,
) -> Result<()> {
    let result = session
        .mark_running_interruptible(cancelled)
        .and_then(|outcome| lock_outcome_or_interruption(outcome, interrupt_requested));
    result.map_err(|error| {
        if is_interruption(&error) {
            select_interruption();
        } else {
            select_primary_outcome();
        }
        error.context("Failed to mark the Jig dev session as running")
    })
}

pub(super) fn ensure_app_start_not_interrupted(
    session: &DevSessionRuntime,
    children: &mut [RunningChild],
    interrupt_requested: &impl Fn() -> Option<TerminationReason>,
) -> Result<()> {
    if let Err(error) = ensure_not_interrupted_with(interrupt_requested) {
        select_interruption();
        cleanup_dev_session_children(session, children);
        for running in children {
            running.output.discard();
        }
        return Err(error);
    }
    Ok(())
}

pub(super) fn prepare_app_spawn(
    session: &DevSessionRuntime,
    spec: &AppRunSpec,
    port: u16,
    cancelled: &impl Fn() -> bool,
    interrupt_requested: &impl Fn() -> Option<TerminationReason>,
    children: &mut [RunningChild],
) -> Result<()> {
    let result = session
        .prepare_app_spawn_interruptible(&spec.name, port, cancelled)
        .and_then(|outcome| lock_outcome_or_interruption(outcome, interrupt_requested));
    if let Err(error) = result {
        let interrupted = is_interruption(&error);
        if interrupted {
            select_interruption();
        } else {
            select_primary_outcome();
        }
        cleanup_dev_session_children(session, children);
        if interrupted {
            for running in children {
                running.output.discard();
            }
        }
        return Err(error)
            .context("Failed to persist cleanup intent before starting a development app");
    }
    Ok(())
}

pub(super) fn prepare_apps(
    specs: Vec<AppRunSpec>,
    settings: &ProxySettings,
) -> Result<Vec<PreparedApp>> {
    let mut assigned_ports = HashSet::new();
    specs
        .into_iter()
        .map(|spec| {
            let route_parts = spec
                .proxy
                .then(|| process_route_parts(settings, &spec))
                .transpose()?;
            let port = choose_app_port(spec.explicit_port, &spec.target_host, &mut assigned_ports)?;
            let argv = command_argv(&spec.command, &spec.kind, port)?;
            if argv.is_empty() {
                bail!("No command configured for app '{}'", spec.name);
            }
            Ok(PreparedApp {
                spec,
                route_parts,
                port,
                argv,
            })
        })
        .collect()
}

pub(super) struct DevSessionOutcome {
    pub(super) first_exit: Option<(String, i32)>,
    pub(super) proxy_stopped: bool,
    pub(super) interrupted: Option<TerminationReason>,
    pub(super) failed_child: Option<usize>,
}

pub(super) fn monitor_dev_session(
    children: &mut [RunningChild],
    uses_proxy: bool,
    store: &StateStore,
    settings: &ProxySettings,
    session: &DevSessionRuntime,
    interrupt_requested: &impl Fn() -> Option<TerminationReason>,
    cancelled: &impl Fn() -> bool,
) -> Result<DevSessionOutcome> {
    let mut outcome = DevSessionOutcome {
        first_exit: None,
        proxy_stopped: false,
        interrupted: None,
        failed_child: None,
    };
    let mut proxy_health_misses = 0u8;
    let mut next_proxy_health_check = Instant::now() + PROXY_HEALTH_CHECK_INTERVAL;
    while outcome.first_exit.is_none() {
        for (index, running) in children.iter_mut().enumerate() {
            match try_wait_preserving_process_group(&mut running.child) {
                Ok(Some(status)) => {
                    if !status.success() {
                        outcome.failed_child = Some(index);
                    }
                    outcome.first_exit = Some((running.name.clone(), child_exit_status(&status)));
                    break;
                }
                Ok(None) => {}
                Err(error) => {
                    select_primary_outcome();
                    cleanup_dev_session_children(session, children);
                    return Err(error.into());
                }
            }
        }
        if outcome.first_exit.is_some() {
            select_primary_outcome();
            break;
        }
        if let Some(reason) = interrupt_requested() {
            // Close the small observation race by checking every child once
            // more before accepting interruption as the terminal outcome.
            for (index, running) in children.iter_mut().enumerate() {
                match try_wait_preserving_process_group(&mut running.child) {
                    Ok(Some(status)) => {
                        if !status.success() {
                            outcome.failed_child = Some(index);
                        }
                        outcome.first_exit =
                            Some((running.name.clone(), child_exit_status(&status)));
                        break;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        select_primary_outcome();
                        cleanup_dev_session_children(session, children);
                        return Err(error.into());
                    }
                }
            }
            if outcome.first_exit.is_some() {
                select_primary_outcome();
                break;
            }
            select_interruption();
            outcome.interrupted = Some(reason);
            break;
        }
        if outcome.first_exit.is_none() && uses_proxy && Instant::now() >= next_proxy_health_check {
            next_proxy_health_check = Instant::now() + PROXY_HEALTH_CHECK_INTERVAL;
            let proxy_is_ready = match proxy_ready_interruptible(store, settings, cancelled) {
                Ok(LockOutcome::Acquired(ready)) => ready,
                Ok(LockOutcome::Cancelled) => {
                    let Some(reason) = interrupt_requested() else {
                        select_primary_outcome();
                        cleanup_dev_session_children(session, children);
                        bail!(
                            "foreground runtime-state wait was cancelled without a termination request"
                        );
                    };
                    select_interruption();
                    outcome.interrupted = Some(reason);
                    break;
                }
                Err(error) => {
                    select_primary_outcome();
                    cleanup_dev_session_children(session, children);
                    return Err(error);
                }
            };
            if proxy_health_failed(&mut proxy_health_misses, proxy_is_ready) {
                outcome.first_exit = Some(("jig proxy".to_string(), 1));
                outcome.proxy_stopped = true;
                select_primary_outcome();
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(outcome)
}

pub(super) fn finish_dev_session(
    mut children: Vec<RunningChild>,
    routes: Vec<Route>,
    first_exit: Option<(String, i32)>,
    proxy_stopped: bool,
    interrupted: Option<TerminationReason>,
    failed_child: Option<usize>,
    session: &DevSessionRuntime,
) -> Result<Value> {
    let cleanup_complete = cleanup_dev_session_children(session, &mut children);
    if interrupted.is_some() {
        for running in &mut children {
            running.output.discard();
        }
    } else if let Some(index) = failed_child {
        let failed = &mut children[index];
        failed.output.print_failure();
    }

    if proxy_stopped {
        eprintln!("Jig proxy stopped responding; shutting down development session");
    } else if interrupted.is_none()
        && let Some((name, code)) = &first_exit
    {
        eprintln!("{name} exited with status {code}; stopping development session");
    }

    if let Some(reason) = interrupted {
        return Err(interruption_error(reason));
    }

    let primary_failed = proxy_stopped
        || first_exit
            .as_ref()
            .is_some_and(|(_, exit_status)| *exit_status != 0);
    require_cleanup_for_success(cleanup_complete, primary_failed)?;

    Ok(json!({
        "ok": first_exit.as_ref().map(|(_, code)| *code == 0).unwrap_or(false),
        "first_exit": first_exit.map(|(name, code)| json!({ "app": name, "exit_status": code })),
        "proxy_failed": proxy_stopped,
        "routes": routes,
    }))
}
