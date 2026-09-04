impl EvaluatedReceipt {
    fn new<T: GateReceiptView>(
        receipt: Option<&T>,
        freshness_receipt: Option<&T>,
        current_fingerprint: &crate::state::CurrentWorktreeFingerprint,
    ) -> Self {
        let freshness = gate_freshness(freshness_receipt, current_fingerprint);
        let freshness_reason = freshness
            .reason(freshness_receipt, current_fingerprint)
            .to_string();
        Self::with_freshness(
            receipt,
            freshness_receipt,
            current_fingerprint,
            freshness,
            freshness_reason,
        )
    }

    fn with_freshness<T: GateReceiptView>(
        receipt: Option<&T>,
        freshness_receipt: Option<&T>,
        current_fingerprint: &crate::state::CurrentWorktreeFingerprint,
        freshness: GateFreshness,
        freshness_reason: String,
    ) -> Self {
        let (changed_paths, changed_path_count, changed_paths_truncated, changed_paths_digest) =
            gate_changed_paths(freshness_receipt);
        Self {
            receipt_id: receipt.map(|receipt| receipt.receipt_id().to_string()),
            freshness_receipt_id: freshness_receipt.map(|receipt| receipt.receipt_id().to_string()),
            exit_status: receipt.map(GateReceiptView::exit_status),
            ended_at_ms: receipt.map(GateReceiptView::ended_at_ms),
            freshness,
            freshness_reason,
            changed_paths,
            changed_path_count,
            changed_paths_truncated,
            changed_paths_digest,
            diff_summary: freshness_receipt.map(|receipt| receipt.diff_summary().to_string()),
            receipt_worktree_fingerprint_error: freshness_receipt
                .and_then(GateReceiptView::worktree_fingerprint_error)
                .map(str::to_string),
            current_worktree_fingerprint_error: current_fingerprint.error.clone(),
            valid_until_ms: freshness_receipt.and_then(GateReceiptView::valid_until_ms),
            requires_time_validity: freshness_receipt
                .is_some_and(GateReceiptView::requires_time_validity),
        }
    }

    fn scoped(
        status: &crate::state::WorkCheckGateReceiptStatus,
        current: &GateScopeEvaluation,
    ) -> Self {
        let evidence = &status.evidence;
        let valid_until_ms = [evidence.valid_until_ms, status.batch.valid_until_ms]
            .into_iter()
            .flatten()
            .min();
        let requires_time_validity =
            evidence.requires_time_validity || status.batch.requires_time_validity;
        let (freshness, freshness_reason) = if !crate::state::time_validity_is_current(
            valid_until_ms,
            requires_time_validity,
            crate::state::now_ms(),
        ) {
            if valid_until_ms.is_some() {
                (
                    GateFreshness::Stale,
                    "receipt time validity has expired".into(),
                )
            } else {
                (
                    GateFreshness::Unknown,
                    "receipt requires time validity but recorded no boundary".into(),
                )
            }
        } else if let Some(error) = status.batch.worktree_fingerprint_error.as_deref() {
            (
                GateFreshness::Unknown,
                format!("work-check batch could not prove a stable worktree: {error}"),
            )
        } else if let Some(error) = current.error() {
            (
                GateFreshness::Unknown,
                format!("current gate scope could not be collected: {error}"),
            )
        } else if evidence.gate_signature != current.gate_signature() {
            (
                GateFreshness::Stale,
                "gate policy or execution definition changed since the receipt".into(),
            )
        } else {
            match (
                evidence.scope_fingerprint.as_deref(),
                current.scope_fingerprint(),
            ) {
                (Some(receipt), Some(current)) if receipt == current => (
                    GateFreshness::Fresh,
                    "receipt matches the current gate-scoped fingerprint".into(),
                ),
                (Some(_), Some(_)) => (
                    GateFreshness::Stale,
                    "receipt was recorded for a different gate-scoped fingerprint".into(),
                ),
                _ => (
                    GateFreshness::Unknown,
                    "gate-scoped freshness could not be determined".into(),
                ),
            }
        };
        Self {
            receipt_id: evidence
                .tool_receipt_id
                .clone()
                .or_else(|| Some(status.batch.receipt_id.clone())),
            freshness_receipt_id: Some(status.batch.receipt_id.clone()),
            exit_status: match evidence.status.as_str() {
                "executed" | "failed" | "cancelled" => evidence.exit_status,
                "reused" | "not_applicable" => Some(0),
                "unknown" => None,
                _ => None,
            },
            ended_at_ms: Some(status.batch.ended_at_ms),
            freshness,
            freshness_reason,
            changed_paths: evidence.changed_paths.clone(),
            changed_path_count: evidence.changed_path_count,
            changed_paths_truncated: evidence.changed_paths_truncated,
            changed_paths_digest: evidence.changed_paths_digest.clone(),
            diff_summary: Some(status.batch.diff_summary.clone()),
            receipt_worktree_fingerprint_error: status
                .batch
                .worktree_fingerprint_error
                .clone()
                .or_else(|| evidence.scope_error.clone()),
            current_worktree_fingerprint_error: current.error().map(str::to_string),
            valid_until_ms,
            requires_time_validity,
        }
    }
}

