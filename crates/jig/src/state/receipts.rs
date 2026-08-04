#[cfg(test)]
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cancellation::{ensure_status_collection_active, status_collection_cancellation};
use crate::context::RepoContext;
use crate::git_receipts::{
    GitReceiptMetadata, collect_git_receipt_metadata,
    collect_git_receipt_metadata_without_worktree_fingerprint,
    is_worktree_fingerprint_cancellation, repo_worktree_fingerprint,
    repo_worktree_fingerprint_with_cancellation,
};
use crate::tool_defs::tool;

use super::jsonl::{RawJsonlRecord, append_jsonl, read_receipts_reverse, scan_jsonl_raw};
use super::records::ReceiptRecord;
use super::sessions::current_session;
use super::support::{ensure_state_layout, new_id, now_ms, truncate};

mod archive;

use archive::refuse_unterminated_receipt_stream;
#[cfg(test)]
use archive::{ReceiptProtectionIndex, sha256_reader, write_receipt_gzip};
pub(crate) use archive::{StateArchiveRequest, receipts_archive, receipts_export};

const SUCCESSFUL_RECEIPT_PREVIEW_BYTES: usize = 512;

pub(crate) struct ReceiptInput<'a> {
    pub(crate) tool_name: &'a str,
    pub(crate) args: Value,
    pub(crate) invoked_command_key: Option<String>,
    pub(crate) plan_id: Option<String>,
    pub(crate) started_at_ms: u64,
    pub(crate) ended_at_ms: u64,
    pub(crate) exit_status: i32,
    pub(crate) stdout: &'a str,
    pub(crate) stderr: &'a str,
    pub(crate) evidence: Option<Value>,
    pub(crate) session_override: Option<String>,
    pub(crate) collect_git_metadata: bool,
    pub(crate) collect_worktree_fingerprint: bool,
    pub(crate) worktree_fingerprint_override: Option<std::result::Result<String, String>>,
}

