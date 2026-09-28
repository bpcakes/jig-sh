use std::path::Path;

use anyhow::{Context, Result};
use jig_contract::freshness::EffectiveTimeValidityV1;
use jig_contract::{ActionId, ComponentId, Finding, TargetId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::RepoContext;
use crate::git_receipts::{
    GitReceiptMetadata, collect_git_receipt_metadata,
    collect_git_receipt_metadata_with_cancellation,
    collect_git_receipt_metadata_without_worktree_fingerprint,
    collect_git_receipt_metadata_without_worktree_fingerprint_with_cancellation,
};

use super::jsonl::{RawJsonlRecord, read_receipts_reverse, scan_jsonl_raw};
use super::privacy::{
    redact_repository_root, redact_repository_root_in_value, repository_root_spellings,
};
use super::records::ReceiptRecord;
use super::support::{new_id, now_ms, truncate};

mod archive;
mod journal;
mod originals;
mod validity;
pub(crate) use originals::OriginalReceiptIndex;
pub(crate) use validity::{metadata_time, receipt_effective_time};
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
use target_evidence::{IndexedTargetReceipts, cross_plan_receipt_is_eligible};

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
    pub(crate) target_freshness: Option<jig_contract::freshness::TargetFreshnessMetadata>,
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
    pub(crate) original: TargetReceiptStatus,
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
    Ok(receipts.pop().map(FileBudgetLifecycleReceipt::from_record))
}

impl FileBudgetLifecycleReceipt {
    fn from_record(receipt: ReceiptRecord) -> Self {
        Self {
            original: target_receipt_status(
                &receipt,
                receipt
                    .target
                    .as_ref()
                    .expect("lifecycle selection requires target metadata"),
            ),
            receipt_id: receipt.id,
            config_digest: receipt.config_digest,
            input_digest: receipt.input_digest,
            exit_status: receipt.exit_status,
            worktree_fingerprint: receipt.worktree_fingerprint,
            worktree_fingerprint_error: receipt.worktree_fingerprint_error,
            evaluated_at_ms: receipt.evaluated_at_ms,
            valid_until_ms: receipt.valid_until_ms,
            evidence: receipt.evidence,
        }
    }
}

#[cfg(test)]
pub(super) struct StateToolReceipt<'a> {
    pub(super) tool_name: &'a str,
    pub(super) args: Value,
    pub(super) started_at_ms: u64,
    pub(super) plan_id: Option<String>,
    pub(super) session_override: Option<String>,
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

pub(crate) const WORK_CHECK_EVIDENCE_SCHEMA: &str = "jig.work_check/v2";
/// Schema emitted by target-oriented `work check` and consumed by deep linkage diagnosis.
pub(crate) const WORK_CHECK_TARGETS_SCHEMA: &str = "jig.work_check_targets/v1";

#[cfg(test)]
pub(crate) fn work_check_targets_evidence(targets: &[Value]) -> Value {
    serde_json::json!({"schema": WORK_CHECK_TARGETS_SCHEMA, "targets": targets})
}

const fn usize_is_zero(value: &usize) -> bool {
    *value == 0
}

const fn bool_is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct WorkCheckBatchEvidence {
    #[serde(default, flatten)]
    pub(crate) effective_time: Option<EffectiveTimeValidityV1>,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct WorkCheckGateEvidence {
    #[serde(default, flatten)]
    pub(crate) effective_time: Option<EffectiveTimeValidityV1>,
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

include!("receipts/tail.rs");
#[cfg(test)]
mod tests;
