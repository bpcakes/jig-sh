use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::context::RepoContext;
use crate::git_receipts::{
    GitReceiptMetadata, collect_git_receipt_metadata,
    collect_git_receipt_metadata_with_cancellation,
    collect_git_receipt_metadata_without_worktree_fingerprint,
    collect_git_receipt_metadata_without_worktree_fingerprint_with_cancellation,
};

use super::jsonl::{RawJsonlRecord, scan_jsonl_raw};
use super::privacy::{
    redact_repository_root, redact_repository_root_in_value, repository_root_spellings,
};
use super::records::ReceiptRecord;
use super::support::{new_id, truncate};

mod archive;
mod journal;
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

include!("receipts/tail.rs");
#[cfg(test)]
mod tests;
