use std::path::Path;

use anyhow::{Context, Result};
use jig_contract::{ActionId, ComponentId, Finding, TargetId};
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
use super::support::{new_id, truncate};

mod archive;
mod journal;
mod originals;
mod validity;
pub(crate) use originals::OriginalReceiptIndex;
pub(crate) use validity::metadata_time;
mod target_evidence;
pub(super) use archive::parse_archive_before_ms;
use archive::refuse_unterminated_receipt_stream;
pub(crate) use archive::{StateArchiveRequest, receipts_archive, receipts_export};
#[cfg(test)]
use archive::{sha256_reader, write_receipt_gzip};
#[cfg(test)]
pub(crate) use journal::receipt_append_may_have_landed_for_test;
pub(crate) use journal::{
    receipt_append_may_have_landed, receipt_record_id, with_receipt_journal_writer,
    with_receipt_journal_writer_until,
};
pub(crate) use target_evidence::TargetReceiptStatus;

const SUCCESSFUL_RECEIPT_PREVIEW_BYTES: usize = 512;

pub(crate) struct ReceiptInput<'a> {
    pub(crate) tool_name: &'a str,
    pub(crate) args: Value,
    pub(crate) invoked_command_key: Option<String>,
    pub(crate) started_at_ms: u64,
    pub(crate) ended_at_ms: u64,
    pub(crate) exit_status: i32,
    pub(crate) stdout: &'a str,
    pub(crate) stderr: &'a str,
    pub(crate) evidence: Option<Value>,
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