pub(super) struct StateToolReceipt<'a> {
    pub(super) tool_name: &'a str,
    pub(super) args: Value,
    pub(super) started_at_ms: u64,
    pub(super) plan_id: Option<String>,
    pub(super) session_override: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReceiptListFilter {
    pub(crate) session_id: Option<String>,
    pub(crate) plan_id: Option<String>,
    pub(crate) tool_name: Option<String>,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) failed_only: bool,
    // `usize::default()` is 0, but a null receipt limit should keep the
    // public default instead of asking for zero rows.
    #[serde(
        default = "crate::serde_helpers::default_receipts_limit",
        deserialize_with = "crate::serde_helpers::null_as_default_receipts_limit"
    )]
    pub(crate) limit: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct ToolReceiptStatus {
    pub(crate) receipt_id: String,
    pub(crate) exit_status: i32,
    pub(crate) ended_at_ms: u64,
    pub(crate) changed_paths: Vec<String>,
    pub(crate) changed_path_count: usize,
    pub(crate) changed_paths_truncated: bool,
    pub(crate) changed_paths_digest: Option<String>,
    pub(crate) diff_summary: String,
    pub(crate) worktree_fingerprint: Option<String>,
    pub(crate) worktree_fingerprint_error: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkReviewReceiptStatus {
    pub(crate) receipt_id: String,
    pub(crate) exit_status: i32,
    pub(crate) ended_at_ms: u64,
    pub(crate) evidence: Option<WorkReviewReceiptEvidence>,
    pub(crate) changed_paths: Vec<String>,
    pub(crate) changed_path_count: usize,
    pub(crate) changed_paths_truncated: bool,
    pub(crate) changed_paths_digest: Option<String>,
    pub(crate) diff_summary: String,
    pub(crate) worktree_fingerprint: Option<String>,
    pub(crate) worktree_fingerprint_error: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkReviewReceiptEvidence {
    pub(crate) status: Option<String>,
    pub(crate) finding_count: Option<u64>,
    pub(crate) actionable_count: Option<u64>,
    pub(crate) retained_finding_count: Option<usize>,
    pub(crate) retained_actionable_count: Option<usize>,
    pub(crate) findings_truncated: Option<bool>,
    pub(crate) actionable_findings_truncated: Option<bool>,
    pub(crate) threshold: Option<String>,
    pub(crate) parse_error: Option<String>,
}

#[derive(Debug, Default)]
struct IndexedCheckReceipts {
    direct: Option<ToolReceiptStatus>,
    exact_work_check: Option<ToolReceiptStatus>,
    legacy_work_check: Option<ToolReceiptStatus>,
}

/// A request-scoped view of the receipts needed to evaluate configured work
/// gates. Building it retains a bounded number of statuses per configured gate
/// while scanning the receipt stream exactly once.
#[derive(Debug, Default)]
pub(crate) struct WorkGateReceiptIndex {
    checks: BTreeMap<String, IndexedCheckReceipts>,
    reviews: BTreeMap<String, WorkReviewReceiptStatus>,
}

impl WorkGateReceiptIndex {
    pub(crate) fn tool_receipt(&self, tool_name: &str) -> Option<&ToolReceiptStatus> {
        self.checks
            .get(tool_name)
            .and_then(|receipts| receipts.direct.as_ref())
    }

    pub(crate) fn work_check_receipt(
        &self,
        tool_name: &str,
        tool_receipt_id: &str,
    ) -> Option<&ToolReceiptStatus> {
        let receipts = self.checks.get(tool_name)?;
        if receipts.direct.as_ref()?.receipt_id != tool_receipt_id {
            return None;
        }
        receipts
            .exact_work_check
            .as_ref()
            .or(receipts.legacy_work_check.as_ref())
    }

    pub(crate) fn review_receipt(&self, gate_id: &str) -> Option<&WorkReviewReceiptStatus> {
        self.reviews.get(gate_id)
    }
}

#[cfg(test)]
thread_local! {
    static WORK_GATE_RECEIPT_INDEX_SCAN_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_work_gate_receipt_index_scan_count() {
    WORK_GATE_RECEIPT_INDEX_SCAN_COUNT.set(0);
}

#[cfg(test)]
pub(crate) fn work_gate_receipt_index_scan_count() -> usize {
    WORK_GATE_RECEIPT_INDEX_SCAN_COUNT.get()
}

#[derive(Clone, Debug)]
pub(crate) struct CurrentWorktreeFingerprint {
    pub(crate) fingerprint: Option<String>,
    pub(crate) error: Option<String>,
}

pub(crate) fn receipts_list(ctx: &RepoContext, filter: ReceiptListFilter) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let (receipts, _) =
        read_receipts_reverse(&ctx.state_file("receipts.jsonl"), filter.limit, |receipt| {
            receipt_matches_filters(receipt, &filter)
        })?;
    let receipts = receipts
        .into_iter()
        .map(receipt_list_value)
        .collect::<Result<Vec<_>>>()?;

    Ok(json!({
        "ok": true,
        "receipts": receipts,
    }))
}

fn parse_raw_receipt(record: RawJsonlRecord<'_>, path: &Path) -> Result<ReceiptRecord> {
    serde_json::from_slice(record.bytes).with_context(|| {
        format!(
            "Failed to parse receipt record {} in {}",
            record.line_number,
            path.display()
        )
    })
}

pub(super) fn validate_receipt_stream(path: &Path) -> Result<()> {
    let scan = scan_jsonl_raw(path, &|| false, |record| {
        parse_raw_receipt(record, path).map(|_| ())
    })?;
    refuse_unterminated_receipt_stream(path, scan.unterminated_final_record)
}

pub(crate) fn work_gate_receipt_index(
    ctx: &RepoContext,
    plan_id: &str,
    check_tools: &BTreeSet<String>,
    review_gate_ids: &BTreeSet<String>,
) -> Result<WorkGateReceiptIndex> {
    work_gate_receipt_index_with_cancellation(ctx, plan_id, check_tools, review_gate_ids, &|| false)
}

pub(crate) fn work_gate_receipt_index_with_cancellation(
    ctx: &RepoContext,
    plan_id: &str,
    check_tools: &BTreeSet<String>,
    review_gate_ids: &BTreeSet<String>,
    cancelled: &dyn Fn() -> bool,
) -> Result<WorkGateReceiptIndex> {
    ensure_receipt_scan_active(cancelled)?;
    ensure_state_layout(ctx)?;

    let mut index = WorkGateReceiptIndex {
        checks: check_tools
            .iter()
            .map(|tool_name| (tool_name.clone(), IndexedCheckReceipts::default()))
            .collect(),
        reviews: BTreeMap::new(),
    };
    if check_tools.is_empty() && review_gate_ids.is_empty() {
        return Ok(index);
    }

    let path = ctx.state_file("receipts.jsonl");
    #[cfg(test)]
    WORK_GATE_RECEIPT_INDEX_SCAN_COUNT
        .set(WORK_GATE_RECEIPT_INDEX_SCAN_COUNT.get().saturating_add(1));
    scan_jsonl_raw(&path, cancelled, |record| {
        ensure_receipt_scan_active(cancelled)?;
        let receipt = parse_raw_receipt(record, &path)?;
        if receipt.plan_id.as_deref() != Some(plan_id) {
            return Ok(());
        }

        let direct_tool_name = index
            .checks
            .contains_key(&receipt.tool_name)
            .then(|| receipt.tool_name.clone());
        if let Some(tool_name) = direct_tool_name.as_deref() {
            let receipts = index
                .checks
                .get_mut(tool_name)
                .expect("configured check tool should be indexed");
            receipts.direct = Some(tool_receipt_status(receipt.clone()));
            // A batch can only provide freshness for the latest direct
            // receipt when it appears physically after that receipt.
            receipts.exact_work_check = None;
            receipts.legacy_work_check = None;
        }

        if receipt.tool_name == tool::WORK_CHECK && receipt.exit_status == 0 {
            let batch_status = tool_receipt_status(receipt.clone());
            let has_receipt_ids = receipt_args_has_receipt_ids(&receipt);
            for tool_name in receipt_arg_strings(&receipt, "tools") {
                // If jig.work_check itself is configured as a check gate, the
                // receipt that becomes the direct anchor is not its own batch.
                if direct_tool_name.as_deref() == Some(tool_name) {
                    continue;
                }
                let Some(receipts) = index.checks.get_mut(tool_name) else {
                    continue;
                };
                let Some(direct) = receipts.direct.as_ref() else {
                    continue;
                };
                if receipt_args_include_receipt_id(&receipt, &direct.receipt_id) {
                    receipts.exact_work_check = Some(batch_status.clone());
                } else if !has_receipt_ids {
                    receipts.legacy_work_check = Some(batch_status.clone());
                }
            }
        }

        if receipt.tool_name == tool::WORK_REVIEW {
            if let Some(gate_id) = receipt
                .args
                .get("gate_id")
                .and_then(Value::as_str)
                .filter(|gate_id| review_gate_ids.contains(*gate_id))
            {
                index
                    .reviews
                    .insert(gate_id.to_string(), work_review_receipt_status(receipt));
            }
        }
        Ok(())
    })?;
    ensure_receipt_scan_active(cancelled)?;
    Ok(index)
}

fn receipt_arg_strings<'a>(receipt: &'a ReceiptRecord, key: &str) -> Vec<&'a str> {
    receipt
        .args
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn ensure_receipt_scan_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
}

