use anyhow::Result;
use serde_json::{Value, json};

use crate::types::{DevRequest, DevStatusRequest, DevStopRequest};
use crate::{dev_resolved_with_preflight, dev_sessions, processes, resolve_dev_request};

pub fn dev(request: DevRequest) -> Result<Value> {
    dev_resolved(resolve_dev_request(request)?)
}

pub fn dev_status(request: DevStatusRequest) -> Result<Value> {
    dev_sessions::status(request)
}

pub fn dev_stop(request: DevStopRequest) -> Result<Value> {
    dev_sessions::stop(request)
}

pub fn dev_resolved(request: crate::ResolvedDevRequest) -> Result<Value> {
    dev_resolved_with_preflight(request, |_, _| Ok(()))
}

pub(crate) fn normalize_dev_result(result: Result<Value>) -> Result<Value> {
    match result {
        Err(error) => {
            let Some(reason) = processes::interruption_reason(&error) else {
                return Err(error);
            };
            if reason.is_requested_stop() {
                return Ok(json!({
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
                }));
            }
            Ok(json!({
                "ok": false,
                "interrupted": true,
                "exit_status": reason.exit_status(),
                "exit_signal": reason.signal(),
                "termination_signal": reason.label(),
                "first_exit": null,
                "proxy_failed": false,
                "routes": [],
            }))
        }
        result => result,
    }
}