fn resolve_plan_state(ctx: &RepoContext, plan_id: &str) -> Result<&'static str> {
    Ok(match plan_status(ctx, plan_id)? {
        Some(PlanStatus::Open) => "open",
        Some(PlanStatus::Closed) => "closed",
        None => bail!("Plan not found: {plan_id}"),
    })
}

fn resolve_plan_state_with_cancellation(
    ctx: &RepoContext,
    plan_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<&'static str> {
    Ok(
        match plan_status_with_cancellation(ctx, plan_id, cancelled)? {
            Some(PlanStatus::Open) => "open",
            Some(PlanStatus::Closed) => "closed",
            None => bail!("Plan not found: {plan_id}"),
        },
    )
}

fn resolve_work_plan_id(ctx: &RepoContext, requested: Option<String>) -> Result<String> {
    if let Some(plan_id) = requested {
        ensure_plan_exists(ctx, &plan_id)?;
        return Ok(plan_id);
    }

    let open_plans = open_plan_summaries(ctx)?;
    resolve_open_plan_id(&open_plans)
}

fn resolve_work_plan_id_with_cancellation(
    ctx: &RepoContext,
    requested: Option<String>,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    ensure_gate_collection_active(cancelled)?;
    if let Some(plan_id) = requested {
        ensure_plan_exists_with_cancellation(ctx, &plan_id, cancelled)?;
        ensure_gate_collection_active(cancelled)?;
        return Ok(plan_id);
    }

    let open_plans = open_plan_summaries_with_cancellation(ctx, cancelled)?;
    ensure_gate_collection_active(cancelled)?;
    resolve_open_plan_id(&open_plans)
}

fn resolve_open_plan_id(open_plans: &[Value]) -> Result<String> {
    match open_plans {
        [plan] => plan["plan_id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Open plan summary did not include a plan id")),
        [] => bail!(
            "No open work plans. Run `scripts/jig work status` to find recent plan ids, then pass --plan-id to inspect a closed or specific plan."
        ),
        _ => bail!("Multiple open work plans. Pass --plan-id to choose which plan to inspect."),
    }
}

fn ensure_gate_collection_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
}

fn latest_passing_gates(report: &GateReport) -> Vec<Value> {
    let mut latest = BTreeMap::<String, (u64, Value)>::new();
    for gate in &report.gates {
        let Some(evidence_key) = gate.evidence_key() else {
            continue;
        };
        let Some(value) = gate.to_latest_evidence() else {
            continue;
        };
        let gate_id = gate.id();
        let ended_at_ms = gate
            .receipt()
            .and_then(|receipt| receipt.ended_at_ms)
            .unwrap_or(0);
        match latest.get(&evidence_key) {
            Some((existing_ended_at_ms, _)) if *existing_ended_at_ms > ended_at_ms => {}
            Some((existing_ended_at_ms, existing))
                if *existing_ended_at_ms == ended_at_ms
                    && existing["gate_id"].as_str().unwrap_or("") >= gate_id => {}
            // Replace when this receipt is newer, or when the timestamp ties
            // and the gate id sorts after the current winner.
            _ => {
                latest.insert(evidence_key, (ended_at_ms, value));
            }
        }
    }
    latest.into_values().map(|(_, value)| value).collect()
}

