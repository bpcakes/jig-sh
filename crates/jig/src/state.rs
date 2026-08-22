use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::tool_defs::tool;

use jsonl::append_jsonl;
#[cfg(test)]
use jsonl::read_jsonl;
pub(crate) use plans::{
    PlanAppendRequest, PlanCloseRequest, PlanOpenRequest, PlanStatus, ensure_plan_exists,
    ensure_plan_exists_with_cancellation, ensure_plan_is_open, open_plan_summaries,
    open_plan_summaries_with_cancellation, plan_status, plan_status_with_cancellation,
    plans_append, plans_close, plans_open_prepared, prepare_plan_open,
};
#[cfg(test)]
pub(crate) use plans::{plans_open, seed_open_plan_for_test};
pub(crate) use receipts::{
    CurrentWorktreeFingerprint, ToolReceiptStatus, WorkGateReceiptIndex, WorkReviewReceiptEvidence,
    WorkReviewReceiptStatus, current_worktree_fingerprint,
    current_worktree_fingerprint_for_receipt_with_cancellation,
    current_worktree_fingerprint_with_cancellation, work_gate_receipt_index,
    work_gate_receipt_index_with_cancellation, work_gate_receipt_indexes_with_cancellation,
};
pub(crate) use receipts::{
    ReceiptInput, ReceiptListFilter, receipts_list, record_receipt,
    record_receipt_with_cancellation,
};
pub(crate) use receipts::{StateArchiveRequest, receipts_archive, receipts_export};
use receipts::{StateToolReceipt, record_successful_state_tool};
#[cfg(test)]
pub(crate) use receipts::{
    reset_work_gate_receipt_index_scan_count, work_gate_receipt_index_scan_count,
};
use records::DecisionRecord;
#[cfg(test)]
use records::{PlanEvent, ReceiptRecord};
#[cfg(test)]
use sessions::build_summary;
pub(crate) use sessions::current_session;
pub(crate) use sessions::{
    SessionEndRequest, session_end, session_start, state_summary, state_summary_with_cancellation,
};
pub(crate) use support::now_ms;
#[cfg(test)]
use support::truncate;
use support::{ensure_state_layout, new_id};
pub(crate) use timeline::{
    DecisionStreamRecord, PlanStreamEvent, ReceiptStreamRecord, StateStreams, plan_detail_streams,
    plan_receipts, state_streams,
};

mod compression;
mod diagnostics;
mod jsonl;
mod maintenance;
mod plans;
mod receipts;
mod records;
mod session_compaction;
mod sessions;
mod support;
mod timeline;

pub(super) const MAINTENANCE_WRITER_COORDINATION_NOTE: &str = "Before applying a state rewrite, stop Jig processes launched with older runtimes that wrote through a pre-opened state-file handle. Current runtimes coordinate through the repository state lock.";

pub(crate) use diagnostics::state_diagnose;
pub(crate) use maintenance::{compact_sessions, restore_backup};

#[derive(Debug, Deserialize)]
pub(crate) struct DecisionAddRequest {
    pub(crate) title: String,
    pub(crate) selected_option: String,
    pub(crate) rationale: String,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) alternatives: Vec<String>,
    pub(crate) plan_id: Option<String>,
}

pub(crate) fn decisions_add(ctx: &RepoContext, request: DecisionAddRequest) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let record = DecisionRecord {
        id: new_id("decision"),
        session_id: current_session(ctx)?,
        plan_id: request.plan_id.clone(),
        title: request.title.clone(),
        selected_option: request.selected_option.clone(),
        rationale: request.rationale.clone(),
        alternatives: request.alternatives.clone(),
        timestamp_ms: now_ms(),
    };
    append_jsonl(&ctx.state_file("decisions.jsonl"), &record)?;

    let receipt_id = record_successful_state_tool(
        ctx,
        StateToolReceipt {
            tool_name: tool::DECISIONS_ADD,
            args: json!({
                "title": request.title,
                "selected_option": request.selected_option,
                "plan_id": request.plan_id,
            }),
            started_at_ms: record.timestamp_ms,
            plan_id: record.plan_id.clone(),
            session_override: record.session_id.clone(),
        },
    )?;

    Ok(json!({
        "ok": true,
        "decision_id": record.id,
        "receipt_id": receipt_id,
    }))
}

#[cfg(test)]
mod tests;
