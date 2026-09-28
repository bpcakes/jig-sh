//! The persisted-state summary reported by `jig state summary`.

use std::collections::VecDeque;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::cancellation::ensure_status_collection_active;
use crate::context::RepoContext;

use super::jsonl::{scan_dashboard_jsonl_raw, scan_jsonl_raw};
use super::privacy::{redact_repository_root, repository_root_spellings};
use super::receipts::receipt_diff_summary;
use super::records::ReceiptRecord;

const STATE_SUMMARY_RECENT_LIMIT: usize = 10;

fn public_source_path(ctx: &RepoContext) -> String {
    redact_repository_root(ctx.source_path(), &repository_root_spellings(ctx.root()))
}

#[cfg(test)]
pub(crate) fn state_summary(ctx: &RepoContext) -> Result<Value> {
    state_summary_impl(ctx, &|| false, false)
}

pub(crate) fn state_summary_with_cancellation(
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
    let receipts = summarize_receipts(
        &ctx.state_file("receipts.jsonl"),
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
            "receipts": receipts.count,
            "failed_receipts": receipts.failed,
        },
        "recent_receipts": receipts.recent,
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
    bounded: bool,
) -> Result<ReceiptStreamSummary> {
    let mut count = 0usize;
    let mut failed = 0usize;
    let mut recent = VecDeque::with_capacity(limit);
    let mut visit = |record: super::jsonl::RawJsonlRecord<'_>| {
        let receipt = serde_json::from_slice::<ReceiptRecord>(record.bytes).with_context(|| {
            format!(
                "Failed to parse receipt JSONL record {} in {}",
                record.line_number,
                path.display()
            )
        })?;
        count = count.saturating_add(1);
        failed = failed.saturating_add(usize::from(receipt.exit_status != 0));
        if limit > 0 {
            if recent.len() == limit {
                recent.pop_front();
            }
            recent.push_back(receipt_summary(&receipt));
        }
        Ok(())
    };
    if bounded {
        scan_dashboard_jsonl_raw(path, cancelled, &mut visit)?;
    } else {
        scan_jsonl_raw(path, cancelled, &mut visit)?;
    }
    Ok(ReceiptStreamSummary {
        count,
        failed,
        recent: recent.into_iter().rev().collect(),
    })
}

fn receipt_summary(receipt: &ReceiptRecord) -> Value {
    json!({
        "id": receipt.id,
        "tool_name": receipt.tool_name,
        "invoked_command_key": receipt.invoked_command_key,
        "exit_status": receipt.exit_status,
        "started_at_ms": receipt.started_at_ms,
        "ended_at_ms": receipt.ended_at_ms,
        "diff_summary": receipt_diff_summary(receipt),
    })
}
