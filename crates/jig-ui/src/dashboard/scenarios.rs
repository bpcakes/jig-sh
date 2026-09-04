//! Focused semantic fixtures shared by contract, model, and renderer tests.

use serde_json::{Value, json};

use super::*;

pub const OBSERVED_AT_MS: u64 = 1_700_000_000_000;

#[must_use]
pub fn recorder_snapshot() -> RecorderSnapshot {
    let mut snapshot = RecorderSnapshot::new(
        RecorderEpochId::FIRST,
        OBSERVED_AT_MS,
        TimelineLimit::DEFAULT,
    );
    snapshot.repo = RepositoryObservation {
        name: "ExampleProject".to_string(),
        default_branch: "main".to_string(),
        source_commit: Some("0123456789abcdef".to_string()),
        source_path: Some("/example/source".to_string()),
        branch: Some("feature/example".to_string()),
        detached: false,
    };
    snapshot.harness = HarnessObservation {
        jig_version: None,
        runtime_version: "0.3.0".to_string(),
        contract_version: 8,
    };
    snapshot.current_session_id = Some("session_example".to_string());
    snapshot.counts = RecorderCounts {
        sessions: 2,
        session_events: 3,
        plans: 2,
        plan_events: 4,
        open_plans: 1,
        decisions: 1,
    };
    let gates = gates();
    snapshot.open_plans = vec![OpenPlan {
        plan_id: "plan_example".to_string(),
        title: "Example plan".to_string(),
        body_path: Some(".agent/plans/plan_example.md".to_string()),
        opened_at_ms: Some(OBSERVED_AT_MS - 5_000),
        baseline_ref: Some("HEAD".to_string()),
        baseline_oid: Some("0123456789abcdef".to_string()),
        baseline_error: None,
        gates: Some(gates),
        gates_error: None,
    }];
    snapshot.history = vec![PlanSummary {
        plan_id: "plan_closed".to_string(),
        title: "Closed example".to_string(),
        state: "closed".to_string(),
        opened_at_ms: Some(OBSERVED_AT_MS - 10_000),
        closed_at_ms: Some(OBSERVED_AT_MS - 2_000),
        resolution: Some("completed".to_string()),
        duration_ms: Some(8_000),
        baseline_ref: None,
        baseline_oid: None,
        baseline_error: Some("baseline unavailable".to_string()),
    }];
    snapshot.failures = vec![Failure {
        id: "receipt_failed".to_string(),
        tool_name: "jig.test".to_string(),
        plan_id: Some("plan_example".to_string()),
        ended_at_ms: Some(OBSERVED_AT_MS - 1_000),
        exit_status: 1,
        stderr_preview: BoundedText::for_limit(
            "example failure",
            Some(15),
            LimitId::FailureStderrChars,
        )
        .unwrap(),
    }];
    snapshot.tool_stats = vec![ToolStat {
        tool: "jig.test".to_string(),
        runs: 3,
        failures: 1,
        last_exit_status: 1,
        last_ended_at_ms: OBSERVED_AT_MS - 1_000,
        avg_duration_ms: 250,
    }];
    snapshot.loops = Some(loops());
    snapshot.timeline = vec![TimelineRow::Receipt(ReceiptTimelineRow {
        stable_identity: "receipt:receipt_failed".to_string(),
        timestamp_ms: Some(OBSERVED_AT_MS - 1_000),
        id: "receipt_failed".to_string(),
        tool_name: "jig.test".to_string(),
        invoked_command_key: Some("test".to_string()),
        plan_id: Some("plan_example".to_string()),
        session_id: Some("session_example".to_string()),
        exit_status: 1,
        started_at_ms: Some(OBSERVED_AT_MS - 1_250),
        ended_at_ms: Some(OBSERVED_AT_MS - 1_000),
        duration_ms: Some(250),
        diff_summary: Some("1 file changed".to_string()),
        changed_path_count: Some(1),
        stderr_preview: Some(
            BoundedText::for_limit("example failure", Some(15), LimitId::FailureStderrChars)
                .unwrap(),
        ),
    })];
    snapshot
}

