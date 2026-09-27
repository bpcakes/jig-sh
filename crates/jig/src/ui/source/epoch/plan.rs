use super::*;

pub(super) fn retained_plan(
    epoch: &LocalObservationEpoch,
    plan_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<PlanSnapshotResult, SourceError> {
    PlanObservationBasis {
        context: &epoch.context,
        id: epoch.id,
        retained_observed_at_ms: Some(epoch.observed_at_ms),
        plans: &epoch.plans,
    }
    .plan(plan_id, cancelled)
}

pub(super) fn fresh_plan(
    context: &RepoContext,
    id: RecorderEpochId,
    plan_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<PlanSnapshotResult, SourceError> {
    let plans = collect_plans(context, cancelled)?;
    PlanObservationBasis {
        context,
        id,
        retained_observed_at_ms: None,
        plans: &plans,
    }
    .plan(plan_id, cancelled)
}

struct PlanObservationBasis<'a> {
    context: &'a RepoContext,
    id: RecorderEpochId,
    /// Present when the detail is read against a retained recorder epoch.
    retained_observed_at_ms: Option<u64>,
    plans: &'a StreamSection<PlanFacts>,
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
        // An open plan read against a retained epoch keeps its bounded,
        // newest-first receipt read; other details count every plan receipt.
        let retained_open = info.opened && !info.closed && self.retained_observed_at_ms.is_some();
        let reduction = if retained_open {
            match read_receipts_reverse_with_cancellation(
                &self.context.state_file("receipts.jsonl"),
                LimitId::PlanReceipts.ceiling(),
                |receipt| receipt.plan_id.as_deref() == Some(plan_id),
                cancelled,
            ) {
                Ok((records, _)) => PlanReceiptReduction {
                    rows: records
                        .iter()
                        .map(plan_receipt)
                        .collect::<Result<Vec<_>, SourceError>>()?,
                    total: None,
                    error: None,
                },
                Err(error)
                    if crate::cancellation::is_status_collection_cancellation(&error)
                        || cancelled() =>
                {
                    return Err(SourceError::Cancelled);
                }
                Err(error) => PlanReceiptReduction {
                    rows: Vec::new(),
                    total: None,
                    error: Some(receipt_snapshot_error(error)),
                },
            }
        } else {
            plan_receipts(self.context, plan_id, cancelled)?
        };
        let mut errors = Vec::new();
        errors.extend(reduction.error);
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
        let decision_omitted = decision_total.saturating_sub(decisions.len());
        let receipt_omitted = reduction
            .total
            .map(|total| total.saturating_sub(reduction.rows.len()));
        Ok(PlanSnapshotResult::Found(Box::new(PlanSnapshot {
            ok: true,
            command: UI_COMMAND.to_string(),
            schema_version: RECORDER_SCHEMA_VERSION,
            snapshot_kind: SnapshotKind::Plan,
            generated_at_ms: crate::state::now_ms(),
            basis_epoch: self.id,
            detail_observed_at_ms,
            // Gate evaluation was removed with `jig work`; the documented
            // fields remain present, and `gates` is always null.
            gates_observed_at_ms: self
                .retained_observed_at_ms
                .unwrap_or(detail_observed_at_ms),
            decisions_observed_at_ms,
            plan: info.summary(plan_id),
            body,
            gates: None,
            decisions,
            receipts: reduction.rows,
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
