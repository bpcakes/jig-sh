use std::collections::{BTreeMap, BTreeSet};

use jig_contract::TargetId;

const MAX_INCOMPLETE_TARGET_RECEIPT_GROUPS: usize = 1024;
const MAX_ARCHIVE_INCOMPLETE_TARGET_RECEIPT_GROUPS: usize = 16 * 1024;
const MAX_SUPERSEDED_TARGET_RECEIPT_GROUPS: usize = 256 * 1024;

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
    superseded_complete_runs: BTreeSet<String>,
    max_incomplete_groups: usize,
    index_error: Option<String>,
    permanent_index_error: Option<String>,
    uncertain_latest_candidate: Option<(u64, String)>,
}

impl IndexedTargetReceipts {
    pub(super) fn new(required_targets: BTreeSet<TargetId>) -> Self {
        Self {
            required_targets,
            latest_complete: None,
            partial_by_run: BTreeMap::new(),
            superseded_complete_runs: BTreeSet::new(),
            max_incomplete_groups: MAX_INCOMPLETE_TARGET_RECEIPT_GROUPS,
            index_error: None,
            permanent_index_error: None,
            uncertain_latest_candidate: None,
        }
    }

    /// Archive gets a substantially larger recovery allowance than ordinary
    /// gate evaluation, but remains explicitly bounded against a hostile or
    /// pathologically merged receipt stream.
    pub(super) fn for_archive(required_targets: BTreeSet<TargetId>) -> Self {
        Self {
            required_targets,
            latest_complete: None,
            partial_by_run: BTreeMap::new(),
            superseded_complete_runs: BTreeSet::new(),
            max_incomplete_groups: MAX_ARCHIVE_INCOMPLETE_TARGET_RECEIPT_GROUPS,
            index_error: None,
            permanent_index_error: None,
            uncertain_latest_candidate: None,
        }
    }

    pub(super) fn observe(&mut self, receipt: &TargetReceiptStatus) {
        if !self.required_targets.contains(&receipt.target) {
            return;
        }
        let run_id = receipt.run_id.clone();
        self.note_uncertain_candidate(receipt.ended_at_ms, &run_id);
        if self.superseded_complete_runs.contains(&run_id) {
            return;
        }
        if let Some(complete) = self
            .latest_complete
            .as_mut()
            .filter(|complete| complete.run_id == run_id)
        {
            // Union merges can duplicate a target receipt after its group was
            // already completed. Keep the selected group closed instead of
            // reopening an impossible partial group that consumes the bound.
            let should_replace = complete
                .receipts
                .get(&receipt.target)
                .is_none_or(|selected| {
                    (receipt.ended_at_ms, receipt.receipt_id.as_str())
                        > (selected.ended_at_ms, selected.receipt_id.as_str())
                });
            if should_replace {
                complete
                    .receipts
                    .insert(receipt.target.clone(), receipt.clone());
            }
            return;
        }
        if !self.partial_by_run.contains_key(&run_id)
            && self.partial_by_run.len() >= self.max_incomplete_groups
        {
            let uncertain = self
                .partial_by_run
                .values()
                .map(|group| (group.latest_ended_at_ms(), group.run_id.clone()))
                .max();
            self.index_error.get_or_insert_with(|| {
                format!(
                    "work evidence contains more than {} incomplete run groups and could contain a newer complete group; archive or repair stale receipts before retrying",
                    self.max_incomplete_groups
                )
            });
            if let Some((ended_at_ms, run_id)) = uncertain {
                self.note_uncertain_candidate(ended_at_ms, &run_id);
            }
            // `note_uncertain_candidate` ran before the overflow error was
            // installed, so record the receipt that crossed the bound now as
            // well. It may be newer than every group being discarded.
            self.note_uncertain_candidate(receipt.ended_at_ms, &run_id);
            self.partial_by_run.clear();
        }
        let group = self
            .partial_by_run
            .entry(run_id.clone())
            .or_insert_with(|| TargetReceiptGroup {
                run_id: run_id.clone(),
                receipts: BTreeMap::new(),
            });
        let should_replace = group.receipts.get(&receipt.target).is_none_or(|selected| {
            (receipt.ended_at_ms, receipt.receipt_id.as_str())
                > (selected.ended_at_ms, selected.receipt_id.as_str())
        });
        if should_replace {
            group
                .receipts
                .insert(receipt.target.clone(), receipt.clone());
        }
        if group.receipts.len() != self.required_targets.len() {
            return;
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
            if let Some(superseded) = self.latest_complete.replace(complete) {
                self.remember_superseded_complete(superseded.run_id);
            }
        } else {
            self.remember_superseded_complete(complete.run_id);
        }
    }

    fn remember_superseded_complete(&mut self, run_id: String) {
        if self.superseded_complete_runs.contains(&run_id) {
            return;
        }
        if self.superseded_complete_runs.len() >= MAX_SUPERSEDED_TARGET_RECEIPT_GROUPS {
            self.permanent_index_error.get_or_insert_with(|| {
                format!(
                    "work evidence contains more than {MAX_SUPERSEDED_TARGET_RECEIPT_GROUPS} superseded complete run groups; archive or repair stale receipts before retrying"
                )
            });
            return;
        }
        self.superseded_complete_runs.insert(run_id);
    }

