use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{
    IndexedCheckReceipts, IndexedTargetReceipts, ReceiptRecord, TargetId,
    WORK_CHECK_EVIDENCE_SCHEMA, WorkCheckBatchEvidence, WorkCheckGateReceiptStatus,
    WorkGateReceiptIndex, receipt_arg_strings, receipt_args_has_receipt_ids,
    receipt_args_include_receipt_id, target_receipt_status, tool_receipt_status,
    work_review_receipt_status,
};
use crate::tool_defs::tool;

/// Incremental form used by the dashboard epoch so receipt summaries and all
/// open-plan gate indexes are reduced during the same physical traversal.
pub(crate) struct WorkGateReceiptIndexes {
    indexes: BTreeMap<String, WorkGateReceiptIndex>,
    review_gate_ids: BTreeSet<String>,
}

impl WorkGateReceiptIndexes {
    pub(crate) fn new(
        plan_ids: &BTreeSet<String>,
        check_tools: &BTreeSet<String>,
        review_gate_ids: &BTreeSet<String>,
        evidence_targets: &BTreeMap<String, BTreeSet<TargetId>>,
    ) -> Self {
        let indexes = plan_ids
            .iter()
            .map(|plan_id| {
                (
                    plan_id.clone(),
                    WorkGateReceiptIndex {
                        checks: check_tools
                            .iter()
                            .map(|tool_name| (tool_name.clone(), IndexedCheckReceipts::default()))
                            .collect(),
                        check_gates: BTreeMap::new(),
                        reviews: BTreeMap::new(),
                        evidence: evidence_targets
                            .iter()
                            .map(|(gate_id, targets)| {
                                (gate_id.clone(), IndexedTargetReceipts::new(targets.clone()))
                            })
                            .collect(),
                    },
                )
            })
            .collect();
        Self {
            indexes,
            review_gate_ids: review_gate_ids.clone(),
        }
    }

    pub(crate) fn observe(&mut self, receipt: &ReceiptRecord) {
        let Some(plan_id) = receipt.plan_id.as_deref() else {
            return;
        };
        let Some(index) = self.indexes.get_mut(plan_id) else {
            return;
        };

        let direct_tool_name = index
            .checks
            .contains_key(&receipt.tool_name)
            .then_some(receipt.tool_name.as_str());
        if let Some(tool_name) = direct_tool_name {
            let receipts = index
                .checks
                .get_mut(tool_name)
                .expect("configured check tool should be indexed");
            receipts.direct = Some(tool_receipt_status(receipt));
            receipts.exact_work_check = None;
            receipts.legacy_work_check = None;
        }

        if receipt.tool_name == tool::WORK_CHECK {
            let batch_status = tool_receipt_status(receipt);
            for gate_id in receipt_arg_strings(receipt, "gates") {
                index.check_gates.remove(gate_id);
            }
            if let Some(evidence) = receipt
                .evidence
                .as_ref()
                .and_then(|evidence| {
                    serde_json::from_value::<WorkCheckBatchEvidence>(evidence.clone()).ok()
                })
                .filter(|evidence| evidence.schema == WORK_CHECK_EVIDENCE_SCHEMA)
            {
                for gate in evidence.into_hydrated_gates() {
                    index.check_gates.insert(
                        gate.gate_id.clone(),
                        WorkCheckGateReceiptStatus {
                            batch: batch_status.clone(),
                            evidence: gate,
                        },
                    );
                }
            }
            if receipt.exit_status == 0 {
                let has_receipt_ids = receipt_args_has_receipt_ids(receipt);
                for tool_name in receipt_arg_strings(receipt, "tools") {
                    if direct_tool_name == Some(tool_name) {
                        continue;
                    }
                    let Some(receipts) = index.checks.get_mut(tool_name) else {
                        continue;
                    };
                    let Some(direct) = receipts.direct.as_ref() else {
                        continue;
                    };
                    if receipt_args_include_receipt_id(receipt, &direct.receipt_id) {
                        receipts.exact_work_check = Some(batch_status.clone());
                    } else if !has_receipt_ids {
                        receipts.legacy_work_check = Some(batch_status.clone());
                    }
                }
            }
        }

        if receipt.tool_name == tool::WORK_REVIEW
            && let Some(gate_id) = receipt
                .args
                .get("gate_id")
                .and_then(Value::as_str)
                .filter(|gate_id| self.review_gate_ids.contains(*gate_id))
        {
            index
                .reviews
                .insert(gate_id.to_string(), work_review_receipt_status(receipt));
        }

        if let (Some(run_id), Some(target)) = (receipt.run_id.as_ref(), receipt.target.as_ref()) {
            let status = target_receipt_status(receipt, run_id, target);
            for receipts in index.evidence.values_mut() {
                receipts.observe(&status);
            }
        }
    }

    pub(crate) fn into_indexes(self) -> BTreeMap<String, WorkGateReceiptIndex> {
        self.indexes
    }
}
