use std::collections::HashSet;
use std::path::Path;
use std::process::Child;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::dev_sessions::{DevCleanupLease, DevSessionRuntime};
use crate::state::{
    DevProcessIdentity, LockOutcome, ProcessRouteOwnership, StateStore, now_ms, process_start_token,
};
use crate::types::{AppRunSpec, ProxySettings, Route, RouteMode};
use crate::{DevPreflightError, DevPreflightResult};

use super::{
    PROXY_HEALTH_CHECK_INTERVAL, PreparedApp, RunningChild, SpawnChildFailure, SpawnedChild,
    StartupOutputDisposition, TerminationReason, app_display_interruptible, arm_owned_resources,
    child_exit_status, choose_app_port, cleanup_children, command_argv,
    dev_app_environment_interruptible, ensure_not_interrupted_with,
    ensure_process_routes_supported, ensure_proxy_running_interruptible, force_cleanup_requested,
    interruption_error, interruption_reason, is_interruption, lock_outcome_or_interruption,
    new_route_cleanup_deadline, preflight_process_routes, prepare_certs_for_hosts_interruptible,
    print_dev_table, process_route_parts, proxy_health_failed, proxy_ready_interruptible,
    publish_process_route_interruptible, require_cleanup_for_success, select_interruption,
    select_primary_outcome, spawn_child_with_cleanup_report, start_termination_cleanup_session,
    terminate_and_reap_logged, termination_requested, try_wait_preserving_process_group,
    validate_dev_specs_for_session, validate_explicit_ports, validate_process_routes,
    wait_for_app_ready_interruptible,
};

pub(crate) fn run_apps_with_preflight(
    repo_name: &str,
    root: &Path,
    specs: Vec<AppRunSpec>,
    settings: &ProxySettings,
    current_exe: &Path,
    replace: bool,
    preflight: impl FnOnce(&[AppRunSpec], &dyn Fn() -> bool) -> DevPreflightResult,
) -> Result<Value> {
    let _termination_session = start_termination_cleanup_session()?;
    validate_dev_specs_for_session(&specs, settings)?;
    // Session state and the preflight can each outlive the instruction that
    // creates them. Arm before either exists so a signal requests ordinary
    // cleanup instead of taking the no-resources `_exit` path.
    arm_owned_resources()?;
    let store = match StateStore::resolve_interruptible(settings.state_dir.clone(), &|| {
        termination_requested().is_some()
    })? {
        LockOutcome::Acquired(store) => store,
        LockOutcome::Cancelled => {
            let reason = termination_requested().ok_or_else(|| {
                anyhow::anyhow!(
                    "proxy state resolution was cancelled without a pending termination request"
                )
            })?;
            return Err(interruption_error(reason));
        }
    };
    let session = lock_outcome_or_interruption(
        DevSessionRuntime::start_interruptible(
            store.clone(),
            repo_name,
            root,
            &specs,
            replace,
            &|| termination_requested().is_some(),
        )?,
        &termination_requested,
    )?;
    let requested_reason = || {
        termination_requested().or_else(|| {
            session
                .requested_stop()
                .then_some(TerminationReason::requested_stop())
        })
    };
    let interrupted = || requested_reason().is_some();
    let mut preflight_cleanup = lock_outcome_or_interruption(
        session.begin_preflight_cleanup_interruptible(&interrupted)?,
        &requested_reason,
    )
    .context("Failed to persist cleanup intent before development preflight")?;
    let preflight_result = preflight(&specs, &interrupted);
    finish_preflight_cleanup(
        &session,
        &mut preflight_cleanup,
        preflight_result,
        &requested_reason,
    )?;
    ensure_not_interrupted_with(requested_reason)?;
    let result = run_apps_with_session_and_interrupt_probe(
        specs,
        settings,
        current_exe,
        store,
        &session,
        requested_reason,
    );
    if result
        .as_ref()
        .err()
        .and_then(interruption_reason)
        .is_some_and(TerminationReason::is_requested_stop)
        && !session.cleanup_is_confirmed()
    {
        bail!(
            "Jig dev received a stop request, but process-tree or route cleanup could not be confirmed; the session was retained for inspection"
        );
    }
    result
}