    fn note_uncertain_candidate(&mut self, ended_at_ms: u64, run_id: &str) {
        if self.index_error.is_none() {
            return;
        }
        let candidate = (ended_at_ms, run_id.to_string());
        if self
            .uncertain_latest_candidate
            .as_ref()
            .is_none_or(|latest| candidate > *latest)
        {
            self.uncertain_latest_candidate = Some(candidate);
        }
    }

    fn overflow_is_resolved(&self) -> bool {
        let Some(uncertain) = self.uncertain_latest_candidate.as_ref() else {
            return false;
        };
        self.latest_complete.as_ref().is_some_and(|complete| {
            (complete.latest_ended_at_ms(), complete.run_id.as_str())
                >= (uncertain.0, uncertain.1.as_str())
        })
    }

    pub(super) fn selected(&self) -> Option<&TargetReceiptGroup> {
        if self.error().is_some() {
            return None;
        }
        self.latest_complete.as_ref().or_else(|| {
            self.partial_by_run.values().max_by(|left, right| {
                (left.latest_ended_at_ms(), left.run_id.as_str())
                    .cmp(&(right.latest_ended_at_ms(), right.run_id.as_str()))
            })
        })
    }

    pub(super) fn error(&self) -> Option<&str> {
        if self.permanent_index_error.is_some() {
            self.permanent_index_error.as_deref()
        } else if self.index_error.is_some() && !self.overflow_is_resolved() {
            self.index_error.as_deref()
        } else {
            None
        }
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
    fn incomplete_run_group_overflow_is_retained_as_a_scoped_index_error() {
        let required = BTreeSet::from(["api:lint".parse().unwrap(), "api:test".parse().unwrap()]);
        let mut index = IndexedTargetReceipts::new(required);
        for sequence in 0..MAX_INCOMPLETE_TARGET_RECEIPT_GROUPS {
            index.observe(&receipt(format!("run_{sequence}"), "api:lint"));
        }

        index.observe(&receipt("run_overflow".into(), "api:lint"));

        assert!(index.error().unwrap().contains("incomplete run groups"));
        assert!(index.selected().is_none());

        let mut unaffected = IndexedTargetReceipts::new(BTreeSet::from([
            "web:lint".parse().unwrap(),
            "web:test".parse().unwrap(),
        ]));
        unaffected.observe(&receipt("run_web".into(), "web:lint"));
        unaffected.observe(&receipt("run_web".into(), "web:test"));

        assert!(unaffected.error().is_none());
        assert_eq!(unaffected.selected().unwrap().run_id, "run_web");
    }

    #[test]
    fn newer_complete_group_resolves_historical_incomplete_group_overflow() {
        let required = BTreeSet::from(["api:lint".parse().unwrap(), "api:test".parse().unwrap()]);
        let mut index = IndexedTargetReceipts::new(required);
        for sequence in 0..=MAX_INCOMPLETE_TARGET_RECEIPT_GROUPS {
            let mut stale = receipt(format!("run_stale_{sequence}"), "api:lint");
            stale.ended_at_ms = 1;
            index.observe(&stale);
        }

        let mut lint = receipt("run_current".into(), "api:lint");
        lint.ended_at_ms = 10;
        let mut test = receipt("run_current".into(), "api:test");
        test.ended_at_ms = 11;
        index.observe(&lint);
        index.observe(&test);

        assert!(index.error().is_none());
        assert_eq!(index.selected().unwrap().run_id, "run_current");
        assert!(index.partial_by_run.len() <= MAX_INCOMPLETE_TARGET_RECEIPT_GROUPS);
    }

    #[test]
    fn possible_newer_complete_group_keeps_overflow_fail_closed() {
        let required = BTreeSet::from(["api:lint".parse().unwrap(), "api:test".parse().unwrap()]);
        let mut index = IndexedTargetReceipts::new(required);
        for sequence in 0..=MAX_INCOMPLETE_TARGET_RECEIPT_GROUPS {
            let mut stale = receipt(format!("run_stale_{sequence}"), "api:lint");
            stale.ended_at_ms = 1;
            index.observe(&stale);
        }

        let mut lint = receipt("run_current".into(), "api:lint");
        lint.ended_at_ms = 10;
        let mut test = receipt("run_current".into(), "api:test");
        test.ended_at_ms = 11;
        index.observe(&lint);
        index.observe(&test);

        let mut ambiguous = receipt("run_stale_0".into(), "api:test");
        ambiguous.ended_at_ms = 12;
        index.observe(&ambiguous);

        assert!(
            index
                .error()
                .unwrap()
                .contains("could contain a newer complete group")
        );
        assert!(index.selected().is_none());
    }

    #[test]
    fn receipt_that_crosses_the_bound_remains_in_the_uncertainty_watermark() {
        let required = BTreeSet::from(["api:lint".parse().unwrap(), "api:test".parse().unwrap()]);
        let mut index = IndexedTargetReceipts::new(required);
        for sequence in 0..MAX_INCOMPLETE_TARGET_RECEIPT_GROUPS {
            let mut stale = receipt(format!("run_stale_{sequence}"), "api:lint");
            stale.ended_at_ms = 1;
            index.observe(&stale);
        }
        let mut boundary = receipt("run_boundary".into(), "api:lint");
        boundary.ended_at_ms = 100;
        index.observe(&boundary);

        let mut lint = receipt("run_current".into(), "api:lint");
        lint.ended_at_ms = 10;
        let mut test = receipt("run_current".into(), "api:test");
        test.ended_at_ms = 11;
        index.observe(&lint);
        index.observe(&test);

        assert!(index.error().is_some());
        assert!(index.selected().is_none());
        assert_eq!(
            index.uncertain_latest_candidate,
            Some((100, "run_boundary".into()))
        );
    }

    #[test]
    fn duplicate_receipt_does_not_reopen_the_selected_complete_run() {
        let required = BTreeSet::from(["api:lint".parse().unwrap(), "api:test".parse().unwrap()]);
        let mut index = IndexedTargetReceipts::new(required);
        index.observe(&receipt("run_complete".into(), "api:lint"));
        index.observe(&receipt("run_complete".into(), "api:test"));

        index.observe(&receipt("run_complete".into(), "api:lint"));

        assert!(index.partial_by_run.is_empty());
        assert!(index.error().is_none());
        assert_eq!(index.selected().unwrap().run_id, "run_complete");
    }

    #[test]
    fn partial_duplicate_does_not_reopen_a_superseded_complete_run() {
        let required = BTreeSet::from(["api:lint".parse().unwrap(), "api:test".parse().unwrap()]);
        let mut index = IndexedTargetReceipts::new(required);
        index.observe(&receipt("run_old".into(), "api:lint"));
        index.observe(&receipt("run_old".into(), "api:test"));
        let mut current_lint = receipt("run_current".into(), "api:lint");
        current_lint.ended_at_ms = 10;
        let mut current_test = receipt("run_current".into(), "api:test");
        current_test.ended_at_ms = 11;
        index.observe(&current_lint);
        index.observe(&current_test);

        index.observe(&receipt("run_old".into(), "api:lint"));

        assert!(index.partial_by_run.is_empty());
        assert!(index.error().is_none());
        assert_eq!(index.selected().unwrap().run_id, "run_current");
    }

    #[test]
    fn archive_recovery_index_has_an_explicit_larger_memory_bound() {
        let required = BTreeSet::from(["api:lint".parse().unwrap(), "api:test".parse().unwrap()]);
        let index = IndexedTargetReceipts::for_archive(required);

        assert_eq!(
            index.max_incomplete_groups,
            MAX_ARCHIVE_INCOMPLETE_TARGET_RECEIPT_GROUPS
        );
        assert!(index.max_incomplete_groups > MAX_INCOMPLETE_TARGET_RECEIPT_GROUPS);
    }

    #[test]
    fn older_duplicate_receipt_does_not_replace_selected_target_evidence() {
        let required = BTreeSet::from(["api:lint".parse().unwrap(), "api:test".parse().unwrap()]);
        let mut index = IndexedTargetReceipts::new(required);
        let mut newer = receipt("run_complete".into(), "api:lint");
        newer.receipt_id = "receipt_newer".into();
        newer.ended_at_ms = 20;
        newer.exit_status = 0;
        index.observe(&newer);
        index.observe(&receipt("run_complete".into(), "api:test"));

        let mut older = receipt("run_complete".into(), "api:lint");
        older.receipt_id = "receipt_older".into();
        older.ended_at_ms = 10;
        older.exit_status = 1;
        index.observe(&older);

        let selected = &index.selected().unwrap().receipts[&"api:lint".parse().unwrap()];
        assert_eq!(selected.receipt_id, "receipt_newer");
        assert_eq!(selected.exit_status, 0);
        assert!(index.partial_by_run.is_empty());
    }

    #[test]
    fn older_duplicate_does_not_replace_partial_target_evidence() {
        let required = BTreeSet::from(["api:lint".parse().unwrap(), "api:test".parse().unwrap()]);
        let mut index = IndexedTargetReceipts::new(required);
        let mut newer = receipt("run_complete".into(), "api:lint");
        newer.receipt_id = "receipt_newer".into();
        newer.ended_at_ms = 20;
        newer.exit_status = 0;
        index.observe(&newer);

        let mut older = receipt("run_complete".into(), "api:lint");
        older.receipt_id = "receipt_older".into();
        older.ended_at_ms = 10;
        older.exit_status = 1;
        index.observe(&older);
        index.observe(&receipt("run_complete".into(), "api:test"));

        let selected = &index.selected().unwrap().receipts[&"api:lint".parse().unwrap()];
        assert_eq!(selected.receipt_id, "receipt_newer");
        assert_eq!(selected.exit_status, 0);
    }
}
