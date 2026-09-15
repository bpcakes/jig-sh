fn current_open_plan_ids(events: &[PlanEvent]) -> BTreeSet<String> {
    let mut open = BTreeMap::<String, bool>::new();
    for event in events {
        match event {
            PlanEvent::Open { plan_id, .. } => {
                open.insert(plan_id.clone(), true);
            }
            PlanEvent::Close { plan_id, .. } => {
                open.insert(plan_id.clone(), false);
            }
            PlanEvent::Append { .. } | PlanEvent::Unknown { .. } => {}
        }
    }
    open.into_iter()
        .filter_map(|(plan_id, is_open)| is_open.then_some(plan_id))
        .collect()
}

struct ConfiguredGateEvidence {
    check_tools: BTreeSet<String>,
    check_gate_ids: BTreeSet<String>,
    review_gate_ids: BTreeSet<String>,
    targets: BTreeMap<String, BTreeSet<jig_contract::TargetId>>,
}

fn configured_gate_evidence_keys(ctx: &RepoContext) -> Result<ConfiguredGateEvidence> {
    let mut configured = ConfiguredGateEvidence {
        check_tools: BTreeSet::new(),
        check_gate_ids: BTreeSet::new(),
        review_gate_ids: BTreeSet::new(),
        targets: BTreeMap::new(),
    };
    let gates = ctx.work_gates();
    let repository = gates
        .iter()
        .any(|gate| matches!(gate, WorkGate::Evidence(_)))
        .then(|| RepositoryCatalog::from_context(ctx))
        .transpose()?;
    for gate in gates {
        match gate {
            WorkGate::Check(gate) => {
                configured.check_gate_ids.insert(gate.id);
                configured.check_tools.insert(gate.tool);
            }
            WorkGate::CodexReview(gate) => {
                configured.review_gate_ids.insert(gate.id);
            }
            WorkGate::Evidence(gate) => {
                let catalog = repository
                    .as_ref()
                    .expect("evidence gates initialize the repository catalog");
                configured
                    .targets
                    .insert(gate.id, resolve_evidence_targets(catalog, &gate.selector)?);
            }
            WorkGate::Unsupported(_) => {}
        }
    }
    Ok(configured)
}

impl ReceiptProtectionIndex {
    pub(super) fn protected_receipt_ids(&self) -> Result<BTreeSet<String>> {
        let mut protected = BTreeSet::new();
        for check in self.checks.values() {
            let Some(direct_receipt) = &check.direct_receipt else {
                continue;
            };
            if !receipt_is_time_current(direct_receipt, self.now_ms) {
                continue;
            }
            protected.insert(direct_receipt.id.clone());
            if let Some(work_check) = check
                .exact_work_check
                .as_ref()
                .or(check.legacy_work_check.as_ref())
                .filter(|work_check| work_check_is_time_current(work_check, self.now_ms))
            {
                protected.insert(work_check.id.clone());
                protected.extend(work_check.receipt_ids.iter().cloned());
            }
        }
        for work_check in self.latest_check_by_plan_gate.values() {
            if !work_check_is_time_current(work_check, self.now_ms) {
                continue;
            }
            protected.insert(work_check.id.clone());
            protected.extend(work_check.receipt_ids.iter().cloned());
        }
        for receipt in self.latest_review_by_plan_gate.values() {
            if !receipt_is_time_current(receipt, self.now_ms) {
                continue;
            }
            protected.insert(receipt.id.clone());
            if let Some(worker_receipt_id) = &receipt.worker_receipt_id {
                protected.insert(worker_receipt_id.clone());
            }
        }
        // Keep each target's newest outcome, even a failure or expired proof.
        // It prevents archiving from exposing an older passing receipt again.
        for receipts in self.target_evidence.values() {
            protected.extend(
                receipts
                    .selected()
                    .values()
                    .map(|receipt| receipt.receipt_id.clone()),
            );
        }
        Ok(protected)
    }
}

