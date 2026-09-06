use super::*;

pub(super) fn retained_plan(
    context: &RepoContext,
    id: RecorderEpochId,
    observed_at_ms: u64,
    plans: &StreamSection<PlanFacts>,
    gates: &BTreeMap<String, GateFacts>,
    plan_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<PlanSnapshotResult, SourceError> {
    PlanObservationBasis {
        context,
        id,
        observed_at_ms,
        plans,
        retained_gates: Some(gates),
    }
    .plan(plan_id, cancelled)
}

pub(super) fn fresh_plan(
    context: &RepoContext,
    id: RecorderEpochId,
    plan_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<PlanSnapshotResult, SourceError> {
    let observed_at_ms = crate::state::now_ms();
    let plans = collect_plans(context, cancelled)?;
    PlanObservationBasis {
        context,
        id,
        observed_at_ms,
        plans: &plans,
        retained_gates: None,
    }
    .plan(plan_id, cancelled)
}

struct PlanObservationBasis<'a> {
    context: &'a RepoContext,
    id: RecorderEpochId,
    observed_at_ms: u64,
    plans: &'a StreamSection<PlanFacts>,
    retained_gates: Option<&'a BTreeMap<String, GateFacts>>,
}

impl PlanObservationBasis<'_> {
    fn plan(
        &self,
        plan_id: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PlanSnapshotResult, SourceError> {
        ensure_active(cancelled)?;
        if let Some(error) = &self.plans.error {
            return Err(SourceError::Collection {
                domain: CollectionDomain::Plans,
                message: error.message().to_string(),
            });
        }
        let Some(info) = self.plans.data.distinct.get(plan_id) else {
            return Ok(PlanSnapshotResult::NotFound);
        };
        let detail_observed_at_ms = crate::state::now_ms();
        let (decisions, decision_total, decision_error) =
            plan_decisions(self.context, plan_id, cancelled)?;
        let decisions_observed_at_ms = crate::state::now_ms();
        let uses_retained_open_gates = info.opened && !info.closed && self.retained_gates.is_some();
        let (receipts, receipt_total, receipt_indexes, receipt_error) = if uses_retained_open_gates
        {
            let result = read_receipts_reverse_with_cancellation(
                &self.context.state_file("receipts.jsonl"),
                LimitId::PlanReceipts.ceiling(),
                |receipt| receipt.plan_id.as_deref() == Some(plan_id),
                cancelled,
            );
            match result {
                Ok((records, _)) => (
                    records
                        .iter()
                        .map(plan_receipt)
                        .collect::<Result<Vec<_>, SourceError>>()?,
                    None,
                    None,
                    None,
                ),
                Err(error)
                    if crate::cancellation::is_status_collection_cancellation(&error)
                        || cancelled() =>
                {
                    return Err(SourceError::Cancelled);
                }
                Err(error) => (Vec::new(), None, None, Some(receipt_snapshot_error(error))),
            }
        } else {
            let plan_ids = vec![plan_id.to_string()];
            let indexes =
                crate::runtime::dashboard_gate_receipt_indexes(self.context, &plan_ids, cancelled)
                    .map_err(|error| {
                        collection_error_for(CollectionDomain::Gates, error, cancelled)
                    })?;
            let reduction = plan_receipts_and_indexes(self.context, plan_id, indexes, cancelled)?;
            (
                reduction.rows,
                reduction.total,
                reduction.indexes,
                reduction.error,
            )
        };
        let mut errors = Vec::new();
        errors.extend(receipt_error);
        let body = match read_plan_body(self.context, plan_id, cancelled) {
            Ok(body) => {
                let total = (!body.truncated).then(|| body.text.chars().count());
                Some(
                    BoundedText::for_limit(body.text, total, LimitId::PlanBodyChars)
                        .map_err(limit_error)?,
                )
            }
            Err(error) if crate::cancellation::is_status_collection_cancellation(&error) => {
                return Err(SourceError::Cancelled);
            }
            Err(error) => {
                errors.push(plan_body_error(plan_id, &error));
                None
            }
        };
        let (gates, gates_observed_at_ms) = if uses_retained_open_gates {
            let gate = self
                .retained_gates
                .and_then(|gates| gates.get(plan_id))
                .cloned()
                .unwrap_or_else(|| GateFacts {
                    error: Some("retained epoch omitted the requested plan's gates".to_string()),
                    ..GateFacts::default()
                });
            append_gate_error(&mut errors, plan_id, &gate);
            (gate.recorder, self.observed_at_ms)
        } else {
            let observed = crate::state::now_ms();
            let mut baselines = BTreeMap::new();
            baselines.insert(plan_id.to_string(), info.baseline.clone());
            let gate = if !info.closed
                && let Some(error) = self.plans.data.gate_errors.get(plan_id)
            {
                GateFacts {
                    error: Some(error.clone()),
                    ..GateFacts::default()
                }
            } else if let Some(indexes) = receipt_indexes {
                collect_gates(
                    self.context,
                    &baselines,
                    indexes.into_indexes(),
                    if info.closed { "closed" } else { "open" },
                    cancelled,
                )?
                .remove(plan_id)
                .unwrap_or_else(|| GateFacts {
                    error: Some(format!(
                        "Batched gate evaluation did not return requested plan '{plan_id}'"
                    )),
                    ..GateFacts::default()
                })
            } else {
                GateFacts {
                    error: Some(
                        "gate evidence is unavailable because receipt collection failed"
                            .to_string(),
                    ),
                    ..GateFacts::default()
                }
            };
            append_gate_error(&mut errors, plan_id, &gate);
            (gate.recorder, observed)
        };
        let decision_omitted = decision_total.saturating_sub(decisions.len());
        let receipt_omitted = receipt_total.map(|total| total.saturating_sub(receipts.len()));
        Ok(PlanSnapshotResult::Found(Box::new(PlanSnapshot {
            ok: true,
            command: UI_COMMAND.to_string(),
            schema_version: RECORDER_SCHEMA_VERSION,
            snapshot_kind: SnapshotKind::Plan,
            generated_at_ms: crate::state::now_ms(),
            basis_epoch: self.id,
            detail_observed_at_ms,
            gates_observed_at_ms,
            decisions_observed_at_ms,
            plan: info.summary(plan_id),
            body,
            gates,
            decisions,
            receipts,
            limits: PlanLimits {
                plan_decisions: root_limit(LimitId::PlanDecisions, Some(decision_omitted))
                    .map_err(limit_error)?,
                plan_receipts: root_limit(LimitId::PlanReceipts, receipt_omitted)
                    .map_err(limit_error)?,
            },
            errors: errors.into_iter().chain(decision_error).collect(),
        })))
    }
}

fn append_gate_error(errors: &mut Vec<SnapshotError>, plan_id: &str, gate: &GateFacts) {
    if let Some(error) = &gate.error {
        errors.push(SnapshotError::new(
            CollectionDomain::Gates,
            SnapshotErrorCode::GateObservationFailed,
            Some(plan_id.to_string()),
            error,
        ));
    }
}
