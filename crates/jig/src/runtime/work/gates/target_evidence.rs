use std::collections::BTreeMap;

use anyhow::Result;
use jig_contract::TargetId;
use jig_contract::freshness::{
    EffectiveTimeValidityV1, FreshnessCollectionStats, FreshnessDetailsV1, FreshnessReasons,
    FreshnessSummaryV1, TargetFreshness, TargetFreshnessStatus,
};
use serde_json::{Value, json};

use crate::context::{WorkEvidenceGate, WorkEvidenceSelector};
use crate::repository::{RepositoryCatalog, resolve_evidence_targets, target_input_digest};
use crate::state::{CurrentWorktreeFingerprint, TargetReceiptStatus, WorkGateReceiptIndex};

use super::{EvaluatedReceipt, GateCollection, GateFreshness, GateOutcome, GateReceiptView};

#[derive(Clone, Debug)]
struct TargetEvidenceEvaluation {
    scoped: Option<TargetFreshness>,
    target: TargetId,
    run_id: Option<String>,
    started_at_ms: Option<u64>,
    outcome: GateOutcome,
    receipt: EvaluatedReceipt,
    config_digest: Option<String>,
    expected_config_digest: String,
    input_digest: Option<String>,
    expected_input_digest: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct EvidenceGateEvaluation {
    freshness_collection: Option<FreshnessCollectionStats>,
    id: String,
    required: bool,
    selector: WorkEvidenceSelector,
    conclusion: &'static str,
    run_id: Option<String>,
    outcome: GateOutcome,
    freshness: GateFreshness,
    targets: Vec<TargetEvidenceEvaluation>,
}

impl TargetEvidenceEvaluation {
    fn to_value(&self) -> Value {
        let receipt = &self.receipt;
        let mut value = json!({
            "target": self.target,
            "status": self.outcome.as_str(),
            "receipt_id": receipt.receipt_id,
            "run_id": self.run_id,
            "exit_status": receipt.exit_status,
            "started_at_ms": self.started_at_ms,
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
        });
        if let Some(scoped) = &self.scoped {
            extend_fields(&mut value, &FreshnessDetailsV1::from(scoped));
        }
        value
    }

    fn status_view(&self) -> jig_ui::dashboard::StatusEvidenceTarget {
        let receipt = &self.receipt;
        jig_ui::dashboard::StatusEvidenceTarget {
            scoped_freshness: self.scoped.as_ref().map(FreshnessDetailsV1::from),
            target: self.target.clone(),
            status: self.outcome.as_str().to_string(),
            receipt_id: receipt.receipt_id.clone(),
            run_id: self.run_id.clone(),
            exit_status: receipt.exit_status,
            ended_at_ms: receipt.ended_at_ms,
            config_digest: self.config_digest.clone(),
            expected_config_digest: self.expected_config_digest.clone(),
            input_digest: self.input_digest.clone(),
            expected_input_digest: self.expected_input_digest.clone(),
            freshness: receipt.freshness.as_str().to_string(),
            freshness_reason: receipt.freshness_reason.clone(),
            changed_paths: receipt.changed_paths.clone(),
            changed_path_count: receipt.changed_path_count,
            changed_paths_truncated: receipt.changed_paths_truncated,
            changed_paths_digest: receipt.changed_paths_digest.clone(),
            diff_summary: receipt.diff_summary.clone(),
            receipt_worktree_fingerprint_error: receipt.receipt_worktree_fingerprint_error.clone(),
            current_worktree_fingerprint_error: receipt.current_worktree_fingerprint_error.clone(),
            valid_until_ms: receipt.valid_until_ms,
            requires_time_validity: receipt.requires_time_validity,
        }
    }
}

impl EvidenceGateEvaluation {
    pub(super) fn collection_stats(&self) -> Option<&FreshnessCollectionStats> {
        self.freshness_collection.as_ref()
    }