pub(crate) fn current_worktree_fingerprint(ctx: &RepoContext) -> CurrentWorktreeFingerprint {
    current_worktree_fingerprint_from_result(repo_worktree_fingerprint(ctx.root()))
        .expect("blocking worktree fingerprint collection cannot be cancelled")
}

pub(crate) fn current_worktree_fingerprint_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<CurrentWorktreeFingerprint> {
    current_worktree_fingerprint_from_result(repo_worktree_fingerprint_with_cancellation(
        ctx.root(),
        cancelled,
    ))
}

fn current_worktree_fingerprint_from_result(
    result: Result<String>,
) -> Result<CurrentWorktreeFingerprint> {
    match result {
        Ok(fingerprint) => Ok(CurrentWorktreeFingerprint {
            fingerprint: Some(fingerprint),
            error: None,
        }),
        Err(error) if is_worktree_fingerprint_cancellation(&error) => {
            Err(status_collection_cancellation())
        }
        Err(error) => Ok(CurrentWorktreeFingerprint {
            fingerprint: None,
            error: Some(format!("{error:#}")),
        }),
    }
}

pub(crate) fn record_receipt(ctx: &RepoContext, input: ReceiptInput<'_>) -> Result<String> {
    ensure_state_layout(ctx)?;
    let mut git_metadata = receipt_git_metadata(
        ctx,
        input.collect_git_metadata,
        input.collect_worktree_fingerprint,
    );
    if let Some(override_result) = input.worktree_fingerprint_override {
        match override_result {
            Ok(fingerprint) => {
                git_metadata.worktree_fingerprint = Some(fingerprint);
                git_metadata.worktree_fingerprint_error = None;
            }
            Err(error) => {
                git_metadata.worktree_fingerprint = None;
                git_metadata.worktree_fingerprint_error = Some(error);
            }
        }
    }
    let receipt = ReceiptRecord {
        id: new_id("receipt"),
        session_id: match input.session_override {
            Some(session_id) => Some(session_id),
            None => current_session(ctx)?,
        },
        plan_id: input.plan_id,
        tool_name: input.tool_name.to_string(),
        args: input.args,
        invoked_command_key: input.invoked_command_key,
        started_at_ms: input.started_at_ms,
        ended_at_ms: input.ended_at_ms,
        exit_status: input.exit_status,
        stdout_preview: receipt_output_preview(input.stdout, input.exit_status),
        stderr_preview: receipt_output_preview(input.stderr, input.exit_status),
        evidence: input.evidence,
        changed_paths: git_metadata.changed_paths,
        changed_path_count: git_metadata.changed_path_count,
        changed_paths_truncated: git_metadata.changed_paths_truncated,
        changed_paths_digest: git_metadata.changed_paths_digest,
        diff_stat: git_metadata.diff_stat,
        git_status_error: git_metadata.git_status_error,
        git_diff_stat_error: git_metadata.git_diff_stat_error,
        worktree_fingerprint: git_metadata.worktree_fingerprint,
        worktree_fingerprint_error: git_metadata.worktree_fingerprint_error,
    };
    let receipt_id = receipt.id.clone();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &receipt)?;
    Ok(receipt_id)
}