#[must_use]
pub fn partial_recorder_snapshot() -> RecorderSnapshot {
    let mut snapshot = recorder_snapshot();
    snapshot.loops = None;
    snapshot.errors.push(SnapshotError::new(
        CollectionDomain::Loops,
        SnapshotErrorCode::LoopObservationFailed,
        None,
        "example loop data is unavailable",
    ));
    snapshot
}

#[must_use]
pub fn plan_snapshot() -> PlanSnapshot {
    let plan = recorder_snapshot().open_plans.remove(0);
    let limits = PlanLimits {
        plan_decisions: root_limit(LimitId::PlanDecisions, Some(0)).unwrap(),
        plan_receipts: root_limit(LimitId::PlanReceipts, Some(0)).unwrap(),
    };
    PlanSnapshot {
        ok: true,
        command: UI_COMMAND.to_string(),
        schema_version: RECORDER_SCHEMA_VERSION,
        snapshot_kind: SnapshotKind::Plan,
        generated_at_ms: OBSERVED_AT_MS,
        basis_epoch: RecorderEpochId::FIRST,
        detail_observed_at_ms: OBSERVED_AT_MS + 10,
        gates_observed_at_ms: OBSERVED_AT_MS,
        decisions_observed_at_ms: OBSERVED_AT_MS + 5,
        plan: PlanSummary {
            plan_id: plan.plan_id.clone(),
            title: plan.title,
            state: "open".to_string(),
            opened_at_ms: plan.opened_at_ms,
            closed_at_ms: None,
            resolution: None,
            duration_ms: None,
            baseline_ref: plan.baseline_ref,
            baseline_oid: plan.baseline_oid,
            baseline_error: plan.baseline_error,
        },
        body: Some(
            BoundedText::for_limit("# Example plan\n", Some(15), LimitId::PlanBodyChars).unwrap(),
        ),
        gates: plan.gates,
        decisions: vec![Decision {
            id: "decision_example".to_string(),
            session_id: Some("session_example".to_string()),
            plan_id: Some(plan.plan_id.clone()),
            timestamp_ms: OBSERVED_AT_MS - 500,
            title: "Choose an example".to_string(),
            selected_option: "A".to_string(),
            alternatives: vec!["A".to_string(), "B".to_string()],
            rationale: BoundedText::for_limit(
                "A is deterministic.",
                Some(19),
                LimitId::TimelineDecisionRationaleChars,
            )
            .unwrap(),
        }],
        receipts: vec![Receipt {
            timestamp_ms: Some(OBSERVED_AT_MS - 100),
            id: "receipt_example".to_string(),
            tool_name: "jig.test".to_string(),
            invoked_command_key: Some("test".to_string()),
            plan_id: Some(plan.plan_id),
            session_id: Some("session_example".to_string()),
            exit_status: 0,
            started_at_ms: Some(OBSERVED_AT_MS - 300),
            ended_at_ms: Some(OBSERVED_AT_MS - 100),
            duration_ms: Some(200),
            diff_summary: Some("1 file changed".to_string()),
            changed_paths: BoundedRows::for_limit(
                vec!["src/example.rs".to_string()],
                Some(1),
                LimitId::ReceiptChangedPaths,
            )
            .unwrap(),
            stdout_preview: BoundedText::for_limit(
                "example output",
                Some(14),
                LimitId::ReceiptStdoutChars,
            )
            .unwrap(),
            stderr_preview: BoundedText::for_limit("", Some(0), LimitId::ReceiptStderrChars)
                .unwrap(),
        }],
        limits,
        errors: Vec::new(),
    }
}

