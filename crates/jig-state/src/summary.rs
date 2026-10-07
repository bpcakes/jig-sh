//! The persisted-state summary reported by `jig state summary`.

use std::collections::VecDeque;
use std::path::Path;

use anyhow::{Context, Result};
use jig_context::RepoContext;
use serde_json::{Value, json};

use crate::cancellation::ensure_status_collection_active;

use super::jsonl::{scan_dashboard_jsonl_raw, scan_jsonl_raw};
use super::privacy::{redact_repository_root, repository_root_spellings};
use super::runs::{CompletedTargetEvent, RunHistoryEvent, run_history_event};

const STATE_SUMMARY_RECENT_LIMIT: usize = 10;

fn public_source_path(ctx: &RepoContext) -> String {
    redact_repository_root(ctx.source_path(), &repository_root_spellings(ctx.root()))
}

#[cfg(test)]
pub fn state_summary(ctx: &RepoContext) -> Result<Value> {
    state_summary_impl(ctx, &|| false, false)
}

pub fn state_summary_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    state_summary_impl(ctx, cancelled, true)
}

fn state_summary_impl(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
    bounded: bool,
) -> Result<Value> {
    ensure_status_collection_active(cancelled)?;
    let runs = summarize_runs(
        &ctx.state_file("runs.jsonl"),
        STATE_SUMMARY_RECENT_LIMIT,
        cancelled,
        bounded,
    )?;
    ensure_status_collection_active(cancelled)?;

    Ok(json!({
        "ok": true,
        "repo": {
            "name": ctx.repo_name(),
            "default_branch": ctx.default_branch(),
            "source_commit": ctx.source_commit(),
            "source_path": public_source_path(ctx),
        },
        "counts": {
            "runs": runs.runs,
            "target_results": runs.target_results,
            "failed_target_results": runs.failed,
        },
        "recent_target_results": runs.recent,
    }))
}

struct RunHistorySummary {
    runs: usize,
    target_results: usize,
    failed: usize,
    recent: Vec<Value>,
}

fn summarize_runs(
    path: &Path,
    limit: usize,
    cancelled: &dyn Fn() -> bool,
    bounded: bool,
) -> Result<RunHistorySummary> {
    let mut runs = 0usize;
    let mut target_results = 0usize;
    let mut failed = 0usize;
    let mut recent = VecDeque::with_capacity(limit);
    let mut visit = |record: super::jsonl::RawJsonlRecord<'_>| {
        let event = run_history_event(record.bytes).with_context(|| {
            format!(
                "Failed to parse run record {} in {}",
                record.line_number,
                path.display()
            )
        })?;
        match event {
            RunHistoryEvent::Queued => runs = runs.saturating_add(1),
            RunHistoryEvent::TargetCompleted(event) => {
                target_results = target_results.saturating_add(1);
                failed = failed.saturating_add(usize::from(event.failed()));
                if limit > 0 {
                    if recent.len() == limit {
                        recent.pop_front();
                    }
                    recent.push_back(target_result_summary(*event));
                }
            }
            RunHistoryEvent::Other => {}
        }
        Ok(())
    };
    if bounded {
        scan_dashboard_jsonl_raw(path, cancelled, &mut visit)?;
    } else {
        scan_jsonl_raw(path, cancelled, &mut visit)?;
    }
    Ok(RunHistorySummary {
        runs,
        target_results,
        failed,
        recent: recent.into_iter().rev().collect(),
    })
}

fn target_result_summary(event: CompletedTargetEvent) -> Value {
    let CompletedTargetEvent { run_id, result } = event;
    json!({
        "run_id": run_id,
        "target": result.target,
        "status": result.status,
        "conclusion": result.conclusion,
        "exit_code": result.exit_code,
        "started_at_ms": result.started_at_ms,
        "ended_at_ms": result.ended_at_ms,
    })
}
