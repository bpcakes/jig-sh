use crate::surface::work::{
    CheckActivity, GateSummary, MAX_ROWS, ObservationSummary, RecoveryCommand, WorkCompletion,
    bounded_reason,
};

use super::*;

pub(super) fn render(report: &GateReport, command: &str, check: Option<&Value>) -> Result<Value> {
    let finish_ready = report.plan_state == "open"
        && report.gates_ok()
        && report.current_worktree_fingerprint_error.is_none();
    let mut remaining_targets = MAX_ROWS;
    let gates = report
        .gates
        .iter()
        .take(MAX_ROWS)
        .map(|gate| {
            let mut summary = summarize_gate(gate, remaining_targets);
            summary.targets.truncate(remaining_targets);
            remaining_targets -= summary.targets.len();
            summary.targets_truncated = summary.target_count > summary.targets.len();
            summary
        })
        .collect();
    let (activity, activity_count) = check.map(check_activity).unwrap_or_default();
    let error = check
        .and_then(|value| value["error"].as_str())
        .map(|message| bounded_reason(message).0);
    let (inspection, message) = report.recovery.as_ref().map_or_else(
        || match report.current_worktree_fingerprint_error.as_deref() {
            Some(error) => ("unavailable", error),
            None => (
                "complete",
                "Current gate evaluation; detailed evidence remains available.",
            ),
        },
        |recovery| (recovery.inspection.as_str(), recovery.message.as_str()),
    );
    let (message, message_truncated) = bounded_reason(message);
    serde_json::to_value(WorkCompletion {
        ok: check.is_none_or(|value| value["ok"] == true),
        schema_version: 1,
        command: command.into(),
        plan_id: report.plan_id.clone(),
        plan_state: report.plan_state.into(),
        observed_at_ms: crate::state::now_ms(),
        finish_ready,
        readiness_basis: "Current required-gate observation only; work finish independently revalidates under its execution lease.".into(),
        observation: ObservationSummary { status: inspection.into(), message, message_truncated },
        gates,
        gate_count: report.gates.len(),
        gates_truncated: report.gates.len() > MAX_ROWS,
        activity,
        activity_count,
        activity_truncated: activity_count > MAX_ROWS,
        error,
        next_step: next_step(report, finish_ready),
        evidence: RecoveryCommand::work("evidence", &report.plan_id, true),
        receipts: RecoveryCommand::work("receipts", &report.plan_id, true),
    }).map_err(Into::into)
}

fn summarize_gate(gate: &GateEvaluation, remaining_targets: usize) -> GateSummary {
    let receipt = gate.receipt();
    let (kind, reason, targets, target_count) = match gate {
        GateEvaluation::Evidence(evidence) => {
            let targets: Vec<_> = evidence.compact_targets().take(remaining_targets).collect();
            (
                "evidence".to_owned(),
                evidence.freshness_reason(),
                targets,
                evidence.target_count(),
            )
        }
        GateEvaluation::Check(check) => (
            "check".into(),
            check
                .current_scope
                .error()
                .unwrap_or_else(|| {
                    if check.outcome == GateOutcome::NotApplicable {
                        check.current_scope.reason()
                    } else {
                        &check.receipt.freshness_reason
                    }
                })
                .to_owned(),
            Vec::new(),
            0,
        ),
        GateEvaluation::CodexReview(review) => (
            "codex_review".into(),
            review
                .evidence
                .as_ref()
                .and_then(WorkReviewReceiptEvidence::parse_error)
                .unwrap_or(&review.receipt.freshness_reason)
                .to_owned(),
            Vec::new(),
            0,
        ),
        GateEvaluation::Unsupported(unsupported) => (
            unsupported.kind.clone(),
            unsupported.reason.clone().unwrap_or_else(|| {
                "Unsupported required policy; inspect the configured gate.".into()
            }),
            Vec::new(),
            0,
        ),
    };
    let (reason, reason_truncated) = bounded_reason(&reason);
    GateSummary {
        id: gate.id().into(),
        kind,
        required: gate.required(),
        status: gate.outcome().as_str().into(),
        freshness: match gate {
            GateEvaluation::Evidence(evidence) => Some(evidence.freshness().as_str().into()),
            _ => receipt.map(|receipt| receipt.freshness.as_str().into()),
        },
        reason,
        reason_truncated,
        targets,
        target_count,
        targets_truncated: target_count > MAX_ROWS,
    }
}

