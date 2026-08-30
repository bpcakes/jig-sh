use std::fs::File;
use std::io;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::context::RepoContext;

use super::super::jsonl::{JsonlWriteGuard, append_jsonl_locked, with_jsonl_write_lock};
use super::super::records::ReceiptRecord;
use super::super::support::ensure_state_layout;

pub(crate) struct ReceiptJournalWriter<'a> {
    path: &'a Path,
    guard: &'a JsonlWriteGuard,
}

pub(crate) fn with_receipt_journal_writer<T>(
    ctx: &RepoContext,
    operation: impl FnOnce(&ReceiptJournalWriter<'_>) -> Result<T>,
) -> Result<T> {
    ensure_state_layout(ctx)?;
    let path = ctx.state_file("receipts.jsonl");
    with_jsonl_write_lock(&path, |guard| {
        operation(&ReceiptJournalWriter { path: &path, guard })
    })
}

impl ReceiptJournalWriter<'_> {
    pub(super) fn append(&self, receipt: &ReceiptRecord) -> Result<()> {
        append_jsonl_locked(self.guard, self.path, receipt).map(|_| ())
    }

    pub(crate) fn inspect<T>(
        &self,
        operation: impl FnOnce(&File) -> Result<T>,
    ) -> Result<Option<T>> {
        match File::open(self.path) {
            Ok(file) => operation(&file).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => {
                Err(error).with_context(|| format!("Failed to open {}", self.path.display()))
            }
        }
    }
}

pub(crate) fn validate_receipt_record_id(record: &[u8], expected_id: &str) -> Result<()> {
    let receipt = serde_json::from_slice::<ReceiptRecord>(record)
        .context("Appended receipt record does not match the durable receipt schema")?;
    if receipt.id != expected_id {
        bail!("appended receipt does not match the expected worker receipt");
    }
    Ok(())
}
