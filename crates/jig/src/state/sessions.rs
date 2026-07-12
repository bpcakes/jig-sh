use std::fs;

use anyhow::{Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::tool_defs::{args, tool};

use super::events::{
    DecisionRecord, PlanEvent, ReceiptRecord, SessionEvent, append_jsonl, ensure_state_layout,
    new_id, now_ms, read_jsonl,
};
use super::plans::open_plans;
use super::receipts::{StateToolReceipt, receipt_diff_summary, record_successful_state_tool};

const STATE_SUMMARY_RECENT_LIMIT: usize = 10;

#[derive(Deserialize)]
pub(crate) struct SessionEndRequest {
    pub(crate) session_id: Option<String>,
    pub(crate) outcome: Option<String>,
}

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
    append_jsonl(&ctx.state_file("sessions.jsonl"), &event)?;
    write_current_session(ctx, Some(&session_id))?;

    let receipt_id = record_successful_state_tool(
        ctx,
        StateToolReceipt {
            tool_name: tool::SESSION_START,
            args: json!({
                args::OPERATION: "session_start",
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

pub(crate) fn session_end(ctx: &RepoContext, request: SessionEndRequest) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let session_id = match request.session_id {
        Some(id) => id,
        None => current_session(ctx)?.ok_or_else(|| anyhow!("No active session."))?,
    };
    let event = SessionEvent::end(
        new_id("session-event"),
        session_id.clone(),
        now_ms(),
        request.outcome.clone(),
    );
    append_jsonl(&ctx.state_file("sessions.jsonl"), &event)?;
    if current_session(ctx)?.as_deref() == Some(session_id.as_str()) {
        write_current_session(ctx, None)?;
    }

    let receipt_id = record_successful_state_tool(
        ctx,
        StateToolReceipt {
            tool_name: tool::SESSION_END,
            args: json!({
                args::OPERATION: "session_end",
                "session_id": session_id,
                "outcome": request.outcome,
            }),
            started_at_ms: event.timestamp_ms(),
            plan_id: None,
            session_override: Some(event.session_id().to_string()),
        },
    )?;

    Ok(json!({
        "ok": true,
        "session_id": event.session_id(),
        "receipt_id": receipt_id,
    }))
}

pub(crate) fn current_session(ctx: &RepoContext) -> Result<Option<String>> {
    let path = ctx.current_session_path();
    if !path.exists() {
        return Ok(None);
    }
    let value = fs::read_to_string(path)?.trim().to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

pub(super) fn build_summary(ctx: &RepoContext) -> Result<Value> {
    let sessions = read_jsonl::<SessionEvent>(&ctx.state_file("sessions.jsonl"))?;
    let plans = read_jsonl::<PlanEvent>(&ctx.state_file("plans.jsonl"))?;
    let receipts = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))?;
    let decisions = read_jsonl::<DecisionRecord>(&ctx.state_file("decisions.jsonl"))?;

    let open_plans = open_plans(&plans);

    let recent_receipts = receipts
        .into_iter()
        .rev()
        .take(5)
        .map(|receipt| {
            json!({
                "id": receipt.id,
                "tool_name": receipt.tool_name,
                "exit_status": receipt.exit_status,
            })
        })
        .collect::<Vec<_>>();

    let recent_decisions = decisions
        .into_iter()
        .rev()
        .take(5)
        .map(|decision| {
            json!({
                "id": decision.id,
                "title": decision.title,
                "selected_option": decision.selected_option,
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "repo_name": ctx.repo_name(),
        "default_branch": ctx.default_branch(),
        "source_commit": ctx.source_commit(),
        "source_path": ctx.source_path(),
        "recent_sessions": sessions.into_iter().rev().take(3).collect::<Vec<_>>(),
        "open_plans": open_plans,
        "recent_receipts": recent_receipts,
        "recent_decisions": recent_decisions,
    }))
}

pub(crate) fn state_summary(ctx: &RepoContext) -> Result<Value> {
    let sessions = read_jsonl::<SessionEvent>(&ctx.state_file("sessions.jsonl"))?;
    let plans = read_jsonl::<PlanEvent>(&ctx.state_file("plans.jsonl"))?;
    let receipts = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))?;
    let decisions = read_jsonl::<DecisionRecord>(&ctx.state_file("decisions.jsonl"))?;

    let open_plans = open_plans(&plans);
    let session_count = sessions.iter().filter(|session| session.is_start()).count();
    let plan_count = plans.iter().filter(|plan| plan.is_open()).count();
    let failed_receipts = receipts
        .iter()
        .filter(|receipt| receipt.exit_status != 0)
        .count();
    let recent_receipts = receipts
        .iter()
        .rev()
        .take(STATE_SUMMARY_RECENT_LIMIT)
        .map(receipt_summary)
        .collect::<Vec<_>>();
    let recent_decisions = decisions
        .iter()
        .rev()
        .take(STATE_SUMMARY_RECENT_LIMIT)
        .map(decision_summary)
        .collect::<Vec<_>>();

    Ok(json!({
        "ok": true,
        "repo": {
            "name": ctx.repo_name(),
            "default_branch": ctx.default_branch(),
            "source_commit": ctx.source_commit(),
            "source_path": ctx.source_path(),
        },
        "current_session_id": current_session(ctx)?,
        "counts": {
            "sessions": session_count,
            "session_events": sessions.len(),
            "plans": plan_count,
            "plan_events": plans.len(),
            "open_plans": open_plans.len(),
            "receipts": receipts.len(),
            "failed_receipts": failed_receipts,
            "decisions": decisions.len(),
        },
        "open_plans": open_plans,
        "recent_receipts": recent_receipts,
        "recent_decisions": recent_decisions,
    }))
}

fn receipt_summary(receipt: &ReceiptRecord) -> Value {
    json!({
        "id": receipt.id,
        "session_id": receipt.session_id,
        "plan_id": receipt.plan_id,
        "tool_name": receipt.tool_name,
        "invoked_command_key": receipt.invoked_command_key,
        "exit_status": receipt.exit_status,
        "started_at_ms": receipt.started_at_ms,
        "ended_at_ms": receipt.ended_at_ms,
        "diff_summary": receipt_diff_summary(receipt),
    })
}

fn decision_summary(decision: &DecisionRecord) -> Value {
    json!({
        "id": decision.id,
        "title": decision.title,
        "selected_option": decision.selected_option,
        "plan_id": decision.plan_id,
        "session_id": decision.session_id,
        "timestamp_ms": decision.timestamp_ms,
    })
}

fn write_current_session(ctx: &RepoContext, session_id: Option<&str>) -> Result<()> {
    let path = ctx.current_session_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match session_id {
        Some(value) => fs::write(path, format!("{value}\n"))?,
        None => {
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}
