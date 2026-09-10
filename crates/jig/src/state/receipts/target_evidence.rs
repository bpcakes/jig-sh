use std::collections::{BTreeMap, BTreeSet};

use jig_contract::TargetId;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TargetReceiptStatus {
    pub(crate) receipt_id: String,
    pub(crate) run_id: Option<String>,
    pub(crate) plan_id: Option<String>,
    pub(crate) target_freshness: Option<jig_contract::freshness::TargetFreshnessMetadata>,
    pub(crate) target: TargetId,
    pub(crate) config_digest: Option<String>,
    pub(crate) input_digest: Option<String>,
    pub(crate) exit_status: i32,
    pub(crate) started_at_ms: u64,
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

/// One original receipt per configured target. Run IDs describe execution
/// provenance, not whether two receipts prove the same current inputs.
#[derive(Debug)]
pub(super) struct IndexedTargetReceipts {
    required_targets: BTreeSet<TargetId>,
    selected: BTreeMap<TargetId, TargetReceiptStatus>,
}

impl IndexedTargetReceipts {
    pub(super) fn new(required_targets: BTreeSet<TargetId>) -> Self {
        Self {
            required_targets,
            selected: BTreeMap::new(),
        }
    }

    pub(super) fn observe(&mut self, receipt: &TargetReceiptStatus) {
        if !self.required_targets.contains(&receipt.target) {
            return;
        }
        // Select outcomes, not successes. A newer failure or unverifiable receipt
        // must not resurrect a previous pass. Ordering is stable under journal
        // union, duplicate records and out-of-order physical lines.
        let replace = self.selected.get(&receipt.target).is_none_or(|previous| {
            (receipt.ended_at_ms, receipt.receipt_id.as_str())
                > (previous.ended_at_ms, previous.receipt_id.as_str())
        });
        if replace {
            self.selected
                .insert(receipt.target.clone(), receipt.clone());
        }
    }

    pub(super) fn selected(&self) -> &BTreeMap<TargetId, TargetReceiptStatus> {
        &self.selected
    }
}

#[cfg(test)]
mod tests;
