use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::state::now_ms;

use super::super::occurrence::OccurrenceStore;
use super::super::state::{AttemptSections, AttemptStore};

pub(super) struct DispatchAttention {
    pub(super) scheduled_occurrence_count: u64,
    pub(super) exhausted_attempt_count: u64,
    pub(super) state_errors: Vec<Value>,
}

impl DispatchAttention {
    pub(super) fn collect(
        ctx: &RepoContext,
        occurrences: &OccurrenceStore,
        cancelled: &dyn Fn() -> bool,
    ) -> Self {
        // Workflow side effects are complete. Final aggregation is best-effort: cancellation
        // becomes structured state-error evidence and cannot prevent the dispatch receipt.
        let mut state_errors = Vec::new();
        let scheduled_occurrence_count =
            match occurrences.snapshot_read_only_with_cancellation(cancelled) {
                Ok(occurrences) => {
                    let checked_at_ms = now_ms();
                    bounded_count(
                        occurrences
                            .iter()
                            .filter(|occurrence| occurrence.requires_attention_at(checked_at_ms)),
                    )
                }
                Err(error) => {
                    state_errors.push(state_error("scheduled_occurrences", error));
                    0
                }
            };
        let exhausted_attempt_count =
            match AttemptStore::new(ctx).snapshot_read_only_with_cancellation(cancelled) {
                Ok(attempts) => bounded_count(
                    AttemptSections::new(&attempts, now_ms())
                        .needs_attention
                        .iter(),
                ),
                Err(error) => {
                    state_errors.push(state_error("attempts", error));
                    0
                }
            };
        Self {
            scheduled_occurrence_count,
            exhausted_attempt_count,
            state_errors,
        }
    }
}

fn bounded_count(items: impl Iterator) -> u64 {
    u64::try_from(items.count()).unwrap_or(u64::MAX)
}

fn state_error(kind: &str, error: anyhow::Error) -> Value {
    json!({
        "kind": kind,
        "error": format!("{error:#}"),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::runtime::loops::state::LOOP_RUNTIME_DIR;

    #[test]
    fn collection_reports_an_unreadable_occurrence_ledger_separately() {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let runtime_dir = temp.path().join(LOOP_RUNTIME_DIR);
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::write(runtime_dir.join("schedule.json"), b"not JSON").unwrap();
        let ctx = RepoContext::load_from(temp.path()).unwrap();

        let attention = DispatchAttention::collect(&ctx, &OccurrenceStore::new(&ctx), &|| false);

        assert_eq!(attention.state_errors.len(), 1);
        assert_eq!(attention.state_errors[0]["kind"], "scheduled_occurrences");
        assert_eq!(attention.exhausted_attempt_count, 0);
    }

    #[test]
    fn collection_turns_cancellation_into_bounded_state_errors() {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let runtime_dir = temp.path().join(LOOP_RUNTIME_DIR);
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::write(
            runtime_dir.join("schedule.json"),
            br#"{"schema_version":3,"occurrences":{}}"#,
        )
        .unwrap();
        let cache_dir = temp.path().join(".agent/.cache/loop");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(cache_dir.join("attempts.json"), br#"{"attempts":{}}"#).unwrap();
        let ctx = RepoContext::load_from(temp.path()).unwrap();

        let attention = DispatchAttention::collect(&ctx, &OccurrenceStore::new(&ctx), &|| true);

        assert_eq!(attention.state_errors.len(), 2);
        assert_eq!(attention.state_errors[0]["kind"], "scheduled_occurrences");
        assert_eq!(attention.state_errors[1]["kind"], "attempts");
        assert!(attention.state_errors.iter().all(|error| {
            error["error"]
                .as_str()
                .is_some_and(|error| error.contains("cancelled"))
        }));
    }

    #[test]
    fn collection_counts_an_expired_running_occurrence_as_attention() {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut occurrences = OccurrenceStore::new(&ctx);
        let _ = occurrences.claim("nightly", 100, 0).unwrap();

        let attention = DispatchAttention::collect(&ctx, &occurrences, &|| false);

        assert_eq!(attention.scheduled_occurrence_count, 1);
        assert!(attention.state_errors.is_empty());
    }
}
