use std::collections::BTreeSet;

use anyhow::{Result, ensure};

use super::{
    MAX_ATTEMPT, MAX_CORRELATION_BYTES, MAX_DETAIL_BYTES, MAX_IDENTIFIER_BYTES,
    MAX_REFERENCES_PER_EVENT, ProjectedTrackerOperation, TRACKER_OPERATION_SCHEMA_VERSION,
    TrackerOperationCorrelation, TrackerOperationEventV1, TrackerOperationPhase,
};
use crate::state::plan_files::validate_plan_id;
use crate::state::tracker_identity::validate_portable_tracker_issue;

pub(super) fn validate_fact(fact: &TrackerOperationEventV1) -> Result<()> {
    ensure!(
        fact.schema_version == TRACKER_OPERATION_SCHEMA_VERSION,
        "unsupported schema version {}",
        fact.schema_version
    );
    validate_local_id("event_id", &fact.event_id, MAX_IDENTIFIER_BYTES)?;
    validate_local_id("operation_id", &fact.operation_id, MAX_IDENTIFIER_BYTES)?;
    validate_plan_id(&fact.plan_id)?;
    validate_portable_tracker_issue(
        &fact.issue.provider,
        &fact.issue.workspace_id,
        &fact.issue.issue_id,
        &fact.issue.tracker_root,
    )?;
    validate_references("receipt_id", &fact.receipt_ids)?;
    validate_references("run_id", &fact.run_ids)?;
    validate_references("resolved event ID", &fact.resolves_event_ids)?;
    if let Some(detail) = &fact.detail {
        validate_bounded_text("detail", detail, MAX_DETAIL_BYTES)?;
    }
    if let Some(correlation) = &fact.correlation {
        validate_correlation(correlation)?;
    }
    match fact.phase {
        TrackerOperationPhase::Acknowledgement => ensure!(
            fact.outcome.is_some(),
            "acknowledgement requires an applied or no_effect outcome"
        ),
        _ => {
            ensure!(
                fact.outcome.is_none(),
                "only an acknowledgement may carry a terminal outcome"
            );
            ensure!(
                fact.resolves_event_ids.is_empty(),
                "only an acknowledgement may resolve nonterminal event IDs"
            );
        }
    }
    Ok(())
}

/// Validate the set of facts independently of their physical JSONL order.
/// `merge=union` can interleave facts that were each valid after the same base.
pub(super) fn validate_merged_history(
    operation: &ProjectedTrackerOperation,
) -> Result<Option<super::TrackerOperationOutcome>> {
    let intent_count = operation
        .events
        .iter()
        .filter(|event| event.fact.phase == TrackerOperationPhase::Intent)
        .count();
    ensure!(
        intent_count == 1,
        "invalid tracker operation '{}' history: expected exactly one intent fact",
        operation.operation_id
    );
    let nonterminal_event_ids = operation
        .events
        .iter()
        .filter(|event| {
            matches!(
                event.fact.phase,
                TrackerOperationPhase::Attempt
                    | TrackerOperationPhase::Observation
                    | TrackerOperationPhase::Error
            )
        })
        .map(|event| event.fact.event_id.as_str())
        .collect::<BTreeSet<_>>();
    let attempt_event_ids = operation
        .events
        .iter()
        .filter(|event| event.fact.phase == TrackerOperationPhase::Attempt)
        .map(|event| event.fact.event_id.as_str())
        .collect::<BTreeSet<_>>();
    let has_observation_or_error = operation.events.iter().any(|event| {
        matches!(
            event.fact.phase,
            TrackerOperationPhase::Observation | TrackerOperationPhase::Error
        )
    });
    let mut resolved_event_ids = BTreeSet::new();
    let mut resolves_attempt = false;
    for acknowledgement in operation
        .events
        .iter()
        .filter(|event| event.fact.phase == TrackerOperationPhase::Acknowledgement)
    {
        for resolved in &acknowledgement.fact.resolves_event_ids {
            ensure!(
                nonterminal_event_ids.contains(resolved.as_str()),
                "tracker operation '{}' acknowledgement resolves unknown or terminal event '{}'",
                operation.operation_id,
                resolved
            );
            resolves_attempt |= attempt_event_ids.contains(resolved.as_str());
            resolved_event_ids.insert(resolved.as_str());
        }
    }
    ensure!(
        !resolves_attempt || has_observation_or_error,
        "invalid tracker operation '{}' history: an acknowledgement resolves an attempt without observation or error evidence",
        operation.operation_id
    );
    Ok(operation
        .terminal_outcome
        .filter(|_| nonterminal_event_ids.is_subset(&resolved_event_ids)))
}

