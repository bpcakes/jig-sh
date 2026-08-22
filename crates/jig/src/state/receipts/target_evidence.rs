use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use jig_contract::TargetId;

const MAX_INCOMPLETE_TARGET_RECEIPT_GROUPS: usize = 1024;

#[derive(Clone, Debug)]
pub(crate) struct TargetReceiptStatus {
    pub(crate) receipt_id: String,
    pub(crate) run_id: String,
    pub(crate) target: TargetId,
    pub(crate) config_digest: Option<String>,
    pub(crate) input_digest: Option<String>,
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
pub(crate) struct TargetReceiptGroup {
    pub(crate) run_id: String,
    pub(crate) receipts: BTreeMap<TargetId, TargetReceiptStatus>,
}

impl TargetReceiptGroup {
    fn latest_ended_at_ms(&self) -> u64 {
        self.receipts
            .values()
            .map(|receipt| receipt.ended_at_ms)
            .max()
            .unwrap_or(0)
    }
}

#[derive(Debug)]
pub(super) struct IndexedTargetReceipts {
    required_targets: BTreeSet<TargetId>,
    latest_complete: Option<TargetReceiptGroup>,
    partial_by_run: BTreeMap<String, TargetReceiptGroup>,
}

impl IndexedTargetReceipts {
    pub(super) fn new(required_targets: BTreeSet<TargetId>) -> Self {
        Self {
            required_targets,
            latest_complete: None,
            partial_by_run: BTreeMap::new(),
        }
    }

    pub(super) fn observe(&mut self, receipt: TargetReceiptStatus) -> Result<()> {
        if !self.required_targets.contains(&receipt.target) {
            return Ok(());
        }
        let run_id = receipt.run_id.clone();
        if !self.partial_by_run.contains_key(&run_id)
            && self.partial_by_run.len() >= MAX_INCOMPLETE_TARGET_RECEIPT_GROUPS
        {
            bail!(
                "work evidence contains more than {MAX_INCOMPLETE_TARGET_RECEIPT_GROUPS} incomplete run groups; archive stale receipts before evaluating this gate"
            );
        }
        let group = self
            .partial_by_run
            .entry(run_id.clone())
            .or_insert_with(|| TargetReceiptGroup {
                run_id: run_id.clone(),
                receipts: BTreeMap::new(),
            });
        group.receipts.insert(receipt.target.clone(), receipt);
        if group.receipts.len() != self.required_targets.len() {
            return Ok(());
        }

        let complete = self
            .partial_by_run
            .remove(&run_id)
            .expect("the completed target receipt group was just indexed");
        let replace = self.latest_complete.as_ref().is_none_or(|latest| {
            (complete.latest_ended_at_ms(), complete.run_id.as_str())
                >= (latest.latest_ended_at_ms(), latest.run_id.as_str())
        });
        if replace {
            self.latest_complete = Some(complete);
        }
        Ok(())
    }

    pub(super) fn selected(&self) -> Option<&TargetReceiptGroup> {
        self.latest_complete.as_ref().or_else(|| {
            self.partial_by_run.values().max_by(|left, right| {
                (left.latest_ended_at_ms(), left.run_id.as_str())
                    .cmp(&(right.latest_ended_at_ms(), right.run_id.as_str()))
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(run_id: String, target: &str) -> TargetReceiptStatus {
        TargetReceiptStatus {
            receipt_id: format!("receipt_{run_id}"),
            run_id,
            target: target.parse().unwrap(),
            config_digest: None,
            input_digest: None,
            exit_status: 0,
            ended_at_ms: 1,
            changed_paths: Vec::new(),
            changed_path_count: 0,
            changed_paths_truncated: false,
            changed_paths_digest: None,
            diff_summary: String::new(),
            worktree_fingerprint: None,
            worktree_fingerprint_error: None,
        }
    }

    #[test]
    fn incomplete_run_groups_fail_closed_at_the_memory_bound() {
        let required = BTreeSet::from(["api:lint".parse().unwrap(), "api:test".parse().unwrap()]);
        let mut index = IndexedTargetReceipts::new(required);
        for sequence in 0..MAX_INCOMPLETE_TARGET_RECEIPT_GROUPS {
            index
                .observe(receipt(format!("run_{sequence}"), "api:lint"))
                .unwrap();
        }

        let error = index
            .observe(receipt("run_overflow".into(), "api:lint"))
            .unwrap_err();

        assert!(error.to_string().contains("incomplete run groups"));
    }
}
