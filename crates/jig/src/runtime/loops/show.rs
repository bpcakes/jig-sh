//! `jig loop show`: one occurrence and the evidence its tick recorded.

use anyhow::{Result, bail};
use serde_json::{Value, json};

use crate::command::LoopShowRequest;
use crate::context::RepoContext;

use super::evidence;
use super::occurrence::OccurrenceStore;

pub(super) fn show_occurrence(
    ctx: &RepoContext,
    request: LoopShowRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    let occurrence_id = request.occurrence.trim();
    if occurrence_id.is_empty() {
        bail!("occurrence must not be empty");
    }
    let occurrence = OccurrenceStore::new(ctx)
        .snapshot_read_only_with_cancellation(cancelled)?
        .into_iter()
        .find(|occurrence| occurrence.occurrence_id == occurrence_id);
    let evidence = evidence::read(ctx, occurrence_id, cancelled)?;
    if occurrence.is_none() && evidence.is_none() {
        bail!(
            "Loop occurrence not found: {occurrence_id}; `jig loop status` lists the occurrences still in history"
        );
    }
    let mut value = json!({
        "ok": true,
        "command": "loop show",
        "occurrence_id": occurrence_id,
        "occurrence": occurrence.as_ref().map(|occurrence| occurrence.status_view()),
        "evidence": evidence,
    });
    if let Some(receipt) = occurrence.and_then(|occurrence| occurrence.legacy_worker_receipt_id) {
        value["legacy_worker_receipt_id"] = json!(receipt);
    }
    Ok(value)
}
