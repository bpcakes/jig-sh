use anyhow::{Result, bail};
use serde_json::{Value, json};

use crate::command::{LoopRunRequest, LoopTickRequest};
use crate::context::RepoContext;

use super::engine::tick;

pub(super) fn run_until(ctx: &RepoContext, request: LoopRunRequest) -> Result<Value> {
    if request.until != "idle" {
        bail!(
            "Unsupported loop run stop condition '{}'. Use --until idle.",
            request.until
        );
    }
    if request.max_ticks == 0 {
        bail!("--max-ticks must be greater than zero");
    }

    let mut ticks = Vec::new();
    let mut status = "max_ticks_reached".to_string();
    for _ in 0..request.max_ticks {
        let tick = tick(
            ctx,
            LoopTickRequest {
                workflow: request.workflow.clone(),
                lease_ttl_seconds: request.lease_ttl_seconds,
                max_attempts: request.max_attempts,
                backoff_seconds: request.backoff_seconds,
            },
        )?;
        let tick_status = tick["status"].as_str().unwrap_or("unknown").to_string();
        let idle = tick["idle"].as_bool().unwrap_or(false);
        ticks.push(tick);
        if matches!(
            tick_status.as_str(),
            "waiting" | "disabled" | "failed" | "needs_attention"
        ) {
            status = tick_status;
            break;
        }
        if idle {
            status = "idle".into();
            break;
        }
    }

    Ok(json!({
        "ok": true,
        "command": "loop run",
        "until": request.until,
        "status": status,
        "tick_count": ticks.len(),
        "ticks": ticks,
    }))
}
