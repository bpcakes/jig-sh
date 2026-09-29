//! The legacy receipt journal, `.agent/state/receipts.jsonl`. Jig no longer
//! writes receipts; export, archive, restore, and diagnosis still read the
//! records earlier runtimes wrote.

use std::path::Path;

use anyhow::{Context, Result};
#[cfg(test)]
use serde_json::Value;

use super::jsonl::{RawJsonlRecord, scan_jsonl_raw};
use super::records::ReceiptRecord;

mod archive;
pub(super) use archive::parse_archive_before_ms;
use archive::refuse_unterminated_receipt_stream;
pub(crate) use archive::{StateArchiveRequest, receipts_archive, receipts_export};
#[cfg(test)]
use archive::{sha256_reader, write_receipt_gzip};

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

#[cfg(test)]
mod tests;
