use super::*;

pub(in crate::runtime::work) fn evaluate_targets(
    catalog: &RepositoryCatalog,
    current_fingerprint: &CurrentWorktreeFingerprint,
    selected: Option<&BTreeMap<TargetId, TargetReceiptStatus>>,
    required_targets: impl IntoIterator<Item = TargetId>,
    collection: GateCollection<'_>,
    scoped: Option<&super::super::scoped_freshness::ScopedGateFreshness>,
) -> Result<Vec<TargetEvidenceEvaluation>> {
    let required_targets = required_targets.into_iter().collect::<Vec<_>>();
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
        let expected_input_digest = expected_input_digests.get(&target).cloned().unwrap_or(None);
        let scoped_target = scoped
            .and_then(|scoped| scoped.targets.get(&target))
            .cloned();
        let current_authority = scoped
            .and_then(|scoped| scoped.current_authority.get(&target))
            .copied()
            .unwrap_or_else(|| {
                if expected_input_digest.is_some() {
                    super::super::scoped_freshness::CurrentTargetAuthority::Available
                } else {
                    super::super::scoped_freshness::CurrentTargetAuthority::Unavailable
                }
            });
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
            original_plan_id: receipt.and_then(|receipt| receipt.plan_id.clone()),
            started_at_ms: receipt.map(|receipt| receipt.started_at_ms),
            outcome,
            receipt: evaluated_receipt,
            config_digest: receipt.and_then(|receipt| receipt.config_digest.clone()),
            expected_config_digest: expected_config_digest.clone(),
            input_digest: receipt.and_then(|receipt| receipt.input_digest.clone()),
            expected_input_digest,
            current_authority,
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
    Ok(targets)
}
