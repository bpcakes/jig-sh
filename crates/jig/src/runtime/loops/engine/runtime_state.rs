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
        lease_store: &LeaseStore,
        attempt_store: &AttemptStore,
        active_scheduled_occurrence: Option<&str>,
        cancelled: &dyn Fn() -> bool,
    ) -> Self {
        let checked_at_ms = now_ms();
        let mut state_errors = Vec::new();
        let live_leases = match lease_store.active_leases_read_only_with_cancellation(cancelled) {
            Ok(leases) => leases,
            Err(error) => {
                state_errors.push(runtime_state_error("leases", error));
                Vec::new()
            }
        };
        let attempts = match attempt_store.snapshot_read_only_with_cancellation(cancelled) {
            Ok(attempts) => attempts,
            Err(error) => {
                state_errors.push(runtime_state_error("attempts", error));
                Vec::new()
            }
        };
        let scheduled_needs_attention =
            match OccurrenceStore::new(ctx).snapshot_read_only_with_cancellation(cancelled) {
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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::test_env::TestRepoBuilder;

    #[test]
    fn post_work_collection_observes_cancellation_without_initializing_state() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let cache_dir = temp.path().join(".agent/.cache/loop");
        let runtime_dir = temp.path().join(".agent/runtime/loop");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::create_dir_all(&runtime_dir).unwrap();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let leases = LeaseStore::new(&ctx);
        let attempts = AttemptStore::new(&ctx);

        let state = TickRuntimeState::collect(&ctx, &leases, &attempts, None, &|| true);

        assert_eq!(state.state_errors.len(), 3, "{:?}", state.state_errors);
        assert!(state.state_errors.iter().all(|error| {
            error["error"]
                .as_str()
                .is_some_and(|error| error.to_ascii_lowercase().contains("cancel"))
        }));
        assert!(fs::read_dir(cache_dir).unwrap().next().is_none());
        assert!(fs::read_dir(runtime_dir).unwrap().next().is_none());
    }
}
