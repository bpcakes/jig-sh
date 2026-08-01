use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cancellation::ensure_status_collection_active;
use crate::context::RepoContext;
use crate::tool_defs::{args, tool};

use super::jsonl::{append_jsonl, read_jsonl, read_jsonl_with_cancellation, scan_jsonl_raw};
use super::plans::open_plans;
use super::receipts::{StateToolReceipt, receipt_diff_summary, record_successful_state_tool};
use super::records::{
    DecisionRecord, PlanEvent, ReceiptRecord, SessionEvent, SessionEventEnvelope,
};
use super::support::{ensure_state_layout, new_id, now_ms};

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

pub(super) fn read_session_events(path: &Path) -> Result<Vec<SessionEvent>> {
    read_session_events_with_cancellation(path, &|| false)
}

pub(super) fn read_session_events_with_cancellation(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<SessionEvent>> {
    let mut canonical = HashMap::<String, (SessionEventEnvelope, u64)>::new();
    scan_jsonl_raw(path, cancelled, |record| {
        let envelope =
            serde_json::from_slice::<SessionEventEnvelope>(record.bytes).with_context(|| {
                format!(
                    "Failed to parse session event envelope at JSONL record {} in {}",
                    record.line_number,
                    path.display()
                )
            })?;
        match canonical.get(&envelope.id) {
            Some((existing, _)) if existing == &envelope => {}
            Some((_, first_line)) => {
                bail!(
                    "Conflicting session event envelope for ID `{}` at JSONL records {} and {} in {}",
                    envelope.id,
                    first_line,
                    record.line_number,
                    path.display()
                );
            }
            None => {
                canonical.insert(envelope.id.clone(), (envelope, record.line_number));
            }
        }
        Ok(())
    })?;

    let mut canonical = canonical
        .into_values()
        .map(|(event, _)| event)
        .collect::<Vec<_>>();
    canonical.sort_by(|left, right| {
        left.timestamp_ms
            .cmp(&right.timestamp_ms)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(canonical
        .into_iter()
        .map(SessionEventEnvelope::into_event)
        .collect())
}

pub(super) fn build_summary(ctx: &RepoContext) -> Result<Value> {
    let sessions = read_session_events(&ctx.state_file("sessions.jsonl"))?;
    let plans = read_jsonl::<PlanEvent>(&ctx.state_file("plans.jsonl"))?;
    let receipts = summarize_receipts(&ctx.state_file("receipts.jsonl"), 5, &|| false)?;
    let decisions = summarize_decisions(&ctx.state_file("decisions.jsonl"), 5, &|| false)?;

    let open_plans = open_plans(&plans);

    let recent_receipts = receipts
        .recent
        .into_iter()
        .map(|receipt| {
            json!({
                "id": receipt["id"],
                "tool_name": receipt["tool_name"],
                "exit_status": receipt["exit_status"],
            })
        })
        .collect::<Vec<_>>();

    let recent_decisions = decisions
        .recent
        .into_iter()
        .map(|decision| {
            json!({
                "id": decision["id"],
                "title": decision["title"],
                "selected_option": decision["selected_option"],
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "repo_name": ctx.repo_name(),
        "default_branch": ctx.default_branch(),
        "source_commit": ctx.source_commit(),
        "source_path": ctx.source_path(),
        "recent_sessions": sessions
            .into_iter()
            .rev()
            .take(3)
            .map(SessionEvent::into_summary_reference)
            .collect::<Vec<_>>(),
        "open_plans": open_plans,
        "recent_receipts": recent_receipts,
        "recent_decisions": recent_decisions,
    }))
}

pub(crate) fn state_summary(ctx: &RepoContext) -> Result<Value> {
    state_summary_with_cancellation(ctx, &|| false)
}

pub(crate) fn state_summary_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    ensure_state_summary_active(cancelled)?;
    let sessions =
        read_session_events_with_cancellation(&ctx.state_file("sessions.jsonl"), cancelled)?;
    ensure_state_summary_active(cancelled)?;
    let plans =
        read_jsonl_with_cancellation::<PlanEvent>(&ctx.state_file("plans.jsonl"), cancelled)?;
    ensure_state_summary_active(cancelled)?;
    let receipts = summarize_receipts(
        &ctx.state_file("receipts.jsonl"),
        STATE_SUMMARY_RECENT_LIMIT,
        cancelled,
    )?;
    ensure_state_summary_active(cancelled)?;
    let decisions = summarize_decisions(
        &ctx.state_file("decisions.jsonl"),
        STATE_SUMMARY_RECENT_LIMIT,
        cancelled,
    )?;
    ensure_state_summary_active(cancelled)?;

    let open_plans = open_plans(&plans);
    let session_count = sessions.iter().filter(|session| session.is_start()).count();
    let plan_count = plans.iter().filter(|plan| plan.is_open()).count();
    ensure_state_summary_active(cancelled)?;
    let current_session_id = current_session(ctx)?;
    ensure_state_summary_active(cancelled)?;

    Ok(json!({
        "ok": true,
        "repo": {
            "name": ctx.repo_name(),
            "default_branch": ctx.default_branch(),
            "source_commit": ctx.source_commit(),
            "source_path": ctx.source_path(),
        },
        "current_session_id": current_session_id,
        "counts": {
            "sessions": session_count,
            "session_events": sessions.len(),
            "plans": plan_count,
            "plan_events": plans.len(),
            "open_plans": open_plans.len(),
            "receipts": receipts.count,
            "failed_receipts": receipts.failed,
            "decisions": decisions.count,
        },
        "open_plans": open_plans,
        "recent_receipts": receipts.recent,
        "recent_decisions": decisions.recent,
    }))
}

struct ReceiptStreamSummary {
    count: usize,
    failed: usize,
    recent: Vec<Value>,
}

fn summarize_receipts(
    path: &Path,
    limit: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<ReceiptStreamSummary> {
    let mut count = 0usize;
    let mut failed = 0usize;
    let mut recent = VecDeque::with_capacity(limit);
    scan_jsonl_raw(path, cancelled, |record| {
        let receipt = serde_json::from_slice::<ReceiptRecord>(record.bytes).with_context(|| {
            format!(
                "Failed to parse receipt JSONL record {} in {}",
                record.line_number,
                path.display()
            )
        })?;
        count = count.saturating_add(1);
        failed = failed.saturating_add(usize::from(receipt.exit_status != 0));
        push_recent(&mut recent, limit, receipt_summary(&receipt));
        Ok(())
    })?;
    Ok(ReceiptStreamSummary {
        count,
        failed,
        recent: recent.into_iter().rev().collect(),
    })
}

struct DecisionStreamSummary {
    count: usize,
    recent: Vec<Value>,
}

fn summarize_decisions(
    path: &Path,
    limit: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<DecisionStreamSummary> {
    let mut count = 0usize;
    let mut recent = VecDeque::with_capacity(limit);
    scan_jsonl_raw(path, cancelled, |record| {
        let decision =
            serde_json::from_slice::<DecisionRecord>(record.bytes).with_context(|| {
                format!(
                    "Failed to parse decision JSONL record {} in {}",
                    record.line_number,
                    path.display()
                )
            })?;
        count = count.saturating_add(1);
        push_recent(&mut recent, limit, decision_summary(&decision));
        Ok(())
    })?;
    Ok(DecisionStreamSummary {
        count,
        recent: recent.into_iter().rev().collect(),
    })
}

fn push_recent(recent: &mut VecDeque<Value>, limit: usize, value: Value) {
    if limit == 0 {
        return;
    }
    if recent.len() == limit {
        recent.pop_front();
    }
    recent.push_back(value);
}

fn ensure_state_summary_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
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

#[cfg(test)]
mod canonical_session_tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn identical_envelopes_collapse_and_queries_sort_by_timestamp_then_id() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("sessions.jsonl");
        let records = [
            json!({
                "id": "event-b",
                "session_id": "session-b",
                "event": "end",
                "timestamp_ms": 1,
                "outcome": "done"
            }),
            json!({
                "id": "event-z",
                "session_id": "session-z",
                "event": "start",
                "timestamp_ms": 2,
                "outcome": null,
                "summary": {"recent_sessions": [{"summary": {"legacy": true}}]}
            }),
            json!({
                "id": "event-a",
                "session_id": "session-a",
                "event": "start",
                "timestamp_ms": 1,
                "outcome": null,
                "summary": null
            }),
            json!({
                "id": "event-z",
                "session_id": "session-z",
                "event": "start",
                "timestamp_ms": 2,
                "outcome": null,
                "summary": null
            }),
        ];
        let source = records
            .iter()
            .map(|record| format!("{}\n", serde_json::to_string(record).unwrap()))
            .collect::<String>();
        fs::write(&path, source).unwrap();

        let events = read_session_events(&path).unwrap();
        let ids = events
            .into_iter()
            .map(|event| {
                serde_json::to_value(event).unwrap()["id"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect::<Vec<_>>();

        assert_eq!(ids, ["event-a", "event-b", "event-z"]);
    }

    #[test]
    fn every_conflicting_envelope_field_is_rejected_with_the_event_id() {
        let base = json!({
            "id": "same-id",
            "session_id": "session-a",
            "event": "end",
            "timestamp_ms": 1,
            "outcome": "done",
            "summary": null
        });
        let conflicts = [
            json!({
                "id": "same-id",
                "session_id": "session-b",
                "event": "end",
                "timestamp_ms": 1,
                "outcome": "done"
            }),
            json!({
                "id": "same-id",
                "session_id": "session-a",
                "event": "start",
                "timestamp_ms": 1,
                "outcome": "done"
            }),
            json!({
                "id": "same-id",
                "session_id": "session-a",
                "event": "end",
                "timestamp_ms": 2,
                "outcome": "done"
            }),
            json!({
                "id": "same-id",
                "session_id": "session-a",
                "event": "end",
                "timestamp_ms": 1,
                "outcome": "failed"
            }),
        ];

        for conflict in conflicts {
            let temp = tempdir().unwrap();
            let path = temp.path().join("sessions.jsonl");
            fs::write(
                &path,
                format!(
                    "{}\n{}\n",
                    serde_json::to_string(&base).unwrap(),
                    serde_json::to_string(&conflict).unwrap()
                ),
            )
            .unwrap();

            let error = read_session_events(&path).unwrap_err().to_string();
            assert!(error.contains("Conflicting session event envelope"));
            assert!(error.contains("same-id"));
            assert!(error.contains("records 1 and 2"));
        }
    }

    #[test]
    fn git_union_merge_duplicates_collapse_to_canonical_session_events() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "jig-test@example.invalid"]);
        git(root, &["config", "user.name", "Jig Test"]);
        fs::write(root.join(".gitattributes"), "sessions.jsonl merge=union\n").unwrap();
        let legacy = session_line("event-a", "session-a", 1, json!({"legacy": true}));
        fs::write(root.join("sessions.jsonl"), format!("{legacy}\n")).unwrap();
        git(root, &["add", ".gitattributes", "sessions.jsonl"]);
        git(root, &["commit", "-q", "-m", "base"]);
        let primary = git_output(root, &["branch", "--show-current"]);
        git(root, &["branch", "stale"]);

        let compact = session_line("event-a", "session-a", 1, Value::Null);
        fs::write(root.join("sessions.jsonl"), format!("{compact}\n")).unwrap();
        git(root, &["add", "sessions.jsonl"]);
        git(root, &["commit", "-q", "-m", "compact"]);

        git(root, &["checkout", "-q", "stale"]);
        let stale_append = session_line("event-b", "session-b", 2, Value::Null);
        fs::write(
            root.join("sessions.jsonl"),
            format!("{legacy}\n{stale_append}\n"),
        )
        .unwrap();
        git(root, &["add", "sessions.jsonl"]);
        git(root, &["commit", "-q", "-m", "stale append"]);

        git(root, &["checkout", "-q", primary.trim()]);
        git(root, &["merge", "-q", "--no-edit", "stale"]);

        let merged = fs::read_to_string(root.join("sessions.jsonl")).unwrap();
        assert!(
            merged.lines().count() >= 3,
            "union merge did not retain both physical event-a variants:\n{merged}"
        );
        let events = read_session_events(&root.join("sessions.jsonl")).unwrap();
        let ids = events
            .into_iter()
            .map(|event| {
                serde_json::to_value(event).unwrap()["id"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, ["event-a", "event-b"]);
    }

    fn session_line(id: &str, session_id: &str, timestamp_ms: u64, summary: Value) -> String {
        serde_json::to_string(&json!({
            "id": id,
            "session_id": session_id,
            "event": "start",
            "timestamp_ms": timestamp_ms,
            "outcome": null,
            "summary": summary,
        }))
        .unwrap()
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed:\nstdout: {}\nstderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_output(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed:\nstdout: {}\nstderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}