fn next_step(report: &GateReport, ready: bool) -> Option<RecoveryCommand> {
    if report.plan_state != "open" {
        return None;
    }
    let inspect = || {
        let mut command = RecoveryCommand::work("gates", &report.plan_id, true);
        command.argv.extend([
            "--projection".into(),
            "agent-v1".into(),
            "--freshness-timeout-ms".into(),
            RECORDING_TIMEOUT_MS.to_string(),
        ]);
        command
    };
    let unavailable = report.current_worktree_fingerprint_error.is_some()
        || report
            .recovery
            .as_ref()
            .is_some_and(|recovery| recovery.inspection != "complete")
        || report.gates.iter().any(|gate| {
            gate.required()
                && (matches!(
                    gate.outcome(),
                    GateOutcome::Unknown | GateOutcome::Unsupported
                ) || match gate {
                    GateEvaluation::Evidence(evidence) => evidence.has_unavailable_target(),
                    _ => gate.receipt().is_some_and(|receipt| {
                        matches!(
                            receipt.freshness,
                            GateFreshness::Unknown | GateFreshness::Unsupported
                        )
                    }),
                })
        });
    if unavailable {
        return Some(inspect());
    }
    if ready {
        return Some(RecoveryCommand::work("finish", &report.plan_id, false));
    }
    if let Some(command) = report
        .recovery
        .as_ref()
        .and_then(|recovery| recovery.next_step.as_ref())
    {
        return Some(RecoveryCommand {
            argv: command.argv.clone(),
            read_only: command.read_only,
        });
    }
    for gate in report.gates.iter().filter(|gate| gate.required()) {
        if matches!(
            gate.outcome(),
            GateOutcome::Passed | GateOutcome::Reused | GateOutcome::NotApplicable
        ) {
            continue;
        }
        return Some(match gate {
            GateEvaluation::CodexReview(_) => {
                let mut command = RecoveryCommand::work("review", &report.plan_id, false);
                command.argv.extend(["--gate".into(), gate.id().into()]);
                command
            }
            GateEvaluation::Check(_) | GateEvaluation::Evidence(_) => {
                RecoveryCommand::work("check", &report.plan_id, false)
            }
            GateEvaluation::Unsupported(_) => inspect(),
        });
    }
    Some(inspect())
}

fn check_activity(check: &Value) -> (Vec<CheckActivity>, usize) {
    let mut activity = Vec::new();
    let mut count = 0;
    let mut retain = |row| {
        count += 1;
        if activity.len() < MAX_ROWS {
            activity.push(row);
        }
    };
    for target in check["target_evidence"].as_array().into_iter().flatten() {
        let subject = format!(
            "{}:{}",
            target["target"]["component"].as_str().unwrap_or("?"),
            target["target"]["action"].as_str().unwrap_or("?")
        );
        retain(CheckActivity {
            subject,
            disposition: target["disposition"].as_str().unwrap_or("unknown").into(),
            status: target["status"].as_str().unwrap_or("unknown").into(),
        });
    }
    let mut represented_gates = BTreeSet::new();
    for gate in check["gate_evidence"].as_array().into_iter().flatten() {
        if let Some(id) = gate["gate_id"].as_str() {
            represented_gates.insert(id);
        }
        retain(CheckActivity {
            subject: gate["gate_id"].as_str().unwrap_or("?").into(),
            disposition: gate["status"].as_str().unwrap_or("unknown").into(),
            status: match gate["status"].as_str() {
                Some("executed" | "reused") => "passed",
                Some(status) => status,
                None => "unknown",
            }
            .into(),
        });
    }
    // A selection can mix gate-backed and ungated tools. Suppress only the
    // exact gate result already represented above, never the entire batch.
    for tool in check["checks"].as_array().into_iter().flatten() {
        let represented = tool["gate_id"]
            .as_str()
            .is_some_and(|id| represented_gates.contains(id));
        if !represented {
            retain(CheckActivity {
                subject: tool["tool"].as_str().unwrap_or("?").into(),
                disposition: "executed".into(),
                status: if tool["result"]["exit_status"] == 0 {
                    "passed"
                } else {
                    "failed"
                }
                .into(),
            });
        }
    }
    (activity, count)
}