fn evaluate_gate(
    ctx: &RepoContext,
    plan_scope: &PlanGateContext,
    gate: &WorkGate,
    current_fingerprint: &crate::state::CurrentWorktreeFingerprint,
    receipt_index: &WorkGateReceiptIndex,
    collection: GateCollection<'_>,
) -> Result<GateEvaluation> {
    collection.ensure_active()?;
    match gate {
        WorkGate::Check(gate) => {
            let tool_name = gate.tool.as_str();
            if let Err(error) = validate_check_tool(ctx, tool_name, "Work gate") {
                return Ok(GateEvaluation::Unsupported(UnsupportedGateEvaluation {
                    id: gate.id.clone(),
                    required: gate.required,
                    kind: "check".into(),
                    reason: Some(format!("{error:#}")),
                }));
            }
            let current_scope = match collection {
                GateCollection::Blocking => plan_scope.evaluate(ctx, gate),
                GateCollection::Cancellable(cancelled) => {
                    plan_scope.evaluate_with_cancellation(ctx, gate, cancelled)
                }
            };
            if let Some(scoped_status) = receipt_index.check_gate_receipt(&gate.id).cloned() {
                let evaluated_receipt = EvaluatedReceipt::scoped(&scoped_status, &current_scope);
                let outcome = match scoped_status.evidence.status.as_str() {
                    "executed" => GateOutcome::Passed,
                    "reused" => GateOutcome::Reused,
                    "not_applicable" => GateOutcome::NotApplicable,
                    "unknown" => GateOutcome::Unknown,
                    _ => GateOutcome::Failed,
                }
                .with_freshness(evaluated_receipt.freshness);
                return Ok(GateEvaluation::Check(Box::new(CheckGateEvaluation {
                    id: gate.id.clone(),
                    required: gate.required,
                    tool: tool_name.to_string(),
                    paths: gate.paths.clone(),
                    paths_ignore: gate.paths_ignore.clone(),
                    reuse: gate.reuse,
                    outcome,
                    receipt: evaluated_receipt,
                    evidence: Some(scoped_status.evidence),
                    current_scope,
                })));
            }
            if gate.paths.is_some() || gate.reuse {
                let mut evaluated_receipt =
                    EvaluatedReceipt::new::<ToolReceiptStatus>(None, None, current_fingerprint);
                let outcome = if let Some(error) = current_scope.error() {
                    evaluated_receipt.freshness = GateFreshness::Unknown;
                    evaluated_receipt.freshness_reason =
                        format!("gate applicability could not be determined: {error}");
                    evaluated_receipt.current_worktree_fingerprint_error = Some(error.to_string());
                    GateOutcome::Unknown
                } else {
                    GateOutcome::Missing
                };
                return Ok(GateEvaluation::Check(Box::new(CheckGateEvaluation {
                    id: gate.id.clone(),
                    required: gate.required,
                    tool: tool_name.to_string(),
                    paths: gate.paths.clone(),
                    paths_ignore: gate.paths_ignore.clone(),
                    reuse: gate.reuse,
                    outcome,
                    receipt: evaluated_receipt,
                    evidence: None,
                    current_scope,
                })));
            }
            collection.ensure_active()?;
            let receipt = receipt_index.tool_receipt(tool_name).cloned();
            collection.ensure_active()?;
            let freshness_receipt = match &receipt {
                Some(receipt) if receipt.exit_status == 0 => {
                    // Freshness is anchored to the batch work-check receipt
                    // when available, since that receipt captures the
                    // before/after worktree fingerprint for the gate run.
                    let latest = receipt_index
                        .work_check_receipt(tool_name, &receipt.receipt_id)
                        .cloned();
                    collection.ensure_active()?;
                    latest.or_else(|| Some(receipt.clone()))
                }
                _ => receipt.clone(),
            };
            let evaluated_receipt = EvaluatedReceipt::new(
                receipt.as_ref(),
                freshness_receipt.as_ref(),
                current_fingerprint,
            );
            let outcome = match &receipt {
                Some(receipt) if receipt.exit_status == 0 => GateOutcome::Passed,
                Some(_) => GateOutcome::Failed,
                None => GateOutcome::Missing,
            };

            Ok(GateEvaluation::Check(Box::new(CheckGateEvaluation {
                id: gate.id.clone(),
                required: gate.required,
                tool: tool_name.to_string(),
                paths: gate.paths.clone(),
                paths_ignore: gate.paths_ignore.clone(),
                reuse: gate.reuse,
                outcome: outcome.with_freshness(evaluated_receipt.freshness),
                receipt: evaluated_receipt,
                evidence: None,
                current_scope,
            })))
        }
        WorkGate::Evidence(gate) => match RepositoryCatalog::from_context(ctx) {
            Ok(catalog) => match EvidenceGateEvaluation::evaluate(
                gate,
                &catalog,
                current_fingerprint,
                receipt_index,
                collection,
            ) {
                Ok(evaluation) => Ok(GateEvaluation::Evidence(evaluation)),
                Err(error) => Ok(GateEvaluation::Unsupported(UnsupportedGateEvaluation {
                    id: gate.id.clone(),
                    required: gate.required,
                    kind: "evidence".into(),
                    reason: Some(format!("{error:#}")),
                })),
            },
            Err(error) => Ok(GateEvaluation::Unsupported(UnsupportedGateEvaluation {
                id: gate.id.clone(),
                required: gate.required,
                kind: "evidence".into(),
                reason: Some(format!("repository catalog is invalid: {error:#}")),
            })),
        },
        WorkGate::CodexReview(gate) => {
            let skill = gate.skill.as_str();
            collection.ensure_active()?;
            let receipt = receipt_index.review_receipt(&gate.id).cloned();
            collection.ensure_active()?;
            let evidence = receipt
                .as_ref()
                .and_then(|receipt| receipt.evidence.as_ref());
            let evidence_status = evidence.and_then(|evidence| evidence.status.as_deref());
            let outcome = match &receipt {
                Some(receipt) if receipt.exit_status == 0 => GateOutcome::Passed,
                Some(_) if evidence_status == Some("invalid_output") => GateOutcome::InvalidOutput,
                Some(_) => GateOutcome::Failed,
                None => GateOutcome::Missing,
            };
            let evaluated_receipt =
                EvaluatedReceipt::new(receipt.as_ref(), receipt.as_ref(), current_fingerprint);
            Ok(GateEvaluation::CodexReview(Box::new(
                ReviewGateEvaluation {
                    id: gate.id.clone(),
                    required: gate.required,
                    skill: skill.to_string(),
                    outcome: outcome.with_freshness(evaluated_receipt.freshness),
                    receipt: evaluated_receipt,
                    evidence: evidence.cloned(),
                },
            )))
        }
        WorkGate::Unsupported(gate) => Ok(GateEvaluation::Unsupported(UnsupportedGateEvaluation {
            id: gate.id.clone(),
            required: gate.required,
            kind: gate.kind.clone(),
            reason: None,
        })),
    }
}

