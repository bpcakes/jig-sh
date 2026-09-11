use super::*;

pub(super) struct GateReport {
    pub(super) recovery: Option<recovery::Recovery>,
    pub(super) plan_id: String,
    pub(super) plan_state: &'static str,
    pub(super) plan_baseline: Option<PlanBaseline>,
    pub(super) current_worktree_fingerprint: Option<String>,
    pub(super) current_worktree_fingerprint_error: Option<String>,
    pub(super) gates: Vec<GateEvaluation>,
    pub(super) required_failures: RequiredGateFailures,
}

impl GateReport {
    pub(super) fn gates_ok(&self) -> bool {
        self.required_failures.is_empty()
    }

    pub(super) fn to_value(&self) -> Value {
        let gates_ok = self.gates_ok();
        json!({
            "ok": true,
            "recovery": self.recovery,
            "gates_ok": gates_ok,
            "plan_id": self.plan_id,
            "plan_state": self.plan_state,
            "plan_baseline": self.plan_baseline,
            "overall": if gates_ok { "passed" } else { "blocked" },
            "current_worktree_fingerprint": self.current_worktree_fingerprint,
            "current_worktree_fingerprint_error": self.current_worktree_fingerprint_error,
            "gates": self.gates.iter().map(GateEvaluation::to_value).collect::<Vec<_>>(),
            "missing_required": self.required_failures.missing,
            "failed_required": self.required_failures.failed,
            "stale_required": self.required_failures.stale,
            "unknown_required": self.required_failures.unknown,
            "unsupported_required": self.required_failures.unsupported,
        })
    }

    pub(super) fn fingerprint_errors(&self) -> Vec<String> {
        self.gates
            .iter()
            .filter_map(|gate| {
                let receipt = gate.receipt()?;
                match (
                    receipt.current_worktree_fingerprint_error.as_deref(),
                    receipt.receipt_worktree_fingerprint_error.as_deref(),
                ) {
                    (None, None) => None,
                    (Some(current), None) => {
                        Some(format!("{}: current={}", gate.id(), concise_error(current)))
                    }
                    (None, Some(receipt)) => {
                        Some(format!("{}: receipt={}", gate.id(), concise_error(receipt)))
                    }
                    (Some(current), Some(receipt)) => Some(format!(
                        "{}: current={}, receipt={}",
                        gate.id(),
                        concise_error(current),
                        concise_error(receipt)
                    )),
                }
            })
            .collect()
    }
}
