use std::fs;

use jig_context::RepoContext;
use tempfile::tempdir;

use super::super::{NoopExecutionObserver, OccurrenceStore, dispatch_workflow, list_workflows};
use crate::occurrence::OccurrenceStatus;
use crate::state::{LeaseAcquire, LeaseStore};
use crate::test_env::TestRepoBuilder;

#[test]
fn successful_scheduled_work_requires_attention_when_its_evidence_fails() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "scheduled-noop"
kind = "noop_status"
schedule = "* * * * *"
"#,
        ),
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    block_evidence_directory(&ctx);
    let workflow = list_workflows(&ctx)
        .unwrap()
        .into_iter()
        .find(|workflow| workflow.id == "scheduled-noop")
        .unwrap();
    let mut occurrences = OccurrenceStore::new(&ctx);

    let step = dispatch_workflow(
        &ctx,
        &mut occurrences,
        &workflow,
        super::timestamp("2026-08-21T08:42:30Z"),
        &mut NoopExecutionObserver,
    );

    assert_eq!(step.executed_count, 1);
    assert_eq!(step.failed_count, 0);
    let action = step.action.as_ref().unwrap();
    assert_eq!(action["status"], "needs_attention", "{action:#}");
    assert_eq!(action["occurrence"]["status"], "needs_attention");
    assert!(
        action["occurrence"]["error"]
            .as_str()
            .is_some_and(|error| error.contains("Failed to record loop occurrence evidence")),
        "{action:#}"
    );
    let occurrence = occurrences.snapshot().unwrap().pop().unwrap();
    assert_eq!(occurrence.status, OccurrenceStatus::NeedsAttention);
}

#[test]
fn held_workflow_lease_is_abandoned_even_when_its_evidence_fails() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "scheduled-noop"
kind = "noop_status"
schedule = "* * * * *"
"#
        ),
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut leases = LeaseStore::new(&ctx);
    let LeaseAcquire::Acquired(_lease) = leases.acquire("workflow:scheduled-noop", 60).unwrap()
    else {
        panic!("expected workflow lease");
    };
    block_evidence_directory(&ctx);
    let workflow = list_workflows(&ctx)
        .unwrap()
        .into_iter()
        .find(|workflow| workflow.id == "scheduled-noop")
        .unwrap();
    let mut occurrences = OccurrenceStore::new(&ctx);

    let step = dispatch_workflow(
        &ctx,
        &mut occurrences,
        &workflow,
        super::timestamp("2026-08-21T08:42:30Z"),
        &mut NoopExecutionObserver,
    );

    assert_eq!(step.executed_count, 0);
    assert_eq!(step.deferred_count, 1);
    assert_eq!(step.skipped_count, 1);
    assert_eq!(step.action.as_ref().unwrap()["status"], "deferred");
    assert!(
        step.state_errors.iter().any(|error| error["error"]
            .as_str()
            .is_some_and(|error| error.contains("Failed to record loop occurrence evidence"))),
        "evidence failure should remain dispatch state evidence"
    );
    assert!(occurrences.snapshot().unwrap().is_empty());
}

/// Puts a file where occurrence evidence is recorded so that writes fail.
pub(super) fn block_evidence_directory(ctx: &RepoContext) {
    let directory = crate::evidence::directory_for_test(ctx).unwrap();
    fs::create_dir_all(directory.parent().unwrap()).unwrap();
    fs::write(directory, "not a directory").unwrap();
}
