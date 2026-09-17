#[derive(Debug, Default)]
pub(super) struct ReceiptProtectionIndex {
    checks: BTreeMap<(String, String), ProtectedCheckReceipts>,
    latest_check_by_plan_gate: BTreeMap<(String, String), ProtectedWorkCheck>,
    latest_review_by_plan_gate: BTreeMap<(String, String), LatestReceipt>,
    target_evidence: BTreeMap<(String, String), IndexedTargetReceipts>,
    shared_targets: Option<IndexedTargetReceipts>,
    now_ms: u64,
}

impl ReceiptProtectionIndex {
    pub(super) fn with_evidence(
        open_plan_ids: &BTreeSet<String>,
        evidence_targets: &BTreeMap<String, BTreeSet<jig_contract::TargetId>>,
        cross_plan_targets: BTreeSet<jig_contract::TargetId>,
    ) -> Self {
        let target_evidence = open_plan_ids
            .iter()
            .flat_map(|plan_id| {
                evidence_targets.iter().map(move |(gate_id, targets)| {
                    (
                        (plan_id.clone(), gate_id.clone()),
                        IndexedTargetReceipts::new(targets.clone()),
                    )
                })
            })
            .collect();
        Self {
            target_evidence,
            shared_targets: (!cross_plan_targets.is_empty()).then(|| IndexedTargetReceipts::new(cross_plan_targets)),
            now_ms: super::now_ms(),
            ..Self::default()
        }
    }

    fn target_roots(&self) -> impl Iterator<Item = &super::TargetReceiptStatus> {
        self.target_evidence.values().chain(self.shared_targets.iter())
            .flat_map(|receipts| receipts.selected().values())
    }

    pub(super) fn observe(
        &mut self,
        receipt: &ReceiptRecord,
        open_plan_ids: &BTreeSet<String>,
        check_gate_tools: &BTreeSet<String>,
        check_gate_ids: &BTreeSet<String>,
        review_gate_ids: &BTreeSet<String>,
    ) {
        if let (Some(shared), Some(target)) = (&mut self.shared_targets, &receipt.target) {
            shared.observe(&target_receipt_status(receipt, target));
        }
        let Some(plan_id) = receipt
            .plan_id
            .as_ref()
            .filter(|plan_id| open_plan_ids.contains(*plan_id))
        else {
            return;
        };
        if let Some(target) = receipt.target.as_ref() {
            let status = target_receipt_status(receipt, target);
            for ((evidence_plan_id, _), receipts) in &mut self.target_evidence {
                if evidence_plan_id == plan_id {
                    receipts.observe(&status);
                }
            }
        }
        if check_gate_tools.contains(&receipt.tool_name) {
            self.checks.insert(
                (plan_id.clone(), receipt.tool_name.clone()),
                ProtectedCheckReceipts {
                    direct_receipt: Some(latest_receipt(receipt, None)),
                    ..ProtectedCheckReceipts::default()
                },
            );
        }
        if receipt.tool_name == tool::WORK_REVIEW
            && let Some(gate_id) = receipt
                .args
                .get("gate_id")
                .and_then(Value::as_str)
                .filter(|gate_id| review_gate_ids.contains(*gate_id))
        {
            self.latest_review_by_plan_gate.insert(
                (plan_id.clone(), gate_id.to_string()),
                LatestReceipt {
                    id: receipt.id.clone(),
                    worker_receipt_id: receipt
                        .evidence
                        .as_ref()
                        .and_then(|evidence| evidence.get("worker_receipt_id"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    valid_until_ms: archive_time_validity(receipt).effective_valid_until_ms,
                    requires_time_validity: archive_time_validity(receipt)
                        .effective_requires_time_validity,
                },
            );
        }
        if receipt.tool_name == tool::WORK_CHECK {
            for gate_id in receipt_arg_strings(receipt, "gates") {
                if !check_gate_ids.contains(gate_id) {
                    continue;
                }
                let key = (plan_id.clone(), gate_id.to_string());
                self.latest_check_by_plan_gate.remove(&key);
                // The batch itself is the durable supersession tombstone even
                // when its structured evidence is malformed or omits this
                // selected gate. Retaining it keeps an archived rewrite from
                // revealing an older pass when the stream is read again.
                self.latest_check_by_plan_gate.insert(
                    key,
                    ProtectedWorkCheck {
                        id: receipt.id.clone(),
                        receipt_ids: Vec::new(),
                        valid_until_ms: archive_time_validity(receipt).effective_valid_until_ms,
                        requires_time_validity: archive_time_validity(receipt)
                            .effective_requires_time_validity,
                    },
                );
            }
            if let Some(evidence) = receipt
                .evidence
                .as_ref()
                .and_then(|evidence| {
                    serde_json::from_value::<WorkCheckBatchEvidence>(evidence.clone()).ok()
                })
                .filter(|evidence| evidence.schema == WORK_CHECK_EVIDENCE_SCHEMA)
            {
                for gate in evidence.gates {
                    if !check_gate_ids.contains(&gate.gate_id) {
                        continue;
                    }
                    let gate_time = archive_gate_time_validity(receipt, &gate);
                    let receipt_ids = [
                        gate.tool_receipt_id,
                        gate.source_batch_receipt_id,
                        gate.source_tool_receipt_id,
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                    self.latest_check_by_plan_gate.insert(
                        (plan_id.clone(), gate.gate_id),
                        ProtectedWorkCheck {
                            id: receipt.id.clone(),
                            receipt_ids,
                            valid_until_ms: gate_time.effective_valid_until_ms,
                            requires_time_validity: gate_time.effective_requires_time_validity,
                        },
                    );
                }
            }
        }
        if receipt.tool_name == tool::WORK_CHECK && receipt.exit_status == 0 {
            let receipt_ids = receipt_arg_strings(receipt, "receipt_ids")
                .map(str::to_string)
                .collect::<Vec<_>>();
            let has_receipt_ids = receipt_args_has_receipt_ids(receipt);
            for tool_name in receipt_arg_strings(receipt, "tools") {
                if !check_gate_tools.contains(tool_name) {
                    continue;
                }
                let Some(check) = self
                    .checks
                    .get_mut(&(plan_id.clone(), tool_name.to_string()))
                else {
                    continue;
                };
                let Some(direct_receipt) = check.direct_receipt.as_ref() else {
                    continue;
                };
                // A work-check receipt configured as its own direct gate
                // cannot also be the later batch proving itself.
                if direct_receipt.id == receipt.id {
                    continue;
                }
                let batch = ProtectedWorkCheck {
                    id: receipt.id.clone(),
                    receipt_ids: receipt_ids.clone(),
                    valid_until_ms: archive_time_validity(receipt).effective_valid_until_ms,
                    requires_time_validity: archive_time_validity(receipt)
                        .effective_requires_time_validity,
                };
                if receipt_ids
                    .iter()
                    .any(|receipt_id| receipt_id == &direct_receipt.id)
                {
                    check.exact_work_check = Some(batch);
                } else if !has_receipt_ids {
                    check.legacy_work_check = Some(batch);
                }
            }
        }
    }
}
