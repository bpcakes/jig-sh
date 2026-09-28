//! Test fixtures that record session events the way the removed `jig work`
//! lifecycle did, for tests of the remaining session-state readers.

use anyhow::Result;
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::tool_defs::tool;

use super::super::jsonl::append_jsonl;
use super::super::receipts::{StateToolReceipt, record_successful_state_tool};
use super::super::records::SessionEvent;
use super::super::session_pointer::with_write_lock;
use super::super::session_pointer::write_locked as write_current_session_locked;
use super::super::support::{ensure_state_layout, new_id, now_ms};
use super::build_summary;

pub(crate) fn session_start(ctx: &RepoContext) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let session_id = new_id("session");
    let summary = build_summary(ctx)?;
    let event = SessionEvent::start(
        new_id("session-event"),
        session_id.clone(),
        now_ms(),
        summary.clone(),
    );
    with_write_lock(ctx, || {
        append_jsonl(&ctx.state_file("sessions.jsonl"), &event)?;
        write_current_session_locked(ctx, Some(&session_id))
    })?;

    let receipt_id = record_successful_state_tool(
        ctx,
        StateToolReceipt {
            tool_name: tool::SESSION_START,
            args: json!({
                "operation": "session_start",
            }),
            started_at_ms: event.timestamp_ms(),
            plan_id: None,
            session_override: Some(session_id.clone()),
        },
    )?;

    Ok(json!({
        "ok": true,
        "session_id": session_id,
        "summary": summary,
        "receipt_id": receipt_id,
    }))
}
