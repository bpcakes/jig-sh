//! Repository-context checks and the process session that supervises their probes.

#[cfg(unix)]
use std::fs;

use jig_context::RepoContext;

use super::agent::agent_check;
use super::check::DoctorCheck;
use super::environment::{DoctorEnvironment, DoctorProcessControl};
use super::proxy::proxy_check_with_process_control;
#[cfg(unix)]
use super::proxy::proxy_configured;
use super::required_tools::required_tools_check_with_environment_and_process_control;
use super::sqlx_cli::sqlx_cli_version_check;
use super::toolchains::{go_runtime_check, node_runtime_check, rust_runtime_check};
#[cfg(unix)]
use crate::signal_supervision::SignalSession;
#[cfg(unix)]
use crate::signal_supervision::session::finish_signal_session;

#[derive(Debug)]
pub(super) struct DoctorContextChecks {
    pub(super) required_tools: DoctorCheck,
    pub(super) rust_runtime: Option<DoctorCheck>,
    pub(super) go_runtime: Option<DoctorCheck>,
    pub(super) node_runtime: Option<DoctorCheck>,
    pub(super) sqlx_cli: Option<DoctorCheck>,
    pub(super) agent: DoctorCheck,
    pub(super) proxy: DoctorCheck,
}

pub(super) fn doctor_context_checks(ctx: &RepoContext) -> DoctorContextChecks {
    let environment = DoctorEnvironment::capture();
    #[cfg(unix)]
    {
        if !doctor_process_session_required(ctx) {
            return doctor_context_checks_with_process_control(
                ctx,
                &environment,
                DoctorProcessControl::allowed_without_signal_session(),
            );
        }
        let Ok(signal_session) = SignalSession::start() else {
            return doctor_context_checks_with_process_control(
                ctx,
                &environment,
                DoctorProcessControl::unavailable(
                    "the process-wide doctor signal session is unavailable",
                ),
            );
        };
        let cancelled = || signal_session.cancelled();
        let mut checks = doctor_context_checks_with_process_control(
            ctx,
            &environment,
            DoctorProcessControl {
                cancellation: Some(&cancelled),
                unavailable_reason: None,
            },
        );
        if finish_signal_session(signal_session).is_err() {
            mark_doctor_signal_retirement_failure(ctx, &mut checks);
        }
        checks
    }
    #[cfg(not(unix))]
    {
        doctor_context_checks_with_process_control(
            ctx,
            &environment,
            DoctorProcessControl::allowed_without_signal_session(),
        )
    }
}

pub(super) fn doctor_context_checks_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> DoctorContextChecks {
    let environment = DoctorEnvironment::capture();
    doctor_context_checks_with_process_control(
        ctx,
        &environment,
        DoctorProcessControl {
            cancellation: Some(cancelled),
            unavailable_reason: None,
        },
    )
}

fn doctor_context_checks_with_process_control(
    ctx: &RepoContext,
    environment: &DoctorEnvironment,
    process_control: DoctorProcessControl<'_>,
) -> DoctorContextChecks {
    let required_tools = required_tools_check_with_environment_and_process_control(
        ctx,
        environment,
        process_control,
    );
    let rust_runtime = rust_runtime_check(ctx, environment, process_control);
    let go_runtime = go_runtime_check(ctx, environment, process_control);
    let node_runtime = node_runtime_check(ctx, environment, process_control);
    let sqlx_cli = sqlx_cli_version_check(ctx, environment, process_control);
    let agent = agent_check(ctx, process_control);
    let proxy = proxy_check_with_process_control(ctx, process_control);
    DoctorContextChecks {
        required_tools,
        rust_runtime,
        go_runtime,
        node_runtime,
        sqlx_cli,
        agent,
        proxy,
    }
}

#[cfg(unix)]
fn doctor_process_session_required(ctx: &RepoContext) -> bool {
    let sqlx_probe_required = ctx.sqlx_enabled()
        && ctx
            .required_commands()
            .iter()
            .any(|command| command == "sqlx_check_command");
    sqlx_probe_required
        || rust_runtime_probe_required(ctx)
        || go_runtime_probe_required(ctx)
        || node_runtime_probe_required(ctx)
        || !ctx.codex_marketplaces().is_empty()
        || proxy_configured(ctx)
}

#[cfg(unix)]
fn rust_runtime_probe_required(ctx: &RepoContext) -> bool {
    fs::symlink_metadata(ctx.root().join("Cargo.toml")).is_ok()
}

#[cfg(unix)]
pub(super) fn go_runtime_probe_required(ctx: &RepoContext) -> bool {
    ctx.go_module_authority_paths()
        .is_ok_and(|paths| paths.iter().any(|path| fs::symlink_metadata(path).is_ok()))
}

#[cfg(unix)]
fn node_runtime_probe_required(ctx: &RepoContext) -> bool {
    !ctx.frontend_apps().is_empty()
        && fs::symlink_metadata(ctx.root().join(".node-version")).is_ok()
}

#[cfg(unix)]
pub(super) fn mark_doctor_signal_retirement_failure(
    ctx: &RepoContext,
    checks: &mut DoctorContextChecks,
) {
    if ctx.sqlx_enabled()
        && ctx
            .required_commands()
            .iter()
            .any(|command| command == "sqlx_check_command")
    {
        if checks.required_tools.status == "present" {
            checks.required_tools.status = "present_unverified".to_string();
        }
        checks.required_tools.detail.push_str(
            "; SQLx capability verification is incomplete because the process-wide doctor signal session could not retire safely",
        );
    }
    if let Some(node_runtime) = checks.node_runtime.as_mut()
        && node_runtime.ok
    {
        node_runtime.ok = false;
        node_runtime.status = "unverified".to_string();
        node_runtime.detail.push_str(
                "; Node runtime verification is incomplete because the process-wide doctor signal session could not retire safely",
            );
        node_runtime.fix =
            Some("Run `scripts/jig doctor` again before starting frontend work.".into());
    }
    for (runtime, label) in [
        (checks.rust_runtime.as_mut(), "Rust runtime"),
        (checks.go_runtime.as_mut(), "Go runtime"),
        (checks.sqlx_cli.as_mut(), "SQLx CLI"),
    ] {
        if let Some(runtime) = runtime
            && runtime.ok
        {
            runtime.ok = false;
            runtime.status = "unverified".to_string();
            runtime.detail.push_str(&format!(
                    "; {label} verification is incomplete because the process-wide doctor signal session could not retire safely"
                ));
            runtime.fix =
                Some("Run `scripts/jig doctor` again before starting database work.".into());
        }
    }
    if !ctx.codex_marketplaces().is_empty() {
        checks.agent.ok = false;
        checks.agent.status = "error".to_string();
        checks.agent.detail.push_str(
            "; Codex marketplace verification is incomplete because the process-wide doctor signal session could not retire safely",
        );
        checks.agent.fix = Some("Run `scripts/jig agent doctor` for agent tooling details.".into());
    }
    if proxy_configured(ctx) {
        checks.proxy.ok = false;
        checks.proxy.status = "error".to_string();
        checks.proxy.detail.push_str(
            "; proxy diagnostics are incomplete because the process-wide doctor signal session could not retire safely",
        );
        checks.proxy.fix = Some("Run `scripts/jig proxy list` for proxy diagnostics.".into());
    }
}