/// Appends still obey the local lifecycle. Merge-tolerant replay must not make
/// a new sequential writer capable of extending terminal or invalid state.
pub(super) fn validate_append_transition(
    operation: Option<&ProjectedTrackerOperation>,
    next: &TrackerOperationEventV1,
) -> Result<()> {
    let Some(operation) = operation else {
        ensure!(
            next.phase == TrackerOperationPhase::Intent,
            "tracker operation '{}' must begin with intent",
            next.operation_id
        );
        return Ok(());
    };
    ensure!(
        operation.plan_id == next.plan_id
            && operation.issue == next.issue
            && operation.kind == next.kind,
        "tracker operation '{}' has conflicting immutable identity",
        next.operation_id
    );
    let terminal_outcome = validate_merged_history(operation)?;
    ensure!(
        terminal_outcome.is_none(),
        "tracker operation '{}' already has a terminal acknowledgement",
        next.operation_id
    );
    if next.phase == TrackerOperationPhase::Acknowledgement {
        if let Some(existing_outcome) = operation.events.iter().find_map(|event| event.fact.outcome)
        {
            ensure!(
                next.outcome == Some(existing_outcome),
                "tracker operation '{}' has conflicting terminal outcomes",
                next.operation_id
            );
        }
        for resolved in &next.resolves_event_ids {
            ensure!(
                operation.events.iter().any(|event| {
                    event.fact.event_id == *resolved
                        && matches!(
                            event.fact.phase,
                            TrackerOperationPhase::Attempt
                                | TrackerOperationPhase::Observation
                                | TrackerOperationPhase::Error
                        )
                }),
                "tracker operation '{}' acknowledgement resolves unknown or terminal event '{}'",
                next.operation_id,
                resolved
            );
        }
        ensure!(
            !next.resolves_event_ids.is_empty()
                || !operation.events.iter().any(|event| {
                    matches!(
                        event.fact.phase,
                        TrackerOperationPhase::Attempt
                            | TrackerOperationPhase::Observation
                            | TrackerOperationPhase::Error
                    )
                }),
            "tracker operation '{}' acknowledgement resolves no nonterminal events",
            next.operation_id
        );
    }
    let previous = operation
        .events
        .last()
        .expect("validated operation contains its intent")
        .fact
        .phase;
    let allowed = match previous {
        TrackerOperationPhase::Intent => matches!(
            next.phase,
            TrackerOperationPhase::Attempt
                | TrackerOperationPhase::Observation
                | TrackerOperationPhase::Acknowledgement
                | TrackerOperationPhase::Error
        ),
        TrackerOperationPhase::Attempt => matches!(
            next.phase,
            TrackerOperationPhase::Observation | TrackerOperationPhase::Error
        ),
        TrackerOperationPhase::Observation | TrackerOperationPhase::Error => matches!(
            next.phase,
            TrackerOperationPhase::Attempt
                | TrackerOperationPhase::Observation
                | TrackerOperationPhase::Acknowledgement
                | TrackerOperationPhase::Error
        ),
        // A union merge can place an acknowledgement at the physical tail
        // while leaving a concurrent attempt unresolved. Permit evidence that
        // reconciles that pending attempt, but never another write attempt.
        TrackerOperationPhase::Acknowledgement => matches!(
            next.phase,
            TrackerOperationPhase::Observation
                | TrackerOperationPhase::Acknowledgement
                | TrackerOperationPhase::Error
        ),
    };
    ensure!(
        allowed,
        "invalid tracker operation '{}' transition from {:?} to {:?}",
        next.operation_id,
        previous,
        next.phase
    );
    Ok(())
}

fn validate_correlation(correlation: &TrackerOperationCorrelation) -> Result<()> {
    let fields = [
        ("idempotency_key", correlation.idempotency_key.as_deref()),
        ("request_id", correlation.request_id.as_deref()),
        (
            "provider_operation_id",
            correlation.provider_operation_id.as_deref(),
        ),
        (
            "reconciliation_key",
            correlation.reconciliation_key.as_deref(),
        ),
    ];
    ensure!(
        fields.iter().any(|(_, value)| value.is_some()) || correlation.attempt.is_some(),
        "correlation must contain at least one value"
    );
    for (name, value) in fields {
        if let Some(value) = value {
            validate_bounded_text(name, value, MAX_CORRELATION_BYTES)?;
        }
    }
    if let Some(attempt) = correlation.attempt {
        ensure!(
            attempt <= MAX_ATTEMPT,
            "correlation attempt exceeds {MAX_ATTEMPT}"
        );
    }
    Ok(())
}

fn validate_references(label: &str, references: &[String]) -> Result<()> {
    ensure!(
        references.len() <= MAX_REFERENCES_PER_EVENT,
        "an event may reference at most {MAX_REFERENCES_PER_EVENT} {label} values"
    );
    let mut unique = BTreeSet::new();
    for reference in references {
        validate_local_id(label, reference, MAX_IDENTIFIER_BYTES)?;
        ensure!(unique.insert(reference), "duplicate {label} '{reference}'");
    }
    Ok(())
}

fn validate_local_id(label: &str, value: &str, max_bytes: usize) -> Result<()> {
    validate_bounded_text(label, value, max_bytes)?;
    ensure!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')),
        "{label} contains unsupported characters"
    );
    Ok(())
}

fn validate_bounded_text(label: &str, value: &str, max_bytes: usize) -> Result<()> {
    ensure!(!value.is_empty(), "{label} must not be empty");
    ensure!(
        value.len() <= max_bytes,
        "{label} exceeds {max_bytes} bytes"
    );
    ensure!(
        !value.chars().any(char::is_control),
        "{label} must not contain control characters"
    );
    Ok(())
}
