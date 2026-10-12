//! Development proxy check.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use jig_context::RepoContext;
use jig_owned_process::{ProcessOutputLimits, run_owned_process_tree_with_output_limits};
use serde_json::{Value, json};

use super::check::{DoctorCheck, check};
use super::environment::DoctorProcessControl;

const PROXY_LIST_DIAGNOSTIC_TIMEOUT: Duration = Duration::from_secs(120);

const PROXY_LIST_STDOUT_LIMIT: usize = 8 * 1024 * 1024;

pub(crate) fn proxy_configured(ctx: &RepoContext) -> bool {
    !ctx.frontend_apps().is_empty()
        || !ctx.dev_config().apps.is_empty()
        || ctx.dev_config().workspace_discovery
}

pub(super) fn proxy_check_with_process_control(
    ctx: &RepoContext,
    process_control: DoctorProcessControl<'_>,
) -> DoctorCheck {
    let label = "Dev proxy";
    let configured = proxy_configured(ctx);
    if !configured {
        return check(
            "proxy",
            label,
            false,
            true,
            "not configured",
            "no dev apps configured",
        )
        .with_data(json!({ "configured": false }));
    }
    match proxy_list_output(ctx, process_control) {
        Ok(output) => proxy_check_from_output(configured, output),
        Err(error) => check("proxy", label, false, false, "error", error.to_string())
            .with_fix("Run `scripts/jig proxy list` for proxy diagnostics.")
            .with_data(json!({ "configured": configured })),
    }
}

fn proxy_list_output(
    ctx: &RepoContext,
    process_control: DoctorProcessControl<'_>,
) -> Result<Value> {
    if let Some(reason) = process_control.unavailable_reason {
        return Err(anyhow!(
            "proxy diagnostics were not started because {reason}"
        ));
    }
    if process_control
        .cancellation
        .is_some_and(|cancelled| cancelled())
    {
        return Err(anyhow!("proxy diagnostics were cancelled before start"));
    }
    let (launcher, mut command) = proxy_list_command(ctx.root())?;
    proxy_list_output_with_timeout_and_limits_and_cancellation(
        &mut command,
        PROXY_LIST_DIAGNOSTIC_TIMEOUT,
        proxy_list_output_limits(),
        || {
            process_control
                .cancellation
                .is_some_and(|cancelled| cancelled())
        },
    )
    .with_context(|| proxy_list_failure_context(&launcher))
}

pub(super) fn proxy_list_command(root: &Path) -> Result<(PathBuf, Command)> {
    let root = std::path::absolute(root).with_context(|| {
        format!(
            "Failed to resolve the absolute proxy diagnostics root path {}",
            root.display()
        )
    })?;
    let launcher = root.join("scripts/jig");
    let mut command = Command::new(&launcher);
    jig_owned_process::sanitize_bash_environment(&mut command);
    command.args(["proxy", "list", "--json"]).current_dir(&root);
    Ok((launcher, command))
}

fn proxy_list_failure_context(launcher: &Path) -> String {
    format!(
        "Failed to run proxy diagnostics through {}",
        launcher.display()
    )
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
pub(super) fn proxy_list_output_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Result<Value> {
    proxy_list_output_with_timeout_and_limits_and_cancellation(
        command,
        timeout,
        proxy_list_output_limits(),
        || false,
    )
}

pub(super) fn proxy_list_output_limits() -> ProcessOutputLimits {
    ProcessOutputLimits {
        stdout: PROXY_LIST_STDOUT_LIMIT,
        ..ProcessOutputLimits::default()
    }
}

pub(super) fn proxy_list_output_with_timeout_and_limits_and_cancellation(
    command: &mut Command,
    timeout: Duration,
    limits: ProcessOutputLimits,
    cancelled: impl FnMut() -> bool,
) -> Result<Value> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = run_owned_process_tree_with_output_limits(command, timeout, limits, cancelled)
        .map_err(|error| anyhow!("`scripts/jig proxy list --json` failed: {error}"))?;
    let stdout = output
        .stdout
        .ok_or_else(|| anyhow!("`scripts/jig proxy list --json` stdout was not captured"))?;
    let stderr = output
        .stderr
        .ok_or_else(|| anyhow!("`scripts/jig proxy list --json` stderr was not captured"))?;

    if !stdout.complete || !stderr.complete {
        return Err(anyhow!(
            "`scripts/jig proxy list --json` output capture did not complete"
        ));
    }
    if stdout.truncated || stderr.truncated {
        return Err(anyhow!(
            "`scripts/jig proxy list --json` output exceeded the diagnostic capture limit"
        ));
    }

    if !output.status.success() {
        return Err(anyhow!(
            "`scripts/jig proxy list --json` exited with status {}",
            output.status
        ));
    }

    serde_json::from_slice(&stdout.bytes)
        .context("Failed to parse `scripts/jig proxy list --json` JSON")
}

fn proxy_check_from_output(configured: bool, output: Value) -> DoctorCheck {
    let running = output["running"].as_bool().unwrap_or(false);
    let status = match (configured, running) {
        (true, true) => "running",
        (true, false) => "not running",
        (false, true) => "running unconfigured",
        (false, false) => "not configured",
    };
    check(
        "proxy",
        "Dev proxy",
        false,
        running,
        status,
        proxy_detail(configured, running, &output),
    )
    .with_optional_fix((configured && !running).then_some("Run `scripts/jig proxy start`."))
    .with_data(json!({
            "configured": configured,
            "status": output,
    }))
}

fn proxy_detail(configured: bool, running: bool, output: &Value) -> String {
    let state_dir = output["state_dir"].as_str().unwrap_or("<unknown>");
    match (configured, running) {
        (true, true) => format!("configured and running; state_dir={state_dir}"),
        (true, false) => format!("configured but not running; state_dir={state_dir}"),
        (false, true) => format!("running, but no dev apps are configured; state_dir={state_dir}"),
        (false, false) => format!("no dev apps configured; state_dir={state_dir}"),
    }
}