    pub(super) fn evaluate(
        gate: &WorkEvidenceGate,
        catalog: &RepositoryCatalog,
        current_fingerprint: &CurrentWorktreeFingerprint,
        receipt_index: &WorkGateReceiptIndex,
        collection: GateCollection<'_>,
        scoped: Option<&super::scoped_freshness::ScopedGateFreshness>,
    ) -> Result<Self> {
        let required_targets = resolve_evidence_targets(catalog, &gate.selector)?;
        let selected = receipt_index.target_receipts(&gate.id);
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
            let receipt = selected.and_then(|receipts| receipts.get(&target));
            let expected_input_digest =
                expected_input_digests.get(&target).cloned().unwrap_or(None);
            let scoped_target = scoped
                .and_then(|scoped| scoped.targets.get(&target))
                .cloned();
            let (freshness, freshness_reason) = if let Some(scoped) = &scoped_target {
                let freshness = GateFreshness::from(scoped.status);
                (freshness, scoped_freshness_reason(scoped).to_owned())
            } else {
                target_evidence_freshness(
                    receipt,
                    &expected_config_digest,
                    expected_input_digest.as_deref(),
                    current_fingerprint,
                )
            };
            let evaluated_receipt = EvaluatedReceipt::with_freshness(
                receipt,
                receipt,
                current_fingerprint,
                freshness,
                freshness_reason,
            );
            let outcome = match receipt {
                Some(receipt) if receipt.exit_status != 0 => GateOutcome::Failed,
                Some(_) => freshness.as_gate_outcome(),
                None => GateOutcome::Missing,
            };
            targets.push(TargetEvidenceEvaluation {
                scoped: scoped_target,
                target,
                run_id: receipt.and_then(|receipt| receipt.run_id.clone()),
                started_at_ms: receipt.map(|receipt| receipt.started_at_ms),
                outcome,
                receipt: evaluated_receipt,
                config_digest: receipt.and_then(|receipt| receipt.config_digest.clone()),
                expected_config_digest: expected_config_digest.clone(),
                input_digest: receipt.and_then(|receipt| receipt.input_digest.clone()),
                expected_input_digest,
            });
        }

        // A dependent proof cannot outlive a failed, missing, stale, or newer
        // dependency result. Iterate to carry invalidity through the graph.
        while scoped.is_none() {
            let invalidate: Vec<_> = targets
                .iter()
                .enumerate()
                .filter_map(|(index, target)| {
                    if target.outcome != GateOutcome::Passed {
                        return None;
                    }
                    let action = catalog.action(&target.target)?;
                    let invalid_dependency = action.depends_on.iter().any(|dependency| {
                        targets
                            .iter()
                            .find(|entry| &entry.target == dependency)
                            .is_none_or(|entry| {
                                entry.outcome != GateOutcome::Passed
                                    || entry.receipt.ended_at_ms > target.started_at_ms
                            })
                    });
                    invalid_dependency.then_some(index)
                })
                .collect();
            if invalidate.is_empty() {
                break;
            }
            for index in invalidate {
                targets[index].outcome = GateOutcome::Stale;
                targets[index].receipt.freshness = GateFreshness::Stale;
                targets[index].receipt.freshness_reason =
                    "a required dependency has missing, nonpassing, stale or newer evidence".into();
            }
        }
        let freshness = aggregate_evidence_freshness(&targets);
        let outcome = aggregate_evidence_outcome(&targets);
        let run_id = targets
            .first()
            .and_then(|first| first.run_id.as_ref())
            .filter(|run_id| {
                targets
                    .iter()
                    .all(|target| target.run_id.as_ref() == Some(*run_id))
            })
            .cloned();
        Ok(Self {
            freshness_collection: scoped.map(|scoped| scoped.stats.clone()),
            id: gate.id.clone(),
            required: gate.required,
            selector: gate.selector.clone(),
            conclusion: gate.conclusion,
            run_id,
            outcome,
            freshness,
            targets,
        })
    }

