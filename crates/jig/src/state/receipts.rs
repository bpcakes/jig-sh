#[cfg(test)]
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};
use jig_contract::{ActionId, ComponentId, Finding, TargetId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cancellation::{ensure_status_collection_active, status_collection_cancellation};
use crate::context::RepoContext;
use crate::git_receipts::{
    GitReceiptMetadata, collect_git_receipt_metadata,
    collect_git_receipt_metadata_with_cancellation,
    collect_git_receipt_metadata_without_worktree_fingerprint,
    collect_git_receipt_metadata_without_worktree_fingerprint_with_cancellation,
    is_git_receipt_collection_cancellation, repo_worktree_fingerprint,
    repo_worktree_fingerprint_with_cancellation, repository_source_snapshot,
    repository_source_snapshot_with_cancellation,
};
use crate::tool_defs::tool;

use super::jsonl::{RawJsonlRecord, read_receipts_reverse, scan_jsonl_raw};
use super::privacy::{
    redact_repository_root, redact_repository_root_in_value, repository_root_spellings,
};
use super::records::ReceiptRecord;
use super::sessions::current_session;
use super::support::{ensure_state_layout, new_id, now_ms, truncate};

mod archive;
mod journal;
mod target_evidence;
pub(super) use archive::parse_archive_before_ms;
use archive::refuse_unterminated_receipt_stream;
#[cfg(test)]
use archive::{ReceiptProtectionIndex, sha256_reader, write_receipt_gzip};
pub(crate) use archive::{StateArchiveRequest, receipts_archive, receipts_export};
#[cfg(test)]
pub(crate) use journal::receipt_append_may_have_landed_for_test;
pub(crate) use journal::{
    receipt_append_may_have_landed, receipt_record_id, with_receipt_journal_writer,
    with_receipt_journal_writer_until,
};
pub(crate) use target_evidence::TargetReceiptStatus;
use target_evidence::{IndexedTargetReceipts, TargetReceiptGroup};

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

#[derive(Clone, Debug)]
pub(crate) struct TargetReceiptMetadata {
    pub(crate) run_id: String,
    pub(crate) target: TargetId,
    pub(crate) config_digest: String,
    pub(crate) input_digest: String,
    pub(crate) findings: Vec<Finding>,
    pub(crate) finding_count: Option<u64>,
    pub(crate) findings_truncated: bool,
    pub(crate) findings_digest: Option<String>,
    pub(crate) evaluated_at_ms: Option<u64>,
    pub(crate) valid_until_ms: Option<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct FileBudgetLifecycleReceipt {
    pub(crate) receipt_id: String,
    pub(crate) config_digest: Option<String>,
    pub(crate) input_digest: Option<String>,
    pub(crate) exit_status: i32,
    pub(crate) worktree_fingerprint: Option<String>,
    pub(crate) worktree_fingerprint_error: Option<String>,
    pub(crate) evaluated_at_ms: Option<u64>,
    pub(crate) valid_until_ms: Option<u64>,
    pub(crate) evidence: Option<Value>,
}

pub(crate) fn latest_file_budget_lifecycle_receipt(
    ctx: &RepoContext,
) -> Result<Option<FileBudgetLifecycleReceipt>> {
    let target = TargetId::new(ComponentId::parse("repo")?, ActionId::parse("file-budget")?);
    let (mut receipts, _) =
        read_receipts_reverse(&ctx.state_file("receipts.jsonl"), 1, |receipt| {
            receipt.target.as_ref() == Some(&target)
        })?;
    Ok(receipts.pop().map(|receipt| FileBudgetLifecycleReceipt {
        receipt_id: receipt.id,
        config_digest: receipt.config_digest,
        input_digest: receipt.input_digest,
        exit_status: receipt.exit_status,
        worktree_fingerprint: receipt.worktree_fingerprint,
        worktree_fingerprint_error: receipt.worktree_fingerprint_error,
        evaluated_at_ms: receipt.evaluated_at_ms,
        valid_until_ms: receipt.valid_until_ms,
        evidence: receipt.evidence,
    }))
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
    pub(crate) valid_until_ms: Option<u64>,
    pub(crate) requires_time_validity: bool,
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
    pub(crate) valid_until_ms: Option<u64>,
    pub(crate) requires_time_validity: bool,
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
    pub(crate) findings: Vec<WorkReviewFinding>,
    parse: WorkReviewEvidenceParse,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkReviewFinding {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) path: Option<String>,
    pub(crate) line: Option<u64>,
}

pub(crate) const WORK_CHECK_EVIDENCE_SCHEMA: &str = "jig.work_check/v2";

const fn usize_is_zero(value: &usize) -> bool {
    *value == 0
}

const fn bool_is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct WorkCheckBatchEvidence {
    pub(crate) schema: String,
    #[serde(default)]
    pub(crate) changed_paths: Vec<String>,
    #[serde(default)]
    pub(crate) changed_path_count: usize,
    #[serde(default)]
    pub(crate) changed_paths_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) changed_paths_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) valid_until_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "bool_is_false")]
    pub(crate) requires_time_validity: bool,
    pub(crate) gates: Vec<WorkCheckGateEvidence>,
}

