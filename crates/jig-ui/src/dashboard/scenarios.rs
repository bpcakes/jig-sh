//! Focused semantic fixtures shared by contract, model, and renderer tests.

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
    snapshot.failures = vec![Failure {
        id: "receipt_failed".to_string(),
        tool_name: "jig.test".to_string(),
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
pub fn status_snapshot() -> StatusSnapshot {
    let recorder = recorder_snapshot();
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
        loops: Some(status_loops()),
        errors: Vec::new(),
    }
}

#[must_use]
pub fn colliding_identities() -> (SelectableIdentity, SelectableIdentity) {
    (
        SelectableIdentity::new("raw\u{1b}[31mA", "raw�A"),
        SelectableIdentity::new("raw\u{202e}A", "raw�A"),
    )
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
                .map(|attempt| StatusLoopAttempt {
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
