use anyhow::Result;
use serde_json::{Value, json};

use crate::types::{DevRequest, DevStatusRequest, DevStopRequest};
use crate::{
    dev_outcome, dev_resolved_with_preflight, dev_sessions, processes, resolve_dev_request,
};

/// Resolves and runs a development request.
///
/// # Errors
///
/// Returns an error when request resolution, workspace discovery, preflight,
/// process supervision, proxy startup, or cleanup fails.
pub fn dev(request: DevRequest) -> Result<Value> {
    dev_resolved(resolve_dev_request(request)?)
}

/// Reports registered development sessions for a repository.
///
/// # Errors
///
/// Returns an error when repository identity or protected session state cannot
/// be resolved, read, validated, or pruned safely.
pub fn dev_status(request: DevStatusRequest) -> Result<Value> {
    dev_sessions::status(request)
}

/// Requests a supervised development session to stop.
///
/// # Errors
///
/// Returns an error when session state is invalid, the authenticated control
/// channel fails, or the owning supervisor cannot confirm cleanup.
pub fn dev_stop(request: DevStopRequest) -> Result<Value> {
    dev_sessions::stop(request)
}

/// Runs an already resolved development request.
///
/// # Errors
///
/// Returns an error when preflight, process supervision, proxy startup, app
/// readiness, route publication, or cleanup fails.
pub fn dev_resolved(request: crate::ResolvedDevRequest) -> Result<Value> {
    dev_resolved_with_preflight(request, |_, _| Ok(()))
}

pub(crate) fn normalize_dev_result(result: Result<Value>) -> Result<Value> {
    match result {
        Err(error) => {
            let (source, recoveries) = dev_outcome::parts(&error);
            let Some(reason) = processes::interruption_reason(source) else {
                let Some(recoveries) = recoveries else {
                    return Err(error);
                };
                let recoveries = recoveries.to_value()?;
                return Ok(json!({
                    "ok": false,
                    "interrupted": false,
                    "stopped": false,
                    "error": command_failed_error(format!("{source:#}")),
                    "exit_status": 1,
                    "exit_signal": null,
                    "termination_signal": null,
                    "first_exit": null,
                    "proxy_failed": false,
                    "routes": [],
                    "recoveries": recoveries,
                }));
            };
            let recoveries = recoveries
                .map(dev_outcome::DevRecoveries::to_value)
                .transpose()?;
            if processes::interruption_cleanup_unconfirmed(&error) {
                let mut output = json!({
                    "ok": false,
                    "interrupted": false,
                    "stopped": false,
                    "cleanup_unconfirmed": true,
                    "error": command_failed_error(processes::UNCONFIRMED_DEV_CLEANUP_MESSAGE),
                    "stop_reason": reason.label(),
                    "exit_status": 1,
                    "exit_signal": null,
                    "termination_signal": null,
                    "first_exit": null,
                    "proxy_failed": false,
                    "routes": [],
                });
                if let Some(recoveries) = recoveries {
                    output["recoveries"] = recoveries;
                }
                return Ok(output);
            }
            if reason.is_requested_stop() {
                let mut output = json!({
                    "ok": true,
                    "interrupted": false,
                    "stopped": true,
                    "stop_reason": reason.label(),
                    "exit_status": reason.exit_status(),
                    "exit_signal": null,
                    "termination_signal": null,
                    "first_exit": null,
                    "proxy_failed": false,
                    "routes": [],
                });
                if let Some(recoveries) = recoveries {
                    output["recoveries"] = recoveries;
                }
                return Ok(output);
            }
            let mut output = json!({
                "ok": false,
                "interrupted": true,
                "exit_status": reason.exit_status(),
                "exit_signal": reason.signal(),
                "termination_signal": reason.label(),
                "first_exit": null,
                "proxy_failed": false,
                "routes": [],
            });
            if let Some(recoveries) = recoveries {
                output["recoveries"] = recoveries;
            }
            Ok(output)
        }
        result => result,
    }
}

fn command_failed_error(message: impl Into<String>) -> Value {
    json!({
        "kind": "command_failed",
        "message": message.into(),
    })
}
