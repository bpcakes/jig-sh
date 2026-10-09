//! Agent tooling check: the Codex CLI and its marketplace support.

use std::time::Duration;

use jig_context::RepoContext;
use serde_json::Value;

use super::check::{DoctorCheck, check};
use super::environment::DoctorProcessControl;
#[cfg(unix)]
use crate::signal_supervision::SignalSession;
#[cfg(unix)]
use crate::signal_supervision::session::finish_signal_session;

const CODEX_SUPPORT_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn standalone_codex_support_probe(
    codex_bin: &std::ffi::OsStr,
    timeout: Duration,
) -> crate::runtime::CodexSupportProbeResult {
    #[cfg(all(unix, not(test)))]
    {
        standalone_codex_support_probe_with_signal_session(codex_bin, timeout)
    }
    #[cfg(any(not(unix), test))]
    {
        crate::runtime::probe_codex_marketplace_support(codex_bin, timeout, || false)
    }
}

#[cfg(unix)]
pub(super) fn standalone_codex_support_probe_with_signal_session(
    codex_bin: &std::ffi::OsStr,
    timeout: Duration,
) -> crate::runtime::CodexSupportProbeResult {
    let signal_session = SignalSession::start().map_err(|_| {
        "Codex marketplace support probe was not started because the process-wide signal session is unavailable".to_string()
    })?;
    let cancelled = || signal_session.cancelled();
    let probe = crate::runtime::probe_codex_marketplace_support(codex_bin, timeout, &cancelled);
    finish_signal_session(signal_session).map_err(|_| {
        "Codex marketplace support probe supervision could not retire safely".to_string()
    })?;
    probe
}

pub(super) fn agent_check(
    ctx: &RepoContext,
    process_control: DoctorProcessControl<'_>,
) -> DoctorCheck {
    let output = crate::runtime::agent_doctor_with_codex_support_probe(ctx, |codex_bin| {
        if let Some(reason) = process_control.unavailable_reason {
            return Err(format!(
                "Codex marketplace support probe was not started because {reason}"
            ));
        }
        if process_control
            .cancellation
            .is_some_and(|cancelled| cancelled())
        {
            return Err("Codex marketplace support probe was cancelled before start".into());
        }
        crate::runtime::probe_codex_marketplace_support(
            codex_bin,
            CODEX_SUPPORT_PROBE_TIMEOUT,
            || {
                process_control
                    .cancellation
                    .is_some_and(|cancelled| cancelled())
            },
        )
    });
    let ok = output["ok"].as_bool().unwrap_or(false);
    let probe_incomplete = output["codex"]["probe_error"].is_string();
    let configured = output["marketplaces"].as_array().map(Vec::len).unwrap_or(0);
    let registered = output["marketplaces"]
        .as_array()
        .map(|marketplaces| {
            marketplaces
                .iter()
                .filter(|marketplace| marketplace["registered"].as_bool().unwrap_or(false))
                .count()
        })
        .unwrap_or(0);
    let detail = if probe_incomplete {
        "Codex marketplace capability verification is incomplete".into()
    } else if configured == 0 {
        "no agent skill marketplaces configured".into()
    } else {
        format!("{registered}/{configured} configured marketplace(s) registered")
    };
    let fix = output["next_steps"]
        .as_array()
        .and_then(|steps| agent_next_step(steps))
        .map(str::to_string);
    // Agent skills improve the Codex experience, but a repository
    // with valid config, runtime, contract, and tools is operational.
    check(
        "agent_skills",
        "Agent skills",
        false,
        ok,
        if probe_incomplete {
            "error"
        } else if ok {
            "installed"
        } else {
            "missing"
        },
        detail,
    )
    .with_optional_fix(fix.as_deref())
    .with_data(output)
}

pub(super) fn agent_next_step(steps: &[Value]) -> Option<&str> {
    steps
        .iter()
        .filter_map(Value::as_str)
        .find(|step| step.contains("`scripts/jig "))
        .or_else(|| steps.iter().filter_map(Value::as_str).next())
}