#[must_use]
pub fn status_snapshot() -> StatusSnapshot {
    let recorder = recorder_snapshot();
    let accepted_provider = AcceptedProviderReport::from_raw(provider_raw_report())
        .expect("the shared provider scenario is valid");
    let provider_summary = ProviderSummary::from_report(accepted_provider.decoded());
    StatusSnapshot {
        ok: true,
        command: STATUS_COMMAND.to_string(),
        schema_version: STATUS_SCHEMA_VERSION,
        observed_at_ms: OBSERVED_AT_MS,
        outcome: StatusOutcome::Complete,
        repository: StatusRepositoryObservation {
            name: recorder.repo.name.clone(),
            default_branch: recorder.repo.default_branch.clone(),
            head_revision: recorder.repo.source_commit.clone(),
            branch: recorder.repo.branch.clone(),
            detached: recorder.repo.detached,
            dirty: Some(false),
            upstream: Some(UpstreamObservation {
                reference: "origin/main".to_string(),
                ahead: 0,
                behind: 0,
                state: "in_sync".to_string(),
                basis: "local_tracking_ref".to_string(),
            }),
        },
        work: StatusWorkSnapshot {
            state: Some(StatusStateSnapshot {
                ok: true,
                repo: StatusStateRepository {
                    name: recorder.repo.name,
                    default_branch: recorder.repo.default_branch,
                    source_commit: recorder.repo.source_commit,
                    source_path: recorder.repo.source_path,
                },
                current_session_id: recorder.current_session_id,
                counts: StatusStateCounts {
                    sessions: recorder.counts.sessions,
                    session_events: recorder.counts.session_events,
                    plans: recorder.counts.plans,
                    plan_events: recorder.counts.plan_events,
                    open_plans: recorder.counts.open_plans,
                    receipts: 3,
                    failed_receipts: 1,
                    decisions: recorder.counts.decisions,
                },
                open_plans: vec![status_open_plan()],
                recent_receipts: vec![StatusReceiptSummary {
                    id: "receipt_failed".to_string(),
                    session_id: Some("session_example".to_string()),
                    tool_name: "jig.test".to_string(),
                    invoked_command_key: Some("test".to_string()),
                    plan_id: Some("plan_example".to_string()),
                    exit_status: 1,
                    started_at_ms: Some(OBSERVED_AT_MS - 1_250),
                    ended_at_ms: Some(OBSERVED_AT_MS - 1_000),
                    diff_summary: Some("1 file changed".to_string()),
                }],
                recent_decisions: vec![StatusDecisionSummary {
                    id: "decision_example".to_string(),
                    session_id: Some("session_example".to_string()),
                    plan_id: Some("plan_example".to_string()),
                    title: "Choose an example".to_string(),
                    selected_option: "A".to_string(),
                    timestamp_ms: OBSERVED_AT_MS - 500,
                }],
            }),
            gates: vec![StatusPlanGates {
                plan_id: "plan_example".to_string(),
                snapshot: Some(status_gate_report()),
                error: None,
            }],
        },
        loops: Some(status_loops()),
        providers: vec![StatusProvider {
            id: "example-provider".to_string(),
            status: "complete".to_string(),
            duration_ms: 25,
            report: Some(accepted_provider),
            summary: Some(provider_summary),
            input_freshness: vec![InputFreshness {
                name: "target".to_string(),
                kind: "git".to_string(),
                path: Some(".".to_string()),
                expected_revision: Some("0123456789abcdef".to_string()),
                observed_revision: Some("0123456789abcdef".to_string()),
                dirty: Some(false),
                status: "current".to_string(),
                reason: None,
            }],
            error: None,
        }],
        errors: Vec::new(),
    }
}

#[must_use]
pub fn provider_raw_report() -> Value {
    json!({
        "protocol": "jig.status-provider/v1",
        "provider": {
            "id": "example-provider",
            "adapter_version": "1.0.0",
            "display_name": "Example Provider",
            "extensions": { "example.identity": { "preserved": true } },
            "future_identity_field": { "preserved": true }
        },
        "observed_at_ms": OBSERVED_AT_MS,
        "outcome": "complete",
        "work_packages": [{
            "id": "package-example",
            "title": "Example package",
            "specification": { "state": "ready", "category": "ready" },
            "implementation": { "state": "active", "category": "active" },
            "verification": { "state": "pending", "category": "pending" },
            "extensions": { "example.package": { "preserved": true } },
            "future_package_field": { "preserved": true }
        }],
        "extensions": { "example.root": { "preserved": true } },
        "future_root_field": { "preserved": true }
    })
}