pub(super) fn record_successful_state_tool(
    ctx: &RepoContext,
    input: StateToolReceipt<'_>,
) -> Result<String> {
    record_receipt(
        ctx,
        ReceiptInput {
            tool_name: input.tool_name,
            args: input.args,
            invoked_command_key: None,
            plan_id: input.plan_id,
            started_at_ms: input.started_at_ms,
            ended_at_ms: now_ms(),
            exit_status: 0,
            stdout: "",
            stderr: "",
            evidence: None,
            session_override: input.session_override,
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: None,
        },
    )
}

fn receipt_matches_filters(receipt: &ReceiptRecord, filter: &ReceiptListFilter) -> bool {
    let session_matches = filter
        .session_id
        .as_ref()
        .is_none_or(|session_id| receipt.session_id.as_ref() == Some(session_id));
    let plan_matches = filter
        .plan_id
        .as_ref()
        .is_none_or(|plan_id| receipt.plan_id.as_ref() == Some(plan_id));
    let tool_matches = filter
        .tool_name
        .as_ref()
        .is_none_or(|tool_name| receipt.tool_name == *tool_name);
    let failure_matches = !filter.failed_only || receipt.exit_status != 0;

    session_matches && plan_matches && tool_matches && failure_matches
}

fn receipt_args_include_receipt_id(receipt: &ReceiptRecord, receipt_id: &str) -> bool {
    receipt
        .args
        .get("receipt_ids")
        .and_then(Value::as_array)
        .is_some_and(|receipt_ids| {
            receipt_ids
                .iter()
                .any(|candidate| candidate.as_str() == Some(receipt_id))
        })
}

fn receipt_args_has_receipt_ids(receipt: &ReceiptRecord) -> bool {
    receipt
        .args
        .get("receipt_ids")
        .and_then(Value::as_array)
        .is_some()
}

pub(super) fn receipt_list_value(receipt: ReceiptRecord) -> Result<Value> {
    let diff_summary = receipt_diff_summary(&receipt);
    let mut value = serde_json::to_value(receipt)?;
    if let Some(object) = value.as_object_mut() {
        object.insert("diff_summary".to_string(), Value::String(diff_summary));
    }
    Ok(value)
}

fn tool_receipt_status(receipt: ReceiptRecord) -> ToolReceiptStatus {
    let diff_summary = receipt_diff_summary(&receipt);
    let changed_path_count = receipt
        .changed_path_count
        .unwrap_or(receipt.changed_paths.len());
    let changed_paths_truncated =
        receipt.changed_paths_truncated || changed_path_count > receipt.changed_paths.len();
    ToolReceiptStatus {
        receipt_id: receipt.id,
        exit_status: receipt.exit_status,
        ended_at_ms: receipt.ended_at_ms,
        changed_paths: receipt.changed_paths,
        changed_path_count,
        changed_paths_truncated,
        changed_paths_digest: receipt.changed_paths_digest,
        diff_summary,
        worktree_fingerprint: receipt.worktree_fingerprint,
        worktree_fingerprint_error: receipt.worktree_fingerprint_error,
    }
}

