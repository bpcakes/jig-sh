use std::collections::BTreeMap;

use anyhow::Result;
use jig_contract::TargetId;
use serde_json::{Value, json};

use crate::context::{WorkEvidenceGate, WorkEvidenceSelector};
use crate::repository::{RepositoryCatalog, resolve_evidence_targets, target_input_digest};
use crate::state::{CurrentWorktreeFingerprint, TargetReceiptStatus, WorkGateReceiptIndex};

use super::{EvaluatedReceipt, GateCollection, GateFreshness, GateOutcome, GateReceiptView};

#[derive(Clone, Debug)]
struct TargetEvidenceEvaluation {
    target: TargetId,
    run_id: Option<String>,
    outcome: GateOutcome,
    receipt: EvaluatedReceipt,
    config_digest: Option<String>,
    expected_config_digest: String,
    input_digest: Option<String>,
    expected_input_digest: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct EvidenceGateEvaluation {
    id: String,
    required: bool,
    selector: WorkEvidenceSelector,
    conclusion: &'static str,
    run_id: Option<String>,
    outcome: GateOutcome,
    freshness: GateFreshness,
    index_error: Option<String>,
    targets: Vec<TargetEvidenceEvaluation>,
}

impl TargetEvidenceEvaluation {
    fn to_value(&self) -> Value {
        let receipt = &self.receipt;
        json!({
            "target": self.target,
            "status": self.outcome.as_str(),
            "receipt_id": receipt.receipt_id,
            "run_id": self.run_id,
            "exit_status": receipt.exit_status,
            "ended_at_ms": receipt.ended_at_ms,
            "config_digest": self.config_digest,
            "expected_config_digest": self.expected_config_digest,
            "input_digest": self.input_digest,
            "expected_input_digest": self.expected_input_digest,
            "freshness": receipt.freshness.as_str(),
            "freshness_reason": receipt.freshness_reason,
            "changed_paths": receipt.changed_paths,
            "changed_path_count": receipt.changed_path_count,
            "changed_paths_truncated": receipt.changed_paths_truncated,
            "changed_paths_digest": receipt.changed_paths_digest,
            "diff_summary": receipt.diff_summary,
            "receipt_worktree_fingerprint_error": receipt.receipt_worktree_fingerprint_error,
            "current_worktree_fingerprint_error": receipt.current_worktree_fingerprint_error,
            "valid_until_ms": receipt.valid_until_ms,
            "requires_time_validity": receipt.requires_time_validity,
        })
    }
}

impl EvidenceGateEvaluation {
    pub(super) fn evaluate(
        gate: &WorkEvidenceGate,
        catalog: &RepositoryCatalog,
        current_fingerprint: &CurrentWorktreeFingerprint,
        receipt_index: &WorkGateReceiptIndex,
        collection: GateCollection<'_>,
    ) -> Result<Self> {
        let required_targets = resolve_evidence_targets(catalog, &gate.selector)?;
        let index_error = receipt_index
            .target_receipt_error(&gate.id)
            .map(str::to_owned);
        let group = index_error
            .is_none()
            .then(|| receipt_index.target_receipts(&gate.id))
            .flatten();
        let expected_config_digest = catalog.config_digest().to_owned();
        let expected_input_digests = required_targets
            .iter()
            .map(|target| {
                let digest = current_fingerprint
                    .fingerprint
                    .as_deref()
                    .map(|fingerprint| target_input_digest(catalog, target, fingerprint))
                    .transpose()?;
                Ok((target.clone(), digest))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let mut targets = Vec::with_capacity(required_targets.len());
        for target in required_targets {
            collection.ensure_active()?;
            let receipt = group.and_then(|group| group.receipts.get(&target));
            let expected_input_digest =
                expected_input_digests.get(&target).cloned().unwrap_or(None);
            let (freshness, freshness_reason) = index_error.as_ref().map_or_else(
                || {
                    target_evidence_freshness(
                        receipt,
                        &expected_config_digest,
                        expected_input_digest.as_deref(),
                        current_fingerprint,
                    )
                },
                |error| (GateFreshness::Unknown, error.clone()),
            );
            let evaluated_receipt = EvaluatedReceipt::with_freshness(
                receipt,
                receipt,
                current_fingerprint,
                freshness,
                freshness_reason,
            );
            let outcome = match (index_error.as_ref(), receipt) {
                (Some(_), _) => GateOutcome::Unknown,
                (None, Some(receipt)) if receipt.exit_status != 0 => GateOutcome::Failed,
                (None, Some(_)) => freshness.as_gate_outcome(),
                (None, None) => GateOutcome::Missing,
            };
            targets.push(TargetEvidenceEvaluation {
                target,
                run_id: receipt.map(|receipt| receipt.run_id.clone()),
                outcome,
                receipt: evaluated_receipt,
                config_digest: receipt.and_then(|receipt| receipt.config_digest.clone()),
                expected_config_digest: expected_config_digest.clone(),
                input_digest: receipt.and_then(|receipt| receipt.input_digest.clone()),
                expected_input_digest,
            });
        }

        let freshness = aggregate_evidence_freshness(&targets);
        let outcome = aggregate_evidence_outcome(&targets);
        Ok(Self {
            id: gate.id.clone(),
            required: gate.required,
            selector: gate.selector.clone(),
            conclusion: gate.conclusion,
            run_id: group.map(|group| group.run_id.clone()),
            outcome,
            freshness,
            index_error,
            targets,
        })
    }

    pub(super) fn id(&self) -> &str {
        &self.id
    }

    pub(super) const fn required(&self) -> bool {
        self.required
    }

    pub(super) const fn outcome(&self) -> GateOutcome {
        self.outcome
    }

    pub(super) fn receipt(&self) -> Option<&EvaluatedReceipt> {
        self.targets
            .iter()
            .filter_map(|target| {
                target
                    .receipt
                    .ended_at_ms
                    .map(|ended| (ended, &target.receipt))
            })
            .max_by_key(|(ended, _)| *ended)
            .map(|(_, receipt)| receipt)
    }

    pub(super) fn evidence_key(&self) -> String {
        match &self.selector {
            WorkEvidenceSelector::Target(target) => format!("target:{target}"),
            WorkEvidenceSelector::Profile(profile) => format!("profile:{profile}"),
        }
    }

    pub(super) fn to_value(&self) -> Value {
        let (target, profile) = self.selector_values();
        let mut value = json!({
            "id": self.id,
            "kind": "evidence",
            "required": self.required,
            "target": target,
            "profile": profile,
            "conclusion": self.conclusion,
            "status": self.outcome.as_str(),
            "run_id": self.run_id,
            "freshness": self.freshness.as_str(),
            "freshness_reason": evidence_freshness_reason(self.freshness),
            "targets": self.targets.iter().map(TargetEvidenceEvaluation::to_value).collect::<Vec<_>>(),
        });
        if let Some(error) = &self.index_error {
            value["index_error"] = Value::String(error.clone());
        }
        value
    }

    pub(super) fn to_latest_evidence(&self) -> Option<Value> {
        if self.index_error.is_some()
            || self.targets.is_empty()
            || self.targets.iter().any(|target| {
                target.receipt.exit_status != Some(0)
                    || target.receipt.freshness != GateFreshness::Fresh
            })
        {
            return None;
        }
        let (target, profile) = self.selector_values();
        let receipt_ids = self
            .targets
            .iter()
            .filter_map(|target| target.receipt.receipt_id.as_deref())
            .collect::<Vec<_>>();
        let receipt = self.receipt()?;
        let valid_until_ms = self
            .targets
            .iter()
            .filter_map(|target| target.receipt.valid_until_ms)
            .min();
        let requires_time_validity = self
            .targets
            .iter()
            .any(|target| target.receipt.requires_time_validity);
        Some(json!({
            "tool": null,
            "skill": null,
            "target": target,
            "profile": profile,
            "gate_id": self.id,
            "status": self.outcome.as_str(),
            "run_id": self.run_id,
            "receipt_id": receipt.receipt_id,
            "receipt_ids": receipt_ids,
            "freshness_receipt_id": null,
            "matches_current_worktree": self.freshness == GateFreshness::Fresh,
            "freshness": self.freshness.as_str(),
            "freshness_reason": evidence_freshness_reason(self.freshness),
            "changed_paths": receipt.changed_paths,
            "changed_path_count": receipt.changed_path_count,
            "changed_paths_truncated": receipt.changed_paths_truncated,
            "changed_paths_digest": receipt.changed_paths_digest,
            "diff_summary": receipt.diff_summary,
            "ended_at_ms": receipt.ended_at_ms.unwrap_or(0),
            "valid_until_ms": valid_until_ms,
            "requires_time_validity": requires_time_validity,
            "targets": self.targets.iter().map(TargetEvidenceEvaluation::to_value).collect::<Vec<_>>(),
        }))
    }

    fn selector_values(&self) -> (Option<String>, Option<String>) {
        match &self.selector {
            WorkEvidenceSelector::Target(target) => (Some(target.to_string()), None),
            WorkEvidenceSelector::Profile(profile) => (None, Some(profile.to_string())),
        }
    }
}

fn target_evidence_freshness(
    receipt: Option<&TargetReceiptStatus>,
    expected_config_digest: &str,
    expected_input_digest: Option<&str>,
    current_fingerprint: &CurrentWorktreeFingerprint,
) -> (GateFreshness, String) {
    let Some(receipt) = receipt else {
        return (
            GateFreshness::Missing,
            "no receipt exists for this target in a compatible run".into(),
        );
    };
    if !crate::state::time_validity_is_current(
        receipt.valid_until_ms,
        receipt.requires_time_validity,
        crate::state::now_ms(),
    ) {
        return if receipt.valid_until_ms.is_some() {
            (
                GateFreshness::Stale,
                "receipt time validity has expired".into(),
            )
        } else {
            (
                GateFreshness::Unknown,
                "receipt requires time validity but recorded no boundary".into(),
            )
        };
    }
    if receipt
        .config_digest
        .as_deref()
        .is_some_and(|digest| digest != expected_config_digest)
    {
        return (
            GateFreshness::Stale,
            "receipt was recorded for a different repository configuration".into(),
        );
    }
    if let (Some(recorded), Some(expected)) =
        (receipt.input_digest.as_deref(), expected_input_digest)
        && recorded != expected
    {
        return (
            GateFreshness::Stale,
            "receipt input digest does not match the current target input digest".into(),
        );
    }
    if let (Some(recorded), Some(current)) = (
        receipt.worktree_fingerprint.as_deref(),
        current_fingerprint.fingerprint.as_deref(),
    ) && recorded != current
    {
        return (
            GateFreshness::Stale,
            "receipt was recorded for a different worktree fingerprint".into(),
        );
    }
    if receipt.config_digest.is_none() {
        return (
            GateFreshness::Unknown,
            "receipt did not record a repository configuration digest".into(),
        );
    }
    if receipt.input_digest.is_none() {
        return (
            GateFreshness::Unknown,
            "receipt did not record a target input digest".into(),
        );
    }
    if receipt.worktree_fingerprint.is_none() {
        return (
            GateFreshness::Unknown,
            "receipt did not record a worktree fingerprint".into(),
        );
    }
    if expected_input_digest.is_none() || current_fingerprint.fingerprint.is_none() {
        return (
            GateFreshness::Unknown,
            "current target freshness could not be determined".into(),
        );
    }
    (
        GateFreshness::Fresh,
        "receipt matches the current target inputs".into(),
    )
}

fn aggregate_evidence_freshness(targets: &[TargetEvidenceEvaluation]) -> GateFreshness {
    if targets
        .iter()
        .any(|target| target.receipt.freshness == GateFreshness::Missing)
    {
        GateFreshness::Missing
    } else if targets
        .iter()
        .any(|target| target.receipt.freshness == GateFreshness::Stale)
    {
        GateFreshness::Stale
    } else if targets
        .iter()
        .any(|target| target.receipt.freshness == GateFreshness::Unknown)
    {
        GateFreshness::Unknown
    } else {
        GateFreshness::Fresh
    }
}

const fn evidence_freshness_reason(freshness: GateFreshness) -> &'static str {
    match freshness {
        GateFreshness::Fresh => "all required target receipts match current inputs",
        GateFreshness::Missing => {
            "one or more required target receipts are missing from a compatible run"
        }
        GateFreshness::Stale => "one or more required target receipts are stale",
        GateFreshness::Unknown => "freshness is unknown for one or more required target receipts",
    }
}

fn aggregate_evidence_outcome(targets: &[TargetEvidenceEvaluation]) -> GateOutcome {
    for outcome in [
        GateOutcome::Failed,
        GateOutcome::Missing,
        GateOutcome::Stale,
        GateOutcome::Unknown,
    ] {
        if targets.iter().any(|target| target.outcome == outcome) {
            return outcome;
        }
    }
    GateOutcome::Passed
}

impl GateReceiptView for TargetReceiptStatus {
    fn receipt_id(&self) -> &str {
        &self.receipt_id
    }