impl WorkCheckBatchEvidence {
    pub(crate) fn into_hydrated_gates(self) -> Vec<WorkCheckGateEvidence> {
        let Self {
            changed_paths,
            changed_path_count,
            changed_paths_truncated,
            changed_paths_digest,
            valid_until_ms,
            requires_time_validity,
            mut gates,
            ..
        } = self;
        for gate in &mut gates {
            if gate.changed_paths_digest.is_none() && changed_paths_digest.is_some() {
                gate.changed_paths.clone_from(&changed_paths);
                gate.changed_path_count = changed_path_count;
                gate.changed_paths_truncated = changed_paths_truncated;
                gate.changed_paths_digest.clone_from(&changed_paths_digest);
            }
            if gate.valid_until_ms.is_none() {
                gate.valid_until_ms = valid_until_ms;
            }
            gate.requires_time_validity |= requires_time_validity;
        }
        gates
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct WorkCheckGateEvidence {
    pub(crate) gate_id: String,
    pub(crate) tool: String,
    pub(crate) status: String,
    pub(crate) applicability: String,
    #[serde(default)]
    pub(crate) required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) paths: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) paths_ignore: Vec<String>,
    #[serde(default)]
    pub(crate) reuse: bool,
    #[serde(default)]
    pub(crate) forced: bool,
    pub(crate) gate_signature: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) baseline_oid: Option<String>,
    pub(crate) reason: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) changed_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "usize_is_zero")]
    pub(crate) changed_path_count: usize,
    #[serde(default, skip_serializing_if = "bool_is_false")]
    pub(crate) changed_paths_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) changed_paths_digest: Option<String>,
    #[serde(default)]
    pub(crate) matching_paths: Vec<String>,
    #[serde(default)]
    pub(crate) matching_path_count: usize,
    #[serde(default)]
    pub(crate) matching_paths_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) matching_paths_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) scope_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) scope_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) tool_receipt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) exit_status: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_plan_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_batch_receipt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_tool_receipt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) valid_until_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "bool_is_false")]
    pub(crate) requires_time_validity: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkCheckGateReceiptStatus {
    pub(crate) batch: ToolReceiptStatus,
    pub(crate) evidence: WorkCheckGateEvidence,
}

#[derive(Clone, Debug)]
pub(crate) struct ReusableWorkCheckEvidence {
    pub(crate) source_plan_id: String,
    pub(crate) source_batch_receipt_id: String,
    pub(crate) source_tool_receipt_id: String,
    pub(crate) valid_until_ms: Option<u64>,
    pub(crate) requires_time_validity: bool,
}

enum ReusableWorkCheckScanState {
    Direct(ReusableWorkCheckEvidence),
    Tombstone,
}

#[derive(Clone, Debug)]
pub(crate) struct ReusableWorkCheckQuery {
    pub(crate) gate_id: String,
    pub(crate) tool: String,
    pub(crate) gate_signature: String,
    pub(crate) scope_fingerprint: String,
}

#[derive(Clone, Debug)]
enum WorkReviewEvidenceParse {
    Valid,
    Invalid(String),
}

impl WorkReviewReceiptEvidence {
    pub(crate) fn parse_error(&self) -> Option<&str> {
        match &self.parse {
            WorkReviewEvidenceParse::Valid => None,
            WorkReviewEvidenceParse::Invalid(error) => Some(error),
        }
    }
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
    check_gates: BTreeMap<String, WorkCheckGateReceiptStatus>,
    reviews: BTreeMap<String, WorkReviewReceiptStatus>,
    evidence: BTreeMap<String, IndexedTargetReceipts>,
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

    pub(crate) fn check_gate_receipt(&self, gate_id: &str) -> Option<&WorkCheckGateReceiptStatus> {
        self.check_gates.get(gate_id)
    }

    pub(crate) fn target_receipts(&self, gate_id: &str) -> Option<&TargetReceiptGroup> {
        self.evidence
            .get(gate_id)
            .and_then(IndexedTargetReceipts::selected)
    }

    pub(crate) fn target_receipt_error(&self, gate_id: &str) -> Option<&str> {
        self.evidence
            .get(gate_id)
            .and_then(IndexedTargetReceipts::error)
    }
}

