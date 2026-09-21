use super::super::jsonl::scan_jsonl_file_for_locked_receipt;
use super::*;

pub(super) fn current_plan_work_check_gate_evidence_in_locked_journal(
    journal: &std::fs::File,
    path: &Path,
    plan_id: &str,
    gate_ids: &BTreeSet<String>,
    cancelled: &dyn Fn() -> bool,
) -> Result<BTreeSet<String>> {
    let mut found = BTreeSet::new();
    scan_jsonl_file_for_locked_receipt(journal, path, cancelled, &mut |record| {
        let receipt = parse_raw_receipt(record, path)?;
        if receipt.tool_name != tool::WORK_CHECK || receipt.plan_id.as_deref() != Some(plan_id) {
            return Ok(());
        }
        found.extend(
            receipt_arg_strings(&receipt, "gates")
                .filter(|gate_id| gate_ids.contains(*gate_id))
                .map(str::to_string),
        );
        if let Some(evidence) = receipt
            .evidence
            .as_ref()
            .and_then(|evidence| {
                serde_json::from_value::<WorkCheckBatchEvidence>(evidence.clone()).ok()
            })
            .filter(|evidence| evidence.schema == WORK_CHECK_EVIDENCE_SCHEMA)
        {
            found.extend(
                evidence
                    .gates
                    .into_iter()
                    .map(|gate| gate.gate_id)
                    .filter(|gate_id| gate_ids.contains(gate_id)),
            );
        }
        Ok(())
    })?;
    Ok(found)
}