fn work_review_receipt_status(receipt: ReceiptRecord) -> WorkReviewReceiptStatus {
    let diff_summary = receipt_diff_summary(&receipt);
    let changed_path_count = receipt
        .changed_path_count
        .unwrap_or(receipt.changed_paths.len());
    let changed_paths_truncated =
        receipt.changed_paths_truncated || changed_path_count > receipt.changed_paths.len();
    WorkReviewReceiptStatus {
        receipt_id: receipt.id,
        exit_status: receipt.exit_status,
        ended_at_ms: receipt.ended_at_ms,
        evidence: receipt.evidence.as_ref().map(work_review_receipt_evidence),
        changed_paths: receipt.changed_paths,
        changed_path_count,
        changed_paths_truncated,
        changed_paths_digest: receipt.changed_paths_digest,
        diff_summary,
        worktree_fingerprint: receipt.worktree_fingerprint,
        worktree_fingerprint_error: receipt.worktree_fingerprint_error,
    }
}

fn work_review_receipt_evidence(evidence: &Value) -> WorkReviewReceiptEvidence {
    let retained_finding_count = evidence["findings"].as_array().map(Vec::len);
    let retained_actionable_count = evidence["actionable_findings"].as_array().map(Vec::len);
    let mut parse_error = evidence["parse_error"].as_str().map(str::to_string);
    if parse_error.is_none() && evidence["status"].as_str().is_none() {
        parse_error = Some("review evidence is missing status".into());
    }
    if parse_error.is_none()
        && evidence.get("findings").is_some()
        && retained_finding_count.is_none()
    {
        parse_error = Some("review evidence findings is not an array".into());
    }
    if parse_error.is_none()
        && evidence.get("actionable_findings").is_some()
        && retained_actionable_count.is_none()
    {
        parse_error = Some("review evidence actionable_findings is not an array".into());
    }
    WorkReviewReceiptEvidence {
        status: evidence["status"].as_str().map(str::to_string),
        finding_count: evidence["raw_finding_count"]
            .as_u64()
            .or_else(|| retained_finding_count.map(|count| count as u64)),
        actionable_count: evidence["raw_actionable_count"]
            .as_u64()
            .or_else(|| retained_actionable_count.map(|count| count as u64)),
        retained_finding_count,
        retained_actionable_count,
        findings_truncated: evidence["findings_truncated"].as_bool(),
        actionable_findings_truncated: evidence["actionable_findings_truncated"].as_bool(),
        threshold: evidence["threshold"].as_str().map(str::to_string),
        parse_error,
    }
}

pub(super) fn receipt_diff_summary(receipt: &ReceiptRecord) -> String {
    if receipt.git_status_error.is_some() || receipt.git_diff_stat_error.is_some() {
        return "git metadata unavailable".to_string();
    }

    let stat = &receipt.diff_stat;
    if stat.files == 0 && stat.insertions == 0 && stat.deletions == 0 {
        "no changes".to_string()
    } else {
        let file_count = if stat.files == 1 {
            "1 file".to_string()
        } else {
            format!("{} files", stat.files)
        };
        format!("{file_count}, +{} -{}", stat.insertions, stat.deletions)
    }
}

fn receipt_output_preview(value: &str, exit_status: i32) -> String {
    if exit_status != 0 {
        return truncate(value);
    }
    truncate_to_bytes(value, SUCCESSFUL_RECEIPT_PREVIEW_BYTES)
}

fn truncate_to_bytes(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

fn receipt_git_metadata(
    ctx: &RepoContext,
    collect_git_metadata: bool,
    collect_worktree_fingerprint: bool,
) -> GitReceiptMetadata {
    if !collect_git_metadata {
        return GitReceiptMetadata::default();
    }

    if collect_worktree_fingerprint {
        collect_git_receipt_metadata(ctx.root())
    } else {
        collect_git_receipt_metadata_without_worktree_fingerprint(ctx.root())
    }
}

#[cfg(test)]
mod tests;