#[must_use]
pub fn colliding_identities() -> (SelectableIdentity, SelectableIdentity) {
    (
        SelectableIdentity::new("provider\u{1b}[31mA", "provider�A"),
        SelectableIdentity::new("provider\u{202e}A", "provider�A"),
    )
}

fn gates() -> GatesObservation {
    GatesObservation {
        overall: "pass".to_string(),
        gates: BoundedRows::for_limit(
            vec![GateObservation {
                id: "test".to_string(),
                tool: Some("jig.test".to_string()),
                skill: None,
                required: true,
                status: "pass".to_string(),
                freshness: Some("fresh".to_string()),
                ended_at_ms: Some(OBSERVED_AT_MS - 100),
                diff_summary: Some("1 file changed".to_string()),
                changed_paths: BoundedRows::for_limit(
                    vec!["src/example.rs".to_string()],
                    Some(1),
                    LimitId::GateChangedPaths,
                )
                .unwrap(),
                matching_paths: BoundedRows::for_limit(
                    vec!["src/example.rs".to_string()],
                    Some(1),
                    LimitId::GateMatchingPaths,
                )
                .unwrap(),
                findings: BoundedRows::for_limit(Vec::new(), Some(0), LimitId::GateFindings)
                    .unwrap(),
                remediation: Some(Remediation {
                    argv: vec![
                        "scripts/jig".to_string(),
                        "check".to_string(),
                        "test".to_string(),
                    ],
                    display: "scripts/jig check test".to_string(),
                }),
            }],
            Some(1),
            LimitId::GateRows,
        )
        .unwrap(),
    }
}

fn status_open_plan() -> StatusOpenPlan {
    StatusOpenPlan {
        plan_id: "plan_example".to_string(),
        title: "Example plan".to_string(),
        body_path: Some(".agent/plans/plan_example.md".to_string()),
        baseline: Some(StatusPlanBaseline {
            requested_ref: "HEAD".to_string(),
            commit_oid: Some("0123456789abcdef".to_string()),
            empty_tree_oid: None,
            error: None,
        }),
    }
}

fn status_gate_report() -> StatusGateReport {
    StatusGateReport {
        ok: true,
        gates_ok: true,
        plan_id: "plan_example".to_string(),
        plan_state: "open".to_string(),
        plan_baseline: status_open_plan().baseline,
        current_worktree_fingerprint: Some("sha256:example".to_string()),
        current_worktree_fingerprint_error: None,
        gates: vec![StatusGate::Check(Box::new(StatusCheckGate {
            id: "test".to_string(),
            required: true,
            tool: "jig.test".to_string(),
            status: "passed".to_string(),
            receipt_id: Some("receipt_example".to_string()),
            freshness_receipt_id: Some("receipt_example".to_string()),
            exit_status: Some(0),
            ended_at_ms: Some(OBSERVED_AT_MS - 100),
            freshness: "fresh".to_string(),
            freshness_reason: "receipt matches current worktree fingerprint".to_string(),
            changed_paths: vec!["src/example.rs".to_string()],
            changed_path_count: 1,
            changed_paths_truncated: false,
            changed_paths_digest: Some("sha256:paths".to_string()),
            diff_summary: Some("1 file changed".to_string()),
            receipt_worktree_fingerprint_error: None,
            current_worktree_fingerprint_error: None,
            evidence_status: Some("completed".to_string()),
            receipt_applicability: Some("applicable".to_string()),
            applicability: Some("applicable".to_string()),
            applicability_reason: Some("changed path matched".to_string()),
            applicability_error: None,
            paths: Some(vec!["src/**".to_string()]),
            paths_ignore: Vec::new(),
            reuse: true,
            forced: Some(false),
            baseline_oid: Some("0123456789abcdef".to_string()),
            receipt_baseline_oid: Some("0123456789abcdef".to_string()),
            gate_signature: Some("sha256:gate".to_string()),
            receipt_gate_signature: Some("sha256:gate".to_string()),
            scope_fingerprint: Some("sha256:scope".to_string()),
            receipt_scope_fingerprint: Some("sha256:scope".to_string()),
            matching_paths: vec!["src/example.rs".to_string()],
            matching_path_count: 1,
            matching_paths_truncated: false,
            matching_paths_digest: Some("sha256:matching".to_string()),
            source_plan_id: None,
            source_batch_receipt_id: None,
            source_tool_receipt_id: None,
            valid_until_ms: None,
            requires_time_validity: false,
        }))],
        missing_required: Vec::new(),
        failed_required: Vec::new(),
        stale_required: Vec::new(),
        unknown_required: Vec::new(),
        unsupported_required: Vec::new(),
        overall: "passed".to_string(),
    }
}

