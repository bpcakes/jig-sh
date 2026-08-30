use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::state::now_ms;

use super::super::occurrence::{OccurrenceStore, ScheduleOccurrence};
use super::super::state::{AttemptRecord, AttemptStore, LeaseRecord, LeaseStore};

pub(super) fn append_tick_error(tick_error: &mut Option<String>, error: String) {
    match tick_error {
        Some(existing) => existing.push_str(&format!("; {error}")),
        None => *tick_error = Some(error),
    }
}

pub(super) struct TickRuntimeState {
    pub(super) checked_at_ms: u64,
    pub(super) live_leases: Vec<LeaseRecord>,
    pub(super) attempts: Vec<AttemptRecord>,
    pub(super) scheduled_needs_attention: Vec<ScheduleOccurrence>,
    pub(super) state_errors: Vec<Value>,
}

impl TickRuntimeState {
    pub(super) fn collect(
        ctx: &RepoContext,
        lease_store: &mut LeaseStore,
        attempt_store: &mut AttemptStore,
        active_scheduled_occurrence: Option<&str>,
    ) -> Self {
        let checked_at_ms = now_ms();
        let mut state_errors = Vec::new();
        let live_leases = match lease_store.active_leases() {
            Ok(leases) => leases,
            Err(error) => {
                state_errors.push(runtime_state_error("leases", error));
                Vec::new()
            }
        };
        let attempts = match attempt_store.snapshot() {
            Ok(attempts) => attempts,
            Err(error) => {
                state_errors.push(runtime_state_error("attempts", error));
                Vec::new()
            }
        };
        let scheduled_needs_attention = match OccurrenceStore::new(ctx).snapshot() {
            Ok(occurrences) => occurrences
                .into_iter()
                .filter(|occurrence| {
                    active_scheduled_occurrence != Some(occurrence.occurrence_id.as_str())
                        && occurrence.requires_attention_at(checked_at_ms)
                })
                .collect(),
            Err(error) => {
                state_errors.push(runtime_state_error("scheduled occurrences", error));
                Vec::new()
            }
        };
        Self {
            checked_at_ms,
            live_leases,
            attempts,
            scheduled_needs_attention,
            state_errors,
        }
    }

    pub(super) fn error_text(&self) -> Option<String> {
        let errors = self
            .state_errors
            .iter()
            .filter_map(|error| error["error"].as_str())
            .collect::<Vec<_>>();
        (!errors.is_empty()).then(|| errors.join("; "))
    }
}

fn runtime_state_error(kind: &str, error: anyhow::Error) -> Value {
    json!({
        "kind": kind.replace(' ', "_"),
        "error": format!("Failed to inspect post-work loop {kind} state: {error:#}"),
    })
}