    pub(super) fn check_targets(&self) -> impl Iterator<Item = (TargetId, bool, Value)> + '_ {
        self.targets.iter().map(|target| {
            (
                target.target.clone(),
                target.outcome == GateOutcome::Passed
                    && target.receipt.freshness == GateFreshness::Fresh,
                target.to_value(),
            )
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
            "freshness_reason": self.freshness_reason(),
            "targets": self.targets.iter().map(TargetEvidenceEvaluation::to_value).collect::<Vec<_>>(),
        });
        self.extend_summary(&mut value);
        value
    }

    pub(super) fn status_view(&self) -> jig_ui::dashboard::StatusEvidenceGate {
        let (target, profile) = match &self.selector {
            WorkEvidenceSelector::Target(target) => (Some(target.to_string()), None),
            WorkEvidenceSelector::Profile(profile) => (None, Some(profile.to_string())),
        };
        jig_ui::dashboard::StatusEvidenceGate {
            scoped_freshness: self.scoped_summary(),
            freshness_collection: self.freshness_collection.clone(),
            id: self.id.clone(),
            required: self.required,
            target,
            profile,
            conclusion: self.conclusion.to_string(),
            status: self.outcome.as_str().to_string(),
            run_id: self.run_id.clone(),
            freshness: self.freshness.as_str().to_string(),
            freshness_reason: self.freshness_reason(),
            targets: self
                .targets
                .iter()
                .map(TargetEvidenceEvaluation::status_view)
                .collect(),
            index_error: None,
        }
    }

    pub(super) fn to_latest_evidence(&self) -> Option<Value> {
        if self.targets.is_empty()
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
        let mut value = json!({
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
            "freshness_reason": self.freshness_reason(),
            "changed_paths": receipt.changed_paths,
            "changed_path_count": receipt.changed_path_count,
            "changed_paths_truncated": receipt.changed_paths_truncated,
            "changed_paths_digest": receipt.changed_paths_digest,
            "diff_summary": receipt.diff_summary,
            "ended_at_ms": receipt.ended_at_ms.unwrap_or(0),
            "valid_until_ms": valid_until_ms,
            "requires_time_validity": requires_time_validity,
            "targets": self.targets.iter().map(TargetEvidenceEvaluation::to_value).collect::<Vec<_>>(),
        });
        self.extend_summary(&mut value);
        Some(value)
    }

    pub(super) fn effective_time_validity(&self) -> (Option<u64>, bool) {
        if let Some(summary) = self.scoped_summary() {
            (
                summary.effective_valid_until_ms,
                summary.effective_requires_time_validity,
            )
        } else {
            (
                self.targets
                    .iter()
                    .filter_map(|target| target.receipt.valid_until_ms)
                    .min(),
                self.targets
                    .iter()
                    .any(|target| target.receipt.requires_time_validity),
            )
        }
    }

    fn scoped_summary(&self) -> Option<FreshnessSummaryV1> {
        self.freshness_collection.as_ref()?;
        let mut reasons = FreshnessReasons::default();
        let mut time = EffectiveTimeValidityV1::default();
        for target in &self.targets {
            if let Some(scoped) = &target.scoped {
                for reason in &scoped.reasons.reasons {
                    let mut reason = reason.clone();
                    if reason.target.is_none() {
                        reason.target = Some(target.target.clone());
                    }
                    reasons.push(reason);
                }
                reasons.reasons_total = reasons.reasons_total.saturating_add(
                    scoped
                        .reasons
                        .reasons_total
                        .saturating_sub(scoped.reasons.reasons.len() as u64),
                );
                reasons.reasons_truncated |= scoped.reasons.reasons_truncated;
                time = time.combine(EffectiveTimeValidityV1::new(
                    scoped.effective_valid_until_ms,
                    scoped.effective_requires_time_validity,
                ));
            }
        }
        Some(FreshnessSummaryV1::new(
            reasons,
            time.effective_valid_until_ms,
            time.effective_requires_time_validity,
        ))
    }

    fn extend_summary(&self, value: &mut Value) {
        if let Some(summary) = self.scoped_summary() {
            extend_fields(value, &summary);
            value["freshness_collection"] = json!(self.freshness_collection);
        }
    }

    fn freshness_reason(&self) -> String {
        if let Some(stats) = &self.freshness_collection
            && self
                .targets
                .iter()
                .filter_map(|target| target.scoped.as_ref())
                .any(|target| {
                    target.reasons.reasons.iter().any(|reason| {
                        reason.code == jig_contract::freshness::FreshnessReasonCode::CollectionLimit
                    })
                })
        {
            return collection_limit_reason(stats);
        }
        evidence_freshness_reason(self.freshness).to_owned()
    }

    fn selector_values(&self) -> (Option<String>, Option<String>) {
        match &self.selector {
            WorkEvidenceSelector::Target(target) => (Some(target.to_string()), None),
            WorkEvidenceSelector::Profile(profile) => (None, Some(profile.to_string())),
        }
    }
}