fn status_loops() -> StatusLoopObservation {
    let recorder = loops();
    StatusLoopObservation {
        ok: recorder.ok,
        command: recorder.command,
        workflows: recorder
            .workflows
            .items()
            .iter()
            .map(|workflow| StatusLoopWorkflow {
                id: workflow.id.clone(),
                kind: workflow.kind.clone(),
                enabled: workflow.enabled,
                configured: workflow.configured,
                lease_ttl_seconds: workflow.lease_ttl_seconds,
                max_attempts: workflow.max_attempts,
                backoff_seconds: workflow.backoff_seconds,
                codex_home_configured: workflow.codex_home_configured.clone(),
                schedule: workflow.schedule.clone(),
                schedule_state: workflow.schedule_state.clone(),
                schedule_state_error: workflow.schedule_state_error.clone(),
                codex_task: workflow.codex_task.clone(),
            })
            .collect(),
        leases: recorder.leases.items().to_vec(),
        attempts: recorder
            .attempts
            .items()
            .iter()
            .map(status_loop_attempt)
            .collect(),
        scheduled_occurrences: recorder
            .scheduled_occurrences
            .items()
            .iter()
            .map(status_scheduled_occurrence)
            .collect(),
        waiting_attempts: recorder
            .waiting_attempts
            .items()
            .iter()
            .map(status_loop_attempt)
            .collect(),
        state_error_count: recorder.state_error_count,
        state_errors: recorder.state_errors,
        needs_attention: StatusLoopAttention {
            exhausted_attempts: recorder
                .needs_attention
                .exhausted_attempts
                .items()
                .iter()
                .map(|attempt| StatusExhaustedAttempt {
                    key: attempt.key.clone(),
                    workflow_id: attempt.workflow_id.clone(),
                    item_key: attempt.item_key.clone(),
                    item_version: attempt.item_version.clone(),
                    observed_item_version: attempt.observed_item_version.clone(),
                    attempts: attempt.attempts,
                    max_attempts: attempt.max_attempts,
                    last_attempt_ms: attempt.last_attempt_ms,
                    next_eligible_ms: attempt.next_eligible_ms,
                    exhausted: attempt.exhausted,
                    last_status: attempt.last_status.clone(),
                })
                .collect(),
            scheduled_occurrences: recorder
                .needs_attention
                .scheduled_occurrences
                .items()
                .iter()
                .map(status_scheduled_occurrence)
                .collect(),
        },
    }
}

fn status_loop_attempt(attempt: &LoopAttempt) -> StatusLoopAttempt {
    StatusLoopAttempt {
        key: attempt.key.clone(),
        workflow_id: attempt.workflow_id.clone(),
        item_key: attempt.item_key.clone(),
        item_version: attempt.item_version.clone(),
        observed_item_version: attempt.observed_item_version.clone(),
        attempts: attempt.attempts,
        max_attempts: attempt.max_attempts,
        last_attempt_ms: attempt.last_attempt_ms,
        next_eligible_ms: attempt.next_eligible_ms,
        exhausted: attempt.exhausted,
        last_status: attempt.last_status.clone(),
    }
}