#[cfg(test)]
thread_local! {
    static WORK_GATE_RECEIPT_INDEX_SCAN_COUNT: Cell<usize> = const { Cell::new(0) };
    static REUSABLE_WORK_CHECK_SCAN_COUNT: Cell<usize> = const { Cell::new(0) };
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
    evidence_targets: &BTreeMap<String, BTreeSet<TargetId>>,
) -> Result<WorkGateReceiptIndex> {
    work_gate_receipt_index_with_cancellation(
        ctx,
        plan_id,
        check_tools,
        review_gate_ids,
        evidence_targets,
        &|| false,
    )
}

pub(crate) fn reusable_work_check_evidence_batch_with_cancellation(
    ctx: &RepoContext,
    current_plan_id: &str,
    queries: &[ReusableWorkCheckQuery],
    cancelled: &dyn Fn() -> bool,
) -> Result<BTreeMap<String, ReusableWorkCheckEvidence>> {
    ensure_receipt_scan_active(cancelled)?;
    if queries.is_empty() {
        return Ok(BTreeMap::new());
    }
    #[cfg(test)]
    REUSABLE_WORK_CHECK_SCAN_COUNT.set(REUSABLE_WORK_CHECK_SCAN_COUNT.get() + 1);
    ensure_state_layout(ctx)?;
    let path = ctx.state_file("receipts.jsonl");
    let queries = queries
        .iter()
        .map(|query| (query.gate_id.as_str(), query))
        .collect::<BTreeMap<_, _>>();
    let requested_tools = queries
        .values()
        .map(|query| query.tool.as_str())
        .collect::<BTreeSet<_>>();
    let scan_now_ms = now_ms();
    let mut successful_receipts = BTreeMap::new();
    let mut latest_matching_evidence = BTreeMap::new();
    let mut current_plan_evidence = BTreeSet::new();
    scan_jsonl_raw(&path, cancelled, |record| {
        ensure_receipt_scan_active(cancelled)?;
        let receipt = parse_raw_receipt(record, &path)?;
        if receipt.exit_status == 0 && requested_tools.contains(receipt.tool_name.as_str()) {
            successful_receipts.insert(
                receipt.id.clone(),
                (receipt.tool_name.clone(), tool_receipt_status(&receipt)),
            );
        }
        if receipt.tool_name != tool::WORK_CHECK {
            return Ok(());
        }
        let Some(plan_id) = receipt.plan_id.as_deref() else {
            return Ok(());
        };
        let selected_query_gates = receipt_arg_strings(&receipt, "gates")
            .filter(|gate_id| queries.contains_key(*gate_id))
            .map(str::to_string)
            .collect::<Vec<_>>();
        if plan_id == current_plan_id {
            current_plan_evidence.extend(selected_query_gates.iter().cloned());
        }
        let Some(evidence) = receipt
            .evidence
            .as_ref()
            .and_then(|evidence| {
                serde_json::from_value::<WorkCheckBatchEvidence>(evidence.clone()).ok()
            })
            .filter(|evidence| evidence.schema == WORK_CHECK_EVIDENCE_SCHEMA)
        else {
            if plan_id != current_plan_id {
                for gate_id in selected_query_gates {
                    latest_matching_evidence.insert(gate_id, ReusableWorkCheckScanState::Tombstone);
                }
            }
            return Ok(());
        };
        if plan_id == current_plan_id {
            current_plan_evidence.extend(
                evidence
                    .gates
                    .iter()
                    .filter(|gate| queries.contains_key(gate.gate_id.as_str()))
                    .map(|gate| gate.gate_id.clone()),
            );
            return Ok(());
        }
        let gates = evidence.into_hydrated_gates();
        let reported_query_gates = gates
            .iter()
            .filter(|gate| queries.contains_key(gate.gate_id.as_str()))
            .map(|gate| gate.gate_id.clone())
            .collect::<BTreeSet<_>>();
        for gate_id in selected_query_gates {
            if !reported_query_gates.contains(gate_id.as_str()) {
                latest_matching_evidence.insert(gate_id, ReusableWorkCheckScanState::Tombstone);
            }
        }
        for gate in gates {
            let Some(query) = queries.get(gate.gate_id.as_str()) else {
                continue;
            };
            if gate.tool != query.tool
                || gate.gate_signature != query.gate_signature
                || gate.scope_fingerprint.as_deref() != Some(query.scope_fingerprint.as_str())
            {
                continue;
            }
            if gate.status == "reused" {
                continue;
            }
            let candidate = gate.tool_receipt_id.as_deref().and_then(|proving_receipt| {
                let proving = successful_receipts.get(proving_receipt);
                let valid_until_ms = [
                    receipt.valid_until_ms,
                    gate.valid_until_ms,
                    proving.and_then(|(_, status)| status.valid_until_ms),
                ]
                .into_iter()
                .flatten()
                .min();
                let requires_time_validity = evidence_requires_time_validity(
                    receipt.evidence.as_ref().unwrap_or(&Value::Null),
                ) || gate.requires_time_validity
                    || proving.is_some_and(|(_, status)| status.requires_time_validity);
                (receipt.exit_status == 0
                    && receipt.worktree_fingerprint.is_some()
                    && receipt.worktree_fingerprint_error.is_none()
                    && gate.status == "executed"
                    && proving.map(|(tool, _)| tool.as_str()) == Some(gate.tool.as_str())
                    && time_validity_is_current(
                        valid_until_ms,
                        requires_time_validity,
                        scan_now_ms,
                    ))
                .then(|| ReusableWorkCheckEvidence {
                    source_plan_id: plan_id.to_string(),
                    source_batch_receipt_id: receipt.id.clone(),
                    source_tool_receipt_id: proving_receipt.to_string(),
                    valid_until_ms,
                    requires_time_validity,
                })
            });
            latest_matching_evidence.insert(
                gate.gate_id,
                candidate.map_or(
                    ReusableWorkCheckScanState::Tombstone,
                    ReusableWorkCheckScanState::Direct,
                ),
            );
        }
        Ok(())
    })?;
    ensure_receipt_scan_active(cancelled)?;
    latest_matching_evidence.retain(|gate_id, _| !current_plan_evidence.contains(gate_id));
    Ok(latest_matching_evidence
        .into_iter()
        .filter_map(|(gate_id, evidence)| match evidence {
            ReusableWorkCheckScanState::Direct(evidence) => Some((gate_id, evidence)),
            ReusableWorkCheckScanState::Tombstone => None,
        })
        .collect())
}