fn collection_limit_reason(stats: &FreshnessCollectionStats) -> String {
    let remedy = if stats.timeout_ms == 2_000
        && stats.elapsed_us >= stats.timeout_ms.saturating_mul(1_000)
    {
        " For a deadline limit, rerun this inspection with --freshness-timeout-ms 30000. Resource ceilings are unchanged."
    } else {
        " A larger timeout does not raise entry, byte, graph, depth, or record limits."
    };
    format!(
        "Freshness collection reached a time or resource limit (budget {} ms).{remedy}",
        stats.timeout_ms
    )
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
            "no receipt exists for this target in this work plan".into(),
        );
    };
    if receipt.run_id.as_deref().is_none_or(str::is_empty) {
        return (
            GateFreshness::Unknown,
            "receipt did not record an original run identity".into(),
        );
    }
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
        .any(|target| target.receipt.freshness == GateFreshness::Unsupported)
    {
        GateFreshness::Unsupported
    } else if targets
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
        GateFreshness::Missing => "one or more required targets have no receipt in this work plan",
        GateFreshness::Stale => "one or more required target receipts are stale",
        GateFreshness::Unknown => "freshness is unknown for one or more required target receipts",
        GateFreshness::Unsupported => {
            "one or more required target receipts need a compatible freshness reader"
        }
    }
}

fn aggregate_evidence_outcome(targets: &[TargetEvidenceEvaluation]) -> GateOutcome {
    for outcome in [
        GateOutcome::Failed,
        GateOutcome::Unsupported,
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

pub(super) fn extend_fields(value: &mut Value, fields: &impl serde::Serialize) {
    let Value::Object(fields) =
        serde_json::to_value(fields).expect("typed freshness metadata serializes")
    else {
        unreachable!()
    };
    value
        .as_object_mut()
        .expect("evidence is an object")
        .extend(fields);
}

pub(super) fn unsupported_reference_summary(
    ctx: &crate::context::RepoContext,
    gate: &WorkEvidenceGate,
) -> Option<FreshnessSummaryV1> {
    use jig_contract::freshness::{
        FreshnessReason, FreshnessReasonCode, TARGET_FRESHNESS_CONTRACT_VERSION,
    };
    (ctx.contract_version() >= TARGET_FRESHNESS_CONTRACT_VERSION).then(|| {
        let target = match &gate.selector {
            WorkEvidenceSelector::Target(target) => Some(target.clone()),
            WorkEvidenceSelector::Profile(_) => None,
        };
        FreshnessSummaryV1::new(
            FreshnessReasons::one(FreshnessReason {
                code: FreshnessReasonCode::UnsupportedReference,
                target,
                path: None,
            }),
            None,
            false,
        )
    })
}

impl From<TargetFreshnessStatus> for GateFreshness {
    fn from(status: TargetFreshnessStatus) -> Self {
        match status {
            TargetFreshnessStatus::Fresh => Self::Fresh,
            TargetFreshnessStatus::Unknown => Self::Unknown,
            TargetFreshnessStatus::Stale => Self::Stale,
            TargetFreshnessStatus::Missing => Self::Missing,
            TargetFreshnessStatus::Unsupported => Self::Unsupported,
        }
    }
}

fn scoped_freshness_reason(result: &TargetFreshness) -> &'static str {
    match result.status {
        TargetFreshnessStatus::Fresh => {
            "original receipt matches current target authority and valid dependency proof"
        }
        TargetFreshnessStatus::Missing => {
            "no original receipt exists for this target in this work plan"
        }
        TargetFreshnessStatus::Unsupported => {
            "receipt authority requires a compatible freshness reader"
        }
        TargetFreshnessStatus::Stale => {
            "target authority changed or its effective time validity expired"
        }
        TargetFreshnessStatus::Unknown => {
            "original target authority or dependency execution proof could not be verified"
        }
    }
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
mod tests;
