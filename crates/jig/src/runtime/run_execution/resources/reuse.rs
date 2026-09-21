use super::*;

/// Admission performs work after the original proof was validated. Its time
/// boundary must still hold at the eventual publication decision.
pub(in crate::runtime::run_execution) fn finalize_reuse(
    mut result: TargetRunResult,
    partial_reason: Option<&str>,
    observed_at_ms: u64,
) -> Option<TargetRunResult> {
    if result
        .valid_until_ms
        .is_some_and(|boundary| observed_at_ms >= boundary)
    {
        return None;
    }
    result.ended_at_ms = Some(observed_at_ms);
    if let Some(reason) = partial_reason {
        let mut warning = finding(
            format!("Cargo resource coordination is partial: {reason}"),
            "cargo_resource",
        );
        warning.severity = FindingSeverity::Warning;
        result.findings.push(warning);
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reused(valid_until_ms: Option<u64>) -> TargetRunResult {
        let mut result = TargetRunResult::queued("api:test".parse().unwrap(), "config", "input");
        result.status = RunStatus::Completed;
        result.conclusion = Some(RunConclusion::Success);
        result.receipt_id = Some("receipt-original".into());
        result.reused_from = Some(jig_contract::ReusedTargetEvidenceV1 {
            receipt_id: "receipt-original".into(),
            run_id: "run-original".into(),
            plan_id: "plan-original".into(),
        });
        result.valid_until_ms = valid_until_ms;
        result
    }

    #[test]
    fn proof_expiring_during_admission_cannot_be_published_as_reused() {
        let proof = reused(Some(200));
        assert!(finalize_reuse(proof.clone(), None, 199).is_some());
        assert!(finalize_reuse(proof.clone(), None, 200).is_none());
        assert!(finalize_reuse(proof, None, 201).is_none());
    }

    #[test]
    fn reuse_preserves_original_provenance_without_fabricating_execution() {
        let proof = reused(None);
        let completed = finalize_reuse(proof.clone(), Some("cargo_metadata_failed"), 300).unwrap();
        assert_eq!(completed.reused_from, proof.reused_from);
        assert_eq!(completed.receipt_id, proof.receipt_id);
        assert_eq!(completed.ended_at_ms, Some(300));
        assert_eq!(completed.started_at_ms, None);
        assert_eq!(completed.exit_code, None);
        assert_eq!(completed.target_freshness, None);
        assert_eq!(completed.findings.len(), 1);
        assert_eq!(completed.findings[0].severity, FindingSeverity::Warning);
    }
}