    fn exit_status(&self) -> i32 {
        self.exit_status
    }

    fn ended_at_ms(&self) -> u64 {
        self.ended_at_ms
    }

    fn changed_paths(&self) -> &[String] {
        &self.changed_paths
    }

    fn changed_path_count(&self) -> usize {
        self.changed_path_count
    }

    fn changed_paths_truncated(&self) -> bool {
        self.changed_paths_truncated
    }

    fn changed_paths_digest(&self) -> Option<&str> {
        self.changed_paths_digest.as_deref()
    }

    fn diff_summary(&self) -> &str {
        &self.diff_summary
    }

    fn worktree_fingerprint(&self) -> Option<&str> {
        self.worktree_fingerprint.as_deref()
    }

    fn worktree_fingerprint_error(&self) -> Option<&str> {
        self.worktree_fingerprint_error.as_deref()
    }

    fn valid_until_ms(&self) -> Option<u64> {
        self.valid_until_ms
    }

    fn requires_time_validity(&self) -> bool {
        self.requires_time_validity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target_receipt(
        valid_until_ms: Option<u64>,
        requires_time_validity: bool,
    ) -> TargetReceiptStatus {
        TargetReceiptStatus {
            receipt_id: "receipt_target".into(),
            run_id: "run_target".into(),
            target: "repo:file-budget".parse().unwrap(),
            config_digest: Some("sha256:config".into()),
            input_digest: Some("sha256:input".into()),
            exit_status: 0,
            ended_at_ms: 1,
            changed_paths: Vec::new(),
            changed_path_count: 0,
            changed_paths_truncated: false,
            changed_paths_digest: None,
            diff_summary: String::new(),
            worktree_fingerprint: Some("fingerprint".into()),
            worktree_fingerprint_error: None,
            valid_until_ms,
            requires_time_validity,
        }
    }

    #[test]
    fn target_evidence_enforces_time_validity_before_source_identity() {
        let current = CurrentWorktreeFingerprint {
            fingerprint: Some("fingerprint".into()),
            error: None,
        };
        let (expired, reason) = target_evidence_freshness(
            Some(&target_receipt(Some(0), true)),
            "sha256:config",
            Some("sha256:input"),
            &current,
        );
        assert_eq!(expired, GateFreshness::Stale);
        assert!(reason.contains("expired"));

        let (missing, reason) = target_evidence_freshness(
            Some(&target_receipt(None, true)),
            "sha256:config",
            Some("sha256:input"),
            &current,
        );
        assert_eq!(missing, GateFreshness::Unknown);
        assert!(reason.contains("no boundary"));
    }
}