pub(crate) const fn time_validity_is_current(
    valid_until_ms: Option<u64>,
    requires_time_validity: bool,
    now_ms: u64,
) -> bool {
    match valid_until_ms {
        Some(boundary) => now_ms < boundary,
        None => !requires_time_validity,
    }
}

pub(crate) fn work_gate_receipt_index_with_cancellation(
    ctx: &RepoContext,
    plan_id: &str,
    check_tools: &BTreeSet<String>,
    review_gate_ids: &BTreeSet<String>,
    evidence_targets: &BTreeMap<String, BTreeSet<TargetId>>,
    cancelled: &dyn Fn() -> bool,
) -> Result<WorkGateReceiptIndex> {
    let plan_ids = BTreeSet::from([plan_id.to_string()]);
    let mut indexes = work_gate_receipt_indexes_with_cancellation(
        ctx,
        &plan_ids,
        check_tools,
        review_gate_ids,
        evidence_targets,
        cancelled,
    )?;
    Ok(indexes
        .remove(plan_id)
        .expect("requested plan index was initialized"))
}

pub(crate) fn work_gate_receipt_indexes_with_cancellation(
    ctx: &RepoContext,
    plan_ids: &BTreeSet<String>,
    check_tools: &BTreeSet<String>,
    review_gate_ids: &BTreeSet<String>,
    evidence_targets: &BTreeMap<String, BTreeSet<TargetId>>,
    cancelled: &dyn Fn() -> bool,
) -> Result<BTreeMap<String, WorkGateReceiptIndex>> {
    ensure_receipt_scan_active(cancelled)?;

    let mut indexes =
        WorkGateReceiptIndexes::new(plan_ids, check_tools, review_gate_ids, evidence_targets);
    if plan_ids.is_empty()
        || (check_tools.is_empty() && review_gate_ids.is_empty() && evidence_targets.is_empty())
    {
        return Ok(indexes.into_indexes());
    }
    ensure_state_layout(ctx)?;

    let path = ctx.state_file("receipts.jsonl");
    #[cfg(test)]
    WORK_GATE_RECEIPT_INDEX_SCAN_COUNT
        .set(WORK_GATE_RECEIPT_INDEX_SCAN_COUNT.get().saturating_add(1));
    scan_jsonl_raw(&path, cancelled, |record| {
        ensure_receipt_scan_active(cancelled)?;
        let receipt = parse_raw_receipt(record, &path)?;
        indexes.observe(&receipt);
        Ok(())
    })?;
    ensure_receipt_scan_active(cancelled)?;
    Ok(indexes.into_indexes())
}

include!("receipts/tail.rs");
mod dashboard;
pub(crate) use dashboard::WorkGateReceiptIndexes;
#[cfg(test)]
mod tests;