fn latest_receipt(receipt: &ReceiptRecord, worker_receipt_id: Option<String>) -> LatestReceipt {
    LatestReceipt {
        id: receipt.id.clone(),
        worker_receipt_id,
        valid_until_ms: archive_time_validity(receipt).effective_valid_until_ms,
        requires_time_validity: archive_time_validity(receipt).effective_requires_time_validity,
    }
}

fn receipt_is_time_current(receipt: &LatestReceipt, now_ms: u64) -> bool {
    time_validity_is_current(
        receipt.valid_until_ms,
        receipt.requires_time_validity,
        now_ms,
    )
}

fn work_check_is_time_current(receipt: &ProtectedWorkCheck, now_ms: u64) -> bool {
    time_validity_is_current(
        receipt.valid_until_ms,
        receipt.requires_time_validity,
        now_ms,
    )
}

fn archive_time_validity(
    receipt: &ReceiptRecord,
) -> jig_contract::freshness::EffectiveTimeValidityV1 {
    super::receipt_effective_time(receipt).unwrap_or_else(|| {
        jig_contract::freshness::EffectiveTimeValidityV1::new(
            receipt.valid_until_ms,
            receipt
                .evidence
                .as_ref()
                .is_some_and(super::evidence_requires_time_validity),
        )
    })
}

fn archive_gate_time_validity(
    receipt: &ReceiptRecord,
    gate: &super::WorkCheckGateEvidence,
) -> jig_contract::freshness::EffectiveTimeValidityV1 {
    use jig_contract::freshness::EffectiveTimeValidityV1;
    let own = archive_time_validity(receipt);
    if super::receipt_effective_time(receipt).is_none() && gate.effective_time.is_none() {
        // Preserve pre-freshness-epoch batch/gate expiry aggregation exactly.
        return EffectiveTimeValidityV1::new(
            [receipt.valid_until_ms, gate.valid_until_ms]
                .into_iter()
                .flatten()
                .min(),
            own.effective_requires_time_validity || gate.requires_time_validity,
        );
    }
    own.combine(gate.effective_time.unwrap_or_else(|| {
        EffectiveTimeValidityV1::new(gate.valid_until_ms, gate.requires_time_validity)
    }))
}

fn plan_receipt_dependency_closure(
    contract_version: u32,
    guard: &JsonlWriteGuard,
    path: &Path,
    protected_plan_ids: &BTreeSet<String>,
    configured_evidence: &ConfiguredGateEvidence,
) -> Result<BTreeSet<String>> {
    let mut protection_index =
        ReceiptProtectionIndex::with_evidence(protected_plan_ids, &configured_evidence.targets);
    let protection_scan = scan_jsonl_raw_locked(guard, path, &|| false, |record| {
        let receipt = parse_raw_receipt(record, path)?;
        protection_index.observe(
            &receipt,
            protected_plan_ids,
            &configured_evidence.check_tools,
            &configured_evidence.check_gate_ids,
            &configured_evidence.review_gate_ids,
        );
        Ok(())
    })?;
    if protection_scan.unterminated_final_record {
        bail!(
            "Refusing receipt protection for {} because its final JSONL record is not newline-terminated",
            path.display()
        );
    }
    let mut protected = protection_index.protected_receipt_ids()?;
    if contract_version >= jig_contract::freshness::TARGET_FRESHNESS_CONTRACT_VERSION
        || protection_index.target_evidence.values().any(|receipts| {
            receipts
                .selected()
                .values()
                .any(|receipt| receipt.target_freshness.is_some())
        })
    {
        dependency_protection::protect_dependencies(
            guard,
            path,
            protection_index
                .target_evidence
                .values()
                .flat_map(|receipts| receipts.selected().values().cloned()),
            &mut protected,
            protection_index.now_ms,
        )?;
    }
    Ok(protected)
}
