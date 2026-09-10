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
        // Preserve pre-epoch-9 batch/gate expiry aggregation exactly.
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