trait GateReceiptView {
    fn receipt_id(&self) -> &str;
    fn exit_status(&self) -> i32;
    fn ended_at_ms(&self) -> u64;
    fn changed_paths(&self) -> &[String];
    fn changed_path_count(&self) -> usize;
    fn changed_paths_truncated(&self) -> bool;
    fn changed_paths_digest(&self) -> Option<&str>;
    fn diff_summary(&self) -> &str;
    fn worktree_fingerprint(&self) -> Option<&str>;
    fn worktree_fingerprint_error(&self) -> Option<&str>;
    fn valid_until_ms(&self) -> Option<u64>;
    fn requires_time_validity(&self) -> bool;
}

impl GateReceiptView for ToolReceiptStatus {
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

impl GateReceiptView for WorkReviewReceiptStatus {
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

fn gate_changed_paths<T: GateReceiptView>(
    receipt: Option<&T>,
) -> (Vec<String>, usize, bool, Option<String>) {
    let Some(receipt) = receipt else {
        return (Vec::new(), 0, false, None);
    };
    let total = receipt.changed_path_count();
    let paths = receipt
        .changed_paths()
        .iter()
        .take(MAX_GATE_CHANGED_PATHS)
        .cloned()
        .collect::<Vec<_>>();
    (
        paths,
        total,
        receipt.changed_paths_truncated() || total > MAX_GATE_CHANGED_PATHS,
        receipt.changed_paths_digest().map(str::to_string),
    )
}

fn gate_freshness<T: GateReceiptView>(
    receipt: Option<&T>,
    current_fingerprint: &crate::state::CurrentWorktreeFingerprint,
) -> GateFreshness {
    let Some(receipt) = receipt else {
        return GateFreshness::Missing;
    };
    if !crate::state::time_validity_is_current(
        receipt.valid_until_ms(),
        receipt.requires_time_validity(),
        crate::state::now_ms(),
    ) {
        return if receipt.valid_until_ms().is_some() {
            GateFreshness::Stale
        } else {
            GateFreshness::Unknown
        };
    }
    let Some(receipt_fingerprint) = receipt.worktree_fingerprint() else {
        return GateFreshness::Unknown;
    };
    let Some(current_fingerprint) = current_fingerprint.fingerprint.as_deref() else {
        return GateFreshness::Unknown;
    };
    if receipt_fingerprint == current_fingerprint {
        GateFreshness::Fresh
    } else {
        GateFreshness::Stale
    }
}

fn concise_error(error: &str) -> String {
    let one_line = error.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_ERROR_CHARS: usize = 240;
    if one_line.chars().count() <= MAX_ERROR_CHARS {
        return one_line;
    }

    let mut truncated = one_line
        .chars()
        .take(MAX_ERROR_CHARS.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::{
        CheckGateEvaluation, EvaluatedReceipt, GateEvaluation, GateFreshness, GateOutcome,
        GateReport, GateScopeEvaluation, RequiredGateFailures, concise_error, gate_freshness,
        latest_passing_gates,
    };

    fn passing_check(id: &str, receipt_id: &str) -> GateEvaluation {
        GateEvaluation::Check(Box::new(CheckGateEvaluation {
            id: id.to_string(),
            required: true,
            tool: "jig.test".into(),
            paths: None,
            paths_ignore: Vec::new(),
            reuse: false,
            outcome: GateOutcome::Passed,
            evidence: None,
            receipt: EvaluatedReceipt {
                receipt_id: Some(receipt_id.to_string()),
                freshness_receipt_id: Some(receipt_id.to_string()),
                exit_status: Some(0),
                ended_at_ms: Some(42),
                freshness: GateFreshness::Fresh,
                freshness_reason: "receipt matches current worktree fingerprint".into(),
                changed_paths: Vec::new(),
                changed_path_count: 0,
                changed_paths_truncated: false,
                changed_paths_digest: None,
                diff_summary: None,
                receipt_worktree_fingerprint_error: None,
                current_worktree_fingerprint_error: None,
                valid_until_ms: None,
                requires_time_validity: false,
            },
            current_scope: GateScopeEvaluation::test_known(
                "signature",
                "always applicable",
                "fingerprint",
            ),
        }))
    }

    #[test]
    fn concise_error_reserves_room_for_ellipsis() {
        let error = "x".repeat(300);
        let concise = concise_error(&error);

        assert_eq!(concise.chars().count(), 240);
        assert!(concise.ends_with("..."));
    }

    #[test]
    fn latest_passing_gates_keeps_distinct_gate_identities() {
        let report = GateReport {
            plan_id: "plan-test".into(),
            plan_state: "open",
            plan_baseline: None,
            current_worktree_fingerprint: Some("fingerprint".into()),
            current_worktree_fingerprint_error: None,
            gates: vec![
                passing_check("alpha", "receipt-alpha"),
                passing_check("zeta", "receipt-zeta"),
            ],
            required_failures: RequiredGateFailures::default(),
        };

        let latest = latest_passing_gates(&report);

        assert_eq!(latest.len(), 2);
        assert_eq!(latest[0]["gate_id"], "alpha");
        assert_eq!(latest[0]["receipt_id"], "receipt-alpha");
        assert_eq!(latest[1]["gate_id"], "zeta");
        assert_eq!(latest[1]["receipt_id"], "receipt-zeta");
    }

    #[test]
    fn gate_outcome_uses_closed_freshness_mapping() {
        assert_eq!(
            GateOutcome::Passed
                .with_freshness(GateFreshness::Fresh)
                .as_str(),
            "passed"
        );
        assert_eq!(
            GateOutcome::Passed
                .with_freshness(GateFreshness::Missing)
                .as_str(),
            "missing"
        );
        assert_eq!(
            GateOutcome::Passed
                .with_freshness(GateFreshness::Stale)
                .as_str(),
            "stale"
        );
        assert_eq!(
            GateOutcome::Passed
                .with_freshness(GateFreshness::Unknown)
                .as_str(),
            "unknown"
        );
        assert_eq!(
            GateOutcome::Failed
                .with_freshness(GateFreshness::Unknown)
                .as_str(),
            "failed"
        );
    }

    fn time_receipt(
        valid_until_ms: Option<u64>,
        requires_time_validity: bool,
    ) -> crate::state::ToolReceiptStatus {
        crate::state::ToolReceiptStatus {
            receipt_id: "receipt-time".into(),
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
    fn direct_gate_freshness_enforces_expiry_and_required_boundaries() {
        let current = crate::state::CurrentWorktreeFingerprint {
            fingerprint: Some("fingerprint".into()),
            error: None,
        };
        assert_eq!(
            gate_freshness(Some(&time_receipt(Some(0), true)), &current),
            GateFreshness::Stale
        );
        assert_eq!(
            gate_freshness(Some(&time_receipt(None, true)), &current),
            GateFreshness::Unknown
        );
        assert_eq!(
            gate_freshness(Some(&time_receipt(Some(u64::MAX), true)), &current),
            GateFreshness::Fresh
        );
        assert_eq!(
            gate_freshness(Some(&time_receipt(None, false)), &current),
            GateFreshness::Fresh
        );
    }
}