fn status_scheduled_occurrence(occurrence: &ScheduledOccurrence) -> StatusScheduledOccurrence {
    StatusScheduledOccurrence {
        occurrence_id: occurrence.occurrence_id.clone(),
        workflow_id: occurrence.workflow_id.clone(),
        scheduled_at_ms: occurrence.scheduled_at_ms,
        owner: occurrence.owner.clone(),
        claim_expires_at_ms: occurrence.claim_expires_at_ms,
        started_at_ms: occurrence.started_at_ms,
        uses_shared_checkout: occurrence.uses_shared_checkout,
        finished_at_ms: occurrence.finished_at_ms,
        acknowledged_at_ms: occurrence.acknowledged_at_ms,
        status: occurrence.status.clone(),
        worker_receipt_id: occurrence.worker_receipt_id.clone(),
        worktree: occurrence.worktree.clone(),
        error: occurrence.error.clone(),
    }
}

fn loops() -> LoopObservation {
    LoopObservation {
        ok: true,
        command: "loop status".to_string(),
        workflows: BoundedRows::for_limit(
            vec![LoopWorkflow {
                id: "workflow-example".to_string(),
                kind: "queue".to_string(),
                enabled: true,
                configured: true,
                lease_ttl_seconds: 120,
                max_attempts: 3,
                backoff_seconds: 30,
                codex_home_configured: None,
                schedule: None,
                schedule_state: None,
                schedule_state_error: None,
                codex_task: None,
            }],
            Some(1),
            LimitId::LoopWorkflows,
        )
        .unwrap(),
        leases: BoundedRows::for_limit(
            vec![LoopLease {
                key: "item-example".to_string(),
                owner: "worker-example".to_string(),
                acquired_at_ms: OBSERVED_AT_MS - 30_000,
                expires_at_ms: OBSERVED_AT_MS + 30_000,
            }],
            Some(1),
            LimitId::LoopLeases,
        )
        .unwrap(),
        attempts: BoundedRows::for_limit(
            vec![LoopAttempt {
                key: "workflow-example:item-example".to_string(),
                workflow_id: "workflow-example".to_string(),
                item_key: "item-example".to_string(),
                item_version: Some("v1".to_string()),
                observed_item_version: Some("v1".to_string()),
                attempts: 2,
                max_attempts: 3,
                last_attempt_ms: OBSERVED_AT_MS - 60_000,
                next_eligible_ms: OBSERVED_AT_MS + 30_000,
                exhausted: false,
                last_status: "attempted".to_string(),
            }],
            Some(1),
            LimitId::LoopAttempts,
        )
        .unwrap(),
        scheduled_occurrences: BoundedRows::for_limit(
            Vec::new(),
            Some(0),
            LimitId::LoopScheduledOccurrences,
        )
        .unwrap(),
        waiting_attempts: BoundedRows::for_limit(
            Vec::new(),
            Some(0),
            LimitId::LoopWaitingAttempts,
        )
        .unwrap(),
        state_error_count: 0,
        state_errors: Vec::new(),
        needs_attention: LoopAttention {
            exhausted_attempts: BoundedRows::for_limit(
                vec![ExhaustedAttempt {
                    key: "workflow-example:item-example".to_string(),
                    workflow_id: "workflow-example".to_string(),
                    item_key: "item-example".to_string(),
                    item_version: Some("v1".to_string()),
                    observed_item_version: Some("v1".to_string()),
                    attempts: 3,
                    max_attempts: 3,
                    last_attempt_ms: OBSERVED_AT_MS - 60_000,
                    next_eligible_ms: OBSERVED_AT_MS,
                    exhausted: true,
                    last_status: "failed".to_string(),
                    remediation: Some(Remediation {
                        argv: vec![
                            "scripts/jig".to_string(),
                            "loop".to_string(),
                            "clear-attempt".to_string(),
                            "--workflow".to_string(),
                            "workflow-example".to_string(),
                            "--item".to_string(),
                            "item-example".to_string(),
                        ],
                        display: "scripts/jig loop clear-attempt --workflow workflow-example --item item-example".to_string(),
                    }),
                }],
                Some(1),
                LimitId::LoopExhaustedAttempts,
            )
            .unwrap(),
            scheduled_occurrences: BoundedRows::for_limit(
                Vec::new(),
                Some(0),
                LimitId::LoopScheduledOccurrences,
            )
            .unwrap(),
        },
    }
}