pub(super) fn normalize_preflight_result(
    result: DevPreflightResult,
    termination_reason: Option<TerminationReason>,
) -> Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(DevPreflightError::Cancelled) => match termination_reason {
            Some(reason) => Err(interruption_error(reason)),
            None => bail!(
                "Development preflight reported cancellation without a pending termination request"
            ),
        },
        Err(DevPreflightError::Failed(error) | DevPreflightError::CleanupUnconfirmed(error)) => {
            Err(error)
        }
    }
}

pub(super) fn finish_preflight_cleanup(
    session: &DevSessionRuntime,
    cleanup: &mut DevCleanupLease,
    result: DevPreflightResult,
    termination_reason: &impl Fn() -> Option<TerminationReason>,
) -> Result<()> {
    if result.is_ok() {
        let cancelled = || termination_reason().is_some();
        lock_outcome_or_interruption(
            session.confirm_preflight_cleanup_interruptible(cleanup, &cancelled)?,
            termination_reason,
        )
        .context("Failed to persist confirmed development preflight cleanup")?;
    } else if !matches!(&result, Err(DevPreflightError::CleanupUnconfirmed(_))) {
        cleanup.confirm();
    }
    normalize_preflight_result(result, termination_reason())
}

fn run_apps_with_session_and_interrupt_probe(
    specs: Vec<AppRunSpec>,
    settings: &ProxySettings,
    current_exe: &Path,
    store: StateStore,
    session: &DevSessionRuntime,
    interrupt_requested: impl Fn() -> Option<TerminationReason>,
) -> Result<Value> {
    if specs.is_empty() {
        bail!("No development apps were configured or discovered.");
    }
    validate_explicit_ports(&specs)?;
    let uses_proxy = specs.iter().any(|spec| spec.proxy);
    let cancelled = || interrupt_requested().is_some();
    if uses_proxy {
        ensure_process_routes_supported()?;
        validate_process_routes(settings, &specs)?;
        preflight_process_routes(&store, &specs, &interrupt_requested)?;
        let hostnames: Vec<String> = specs
            .iter()
            .filter(|spec| spec.proxy)
            .map(|spec| spec.hostname.clone())
            .collect();
        prepare_certs_for_hosts_interruptible(settings, &hostnames, &interrupt_requested)?;
        lock_outcome_or_interruption(
            ensure_proxy_running_interruptible(&store, settings, current_exe, &cancelled)?,
            &interrupt_requested,
        )?;
    }
    ensure_not_interrupted_with(&interrupt_requested)?;
    let mut children = Vec::new();
    let route_cleanup_deadline = new_route_cleanup_deadline();
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
    let dev_env = lock_outcome_or_interruption(
        dev_app_environment_interruptible(
            prepared_apps
                .iter()
                .map(|prepared| (&prepared.spec, prepared.port)),
            settings,
            &store,
            &cancelled,
        )?,
        &interrupt_requested,
    )?;

    arm_owned_resources()?;
    let mark_running_result = session
        .mark_running_interruptible(&cancelled)
        .and_then(|outcome| lock_outcome_or_interruption(outcome, &interrupt_requested));
    if let Err(error) = mark_running_result {
        if is_interruption(&error) {
            select_interruption();
        } else {
            select_primary_outcome();
        }
        return Err(error).context("Failed to mark the Jig dev session as running");
    }
    for prepared in prepared_apps {
        if let Err(error) = ensure_not_interrupted_with(&interrupt_requested) {
            select_interruption();
            cleanup_dev_session_children(session, &mut children);
            for running in &mut children {
                running.output.discard();
            }
            return Err(error);
        }
        let PreparedApp {
            spec,
            route_parts,
            port,
            argv,
        } = prepared;
        let cleanup_scope_result = session
            .prepare_app_spawn_interruptible(&spec.name, port, &cancelled)
            .and_then(|outcome| lock_outcome_or_interruption(outcome, &interrupt_requested));
        if let Err(error) = cleanup_scope_result {
            let interrupted = is_interruption(&error);
            if interrupted {
                select_interruption();
            } else {
                select_primary_outcome();
            }
            cleanup_dev_session_children(session, &mut children);
            if interrupted {
                for running in &mut children {
                    running.output.discard();
                }
            }
            return Err(error)
                .context("Failed to persist cleanup intent before starting a development app");
        }
        let mut session_cleanup = session.arm_cleanup();
        let SpawnedChild {
            mut child,
            mut output,
            process_lease,
        } = match spawn_child_with_cleanup_report(&spec, &argv, port, settings, &dev_env) {
            Ok(spawned) => spawned,
            Err(failure) => {
                let SpawnChildFailure {
                    mut error,
                    cleanup_confirmed,
                    spawned_process,
                } = failure;
                if cleanup_confirmed {
                    let cleanup_cancelled =
                        || force_cleanup_requested() || session.requested_stop();
                    match session
                        .confirm_app_spawn_absent_cleanup_cancelable(&spec.name, &cleanup_cancelled)
                    {
                        Ok(Some(())) => {}
                        Ok(None) => {
                            error = error.context(
                                "Forced cleanup cancelled a contended attempt to confirm that the failed app spawn left no process behind; conservative spawn-pending evidence remains in the Jig dev session",
                            );
                        }
                        Err(record_error) => {
                            error = error.context(format!(
                                "Failed to confirm in the Jig dev session that the app spawn left no process behind: {record_error:#}"
                            ));
                        }
                    }
                    session_cleanup.confirm();
                } else if let Some(process) = spawned_process {
                    let cleanup_cancelled =
                        || force_cleanup_requested() || session.requested_stop();
                    match session.record_app_process_cleanup_cancelable(
                        &spec.name,
                        port,
                        process,
                        &cleanup_cancelled,
                    ) {
                        Ok(Some(())) => {}
                        Ok(None) => {
                            error = error.context(
                                "Forced cleanup cancelled a contended attempt to retain the unconfirmed process identity; generic cleanup-required evidence remains in the Jig dev session",
                            );
                        }
                        Err(record_error) => {
                            error = error.context(format!(
                                "Failed to retain the unconfirmed process identity in the Jig dev session: {record_error:#}"
                            ));
                        }
                    }
                }
                select_primary_outcome();
                cleanup_dev_session_children(session, &mut children);
                return Err(error);
            }
        };
        let child_pid = child.id();
        let session_process = DevProcessIdentity {
            pid: child_pid,
            start_token: process_start_token(child_pid),
        };
        let record_process_result = session
            .record_app_process_interruptible(&spec.name, port, session_process.clone(), &cancelled)
            .and_then(|outcome| lock_outcome_or_interruption(outcome, &interrupt_requested));
        if let Err(error) = record_process_result {
            let interrupted = is_interruption(&error);
            if interrupted {
                select_interruption();
            } else {
                select_primary_outcome();
            }
            cleanup_dev_session_current_and_children(
                session,
                &mut session_cleanup,
                &mut child,
                "could not clean up app after dev-session registration failure",
                &mut children,
            );
            if interrupted {
                output.finish_start_failure(StartupOutputDisposition::Interrupted);
                for running in &mut children {
                    running
                        .output
                        .finish_start_failure(StartupOutputDisposition::Interrupted);
                }
            } else {
                output.finish_start_failure(StartupOutputDisposition::Failure);
            }
            return Err(error).with_context(|| {
                format!(
                    "Failed to persist process identity for development app '{}'",
                    spec.name
                )
            });
        }
        let owner_start_token = if spec.proxy {
            match wait_for_app_ready_interruptible(&spec, port, &mut child, &interrupt_requested) {
                Ok(token) => token,
                Err(error) => {
                    if is_interruption(&error) {
                        select_interruption();
                    } else {
                        select_primary_outcome();
                    }
                    cleanup_dev_session_current_and_children(
                        session,
                        &mut session_cleanup,
                        &mut child,
                        "could not clean up app after readiness failure",
                        &mut children,
                    );
                    if is_interruption(&error) {
                        output.finish_start_failure(StartupOutputDisposition::Interrupted);
                        for running in &mut children {
                            running
                                .output
                                .finish_start_failure(StartupOutputDisposition::Interrupted);
                        }
                    } else {
                        output.finish_start_failure(StartupOutputDisposition::Failure);
                    }
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
                cleanup_dev_session_current_and_children(
                    session,
                    &mut session_cleanup,
                    &mut child,
                    "could not clean up app after missing owner identity",
                    &mut children,
                );
                output.finish_start_failure(StartupOutputDisposition::Failure);
                bail!(
                    "Could not verify start identity for child process {child_pid}; refusing to publish process route"
                );
            };
            if session_process.start_token.as_deref() != Some(owner_start_token.as_str()) {
                select_primary_outcome();
                cleanup_dev_session_current_and_children(
                    session,
                    &mut session_cleanup,
                    &mut child,
                    "could not clean up app after its process identity changed during readiness",
                    &mut children,
                );
                output.finish_start_failure(StartupOutputDisposition::Failure);
                bail!(
                    "Development app '{}' changed process identity before its route could be published",
                    spec.name
                );
            }
            let Some((hostname, target_host)) = route_parts else {
                select_primary_outcome();
                cleanup_dev_session_current_and_children(
                    session,
                    &mut session_cleanup,
                    &mut child,
                    "could not clean up app after route preparation failure",
                    &mut children,
                );
                output.finish_start_failure(StartupOutputDisposition::Failure);
                bail!(
                    "Could not prepare process route for child process {child_pid}; refusing to publish route"
                );
            };
            let route = Route {
                hostname,
                target_host,
                target_port: port,
                owner_pid: Some(child_pid),
                owner_start_token: Some(owner_start_token.clone()),
                mode: RouteMode::Process,
                created_at_ms: now_ms(),
            };
            let ownership =
                ProcessRouteOwnership::new(route.hostname.clone(), child_pid, owner_start_token);
            if let Err(error) = publish_process_route_interruptible(
                &store,
                route.clone(),
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
                let failed_child = children.len();
                children.push(RunningChild {
                    name: spec.name,
                    store: store.clone(),
                    child,
                    output,
                    _process_lease: process_lease,
                    process_cleanup_armed: true,
                    route_ownership: Some(ownership),
                    route_cleanup_armed: true,
                    route_cleanup_deadline,
                    session_cleanup,
                });
                cleanup_dev_session_children(session, &mut children);
                if interrupted {
                    for running in &mut children {
                        running
                            .output
                            .finish_start_failure(StartupOutputDisposition::Interrupted);
                    }
                } else {
                    children[failed_child]
                        .output
                        .finish_start_failure(StartupOutputDisposition::Failure);
                }
                return Err(error);
            }
            if let Err(error) = ensure_not_interrupted_with(&interrupt_requested) {
                select_interruption();
                children.push(RunningChild {
                    name: spec.name,
                    store,
                    child,
                    output,
                    _process_lease: process_lease,
                    process_cleanup_armed: true,
                    route_ownership: Some(ownership),
                    route_cleanup_armed: true,
                    route_cleanup_deadline,
                    session_cleanup,
                });
                cleanup_dev_session_children(session, &mut children);
                for running in &mut children {
                    running
                        .output
                        .finish_start_failure(StartupOutputDisposition::Interrupted);
                }
                return Err(error);
            }
            route_ownership = Some(ownership);
            routes.push(route);
        }
        let display_result =
            app_display_interruptible(&spec, settings, port, child_pid, &store, &cancelled)
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
                let failed_child = children.len();
                children.push(RunningChild {
                    name: spec.name,
                    store,
                    child,
                    output,
                    _process_lease: process_lease,
                    process_cleanup_armed: true,
                    route_cleanup_armed: route_ownership.is_some(),
                    route_ownership,
                    route_cleanup_deadline,
                    session_cleanup,
                });
                cleanup_dev_session_children(session, &mut children);
                if interrupted {
                    for running in &mut children {
                        running
                            .output
                            .finish_start_failure(StartupOutputDisposition::Interrupted);
                    }
                } else {
                    children[failed_child]
                        .output
                        .finish_start_failure(StartupOutputDisposition::Failure);
                }
                return Err(error);
            }
        };
        display_rows.push(display);
        children.push(RunningChild {
            name: spec.name,
            store: store.clone(),
            child,
            output,
            _process_lease: process_lease,
            process_cleanup_armed: true,
            route_cleanup_armed: route_ownership.is_some(),
            route_ownership,
            route_cleanup_deadline: route_cleanup_deadline.clone(),
            session_cleanup,
        });
    }

    print_dev_table(&display_rows);

    let mut first_exit = None;
    let mut proxy_stopped = false;
    let mut interrupted = None;
    let mut failed_child = None;
    let mut proxy_health_misses = 0u8;
    let mut next_proxy_health_check = Instant::now() + PROXY_HEALTH_CHECK_INTERVAL;
    while first_exit.is_none() {
        for (index, running) in children.iter_mut().enumerate() {
            match try_wait_preserving_process_group(&mut running.child) {
                Ok(Some(status)) => {
                    if !status.success() {
                        failed_child = Some(index);
                    }
                    first_exit = Some((running.name.clone(), child_exit_status(&status)));
                    break;
                }
                Ok(None) => {}
                Err(error) => {
                    select_primary_outcome();
                    cleanup_dev_session_children(session, &mut children);
                    return Err(error.into());
                }
            }
        }
        if first_exit.is_some() {
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
                            failed_child = Some(index);
                        }
                        first_exit = Some((running.name.clone(), child_exit_status(&status)));
                        break;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        select_primary_outcome();
                        cleanup_dev_session_children(session, &mut children);
                        return Err(error.into());
                    }
                }
            }
            if first_exit.is_some() {
                select_primary_outcome();
                break;
            }
            select_interruption();
            interrupted = Some(reason);
            break;
        }
        if first_exit.is_none() && uses_proxy && Instant::now() >= next_proxy_health_check {
            next_proxy_health_check = Instant::now() + PROXY_HEALTH_CHECK_INTERVAL;
            let proxy_is_ready = match proxy_ready_interruptible(&store, settings, &cancelled) {
                Ok(LockOutcome::Acquired(ready)) => ready,
                Ok(LockOutcome::Cancelled) => {
                    let Some(reason) = interrupt_requested() else {
                        select_primary_outcome();
                        cleanup_dev_session_children(session, &mut children);
                        bail!(
                            "foreground runtime-state wait was cancelled without a termination request"
                        );
                    };
                    select_interruption();
                    interrupted = Some(reason);
                    break;
                }
                Err(error) => {
                    select_primary_outcome();
                    cleanup_dev_session_children(session, &mut children);
                    return Err(error);
                }
            };
            if proxy_health_failed(&mut proxy_health_misses, proxy_is_ready) {
                first_exit = Some(("jig proxy".to_string(), 1));
                proxy_stopped = true;
                select_primary_outcome();
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
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
    } else if interrupted.is_none() {
        if let Some((name, code)) = &first_exit {
            eprintln!("{name} exited with status {code}; stopping development session");
        }
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

fn cleanup_dev_session_children(
    session: &DevSessionRuntime,
    children: &mut [RunningChild],
) -> bool {
    let complete = cleanup_children(children);
    complete && session.cleanup_is_confirmed()
}

fn cleanup_dev_session_current_and_children(
    session: &DevSessionRuntime,
    session_cleanup: &mut DevCleanupLease,
    child: &mut Child,
    context: &str,
    children: &mut [RunningChild],
) -> bool {
    let current_complete = terminate_and_reap_logged(child, context);
    if current_complete {
        session_cleanup.confirm();
    }
    let prior_complete = cleanup_children(children);
    current_complete && prior_complete && session.cleanup_is_confirmed()
}

#[cfg(all(test, not(windows)))]
pub(super) fn run_apps_with_interrupt_probe(
    specs: Vec<AppRunSpec>,
    settings: &ProxySettings,
    current_exe: &Path,
    interrupt_requested: impl Fn() -> Option<TerminationReason>,
) -> Result<Value> {
    validate_dev_specs_for_session(&specs, settings)?;
    let root = specs
        .first()
        .map(|spec| spec.dir.as_path())
        .ok_or_else(|| anyhow::anyhow!("test dev session requires at least one app"))?;
    let store = StateStore::resolve(settings.state_dir.clone())?;
    let cancelled = || interrupt_requested().is_some();
    let session = lock_outcome_or_interruption(
        DevSessionRuntime::start_interruptible(
            store.clone(),
            "test",
            root,
            &specs,
            false,
            &cancelled,
        )?,
        &interrupt_requested,
    )?;
    run_apps_with_session_and_interrupt_probe(
        specs,
        settings,
        current_exe,
        store,
        &session,
        interrupt_requested,
    )
}
