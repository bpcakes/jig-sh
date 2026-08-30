use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::sync::atomic::{AtomicUsize, Ordering};

use tempfile::tempdir;

use super::super::super::engine::status_at_with_cancellation;
use super::super::super::occurrence::{OccurrenceFinish, OccurrenceOutcome};
use super::super::{
    NoopExecutionObserver, OccurrenceStore, blocking_retained_task_checkout, dispatch_workflow,
    list_workflows,
};
use crate::command::LoopStatusRequest;
use crate::context::RepoContext;
use crate::test_env::TestRepoBuilder;
#[cfg(unix)]
use crate::test_env::{EnvVarGuard, lock_env};

struct AlwaysCancelled;

impl crate::execution::ExecutionObserver for AlwaysCancelled {}

impl crate::execution::ExecutionCancellation for AlwaysCancelled {
    fn cancelled(&self) -> bool {
        true
    }
}

#[cfg(unix)]
struct CancelAfterFirstCheck(AtomicUsize);

#[cfg(unix)]
impl crate::execution::ExecutionObserver for CancelAfterFirstCheck {}

#[cfg(unix)]
impl crate::execution::ExecutionCancellation for CancelAfterFirstCheck {
    fn cancelled(&self) -> bool {
        self.0.fetch_add(1, Ordering::SeqCst) > 0
    }
}

#[test]
fn status_keeps_other_workflows_visible_when_one_schedule_cannot_be_evaluated() {
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
timezone = "UTC"
"#
        ),
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = status_at_with_cancellation(
        &ctx,
        LoopStatusRequest { workflow: None },
        &|| false,
        u64::MAX,
    )
    .unwrap();

    assert_eq!(output["ok"], false, "{output:#}");
    assert_eq!(output["state_error_count"], 1, "{output:#}");
    assert_eq!(output["state_errors"][0]["workflow_id"], "scheduled-noop");
    let workflows = output["workflows"].as_array().unwrap();
    assert!(
        workflows
            .iter()
            .any(|workflow| workflow["id"] == "noop-status"),
        "{output:#}"
    );
    let scheduled = workflows
        .iter()
        .find(|workflow| workflow["id"] == "scheduled-noop")
        .unwrap();
    assert!(scheduled["schedule_state"].is_null(), "{output:#}");
    assert!(scheduled["schedule_state_error"].is_string(), "{output:#}");
}

#[test]
fn retained_task_worktree_blocks_the_next_scheduled_occurrence() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    fs::create_dir_all(temp.path().join(".agent/tasks")).unwrap();
    fs::write(temp.path().join(".agent/tasks/nightly.md"), "Review it.\n").unwrap();
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "nightly-task"
kind = "codex_task"
schedule = "* * * * *"
prompt_file = ".agent/tasks/nightly.md"
checkout = "worktree"
"#
        ),
    )
    .unwrap();
    let retained = temp.path().join("retained-task-worktree");
    fs::create_dir(&retained).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let workflow = list_workflows(&ctx)
        .unwrap()
        .into_iter()
        .find(|workflow| workflow.id == "nightly-task")
        .unwrap();
    let mut occurrences = OccurrenceStore::new(&ctx);
    let super::super::OccurrenceClaim::Acquired(previous) =
        occurrences.claim("nightly-task", 100, 60).unwrap()
    else {
        panic!("expected occurrence claim");
    };
    occurrences
        .finish(
            &previous.occurrence_id,
            &previous.owner,
            OccurrenceFinish {
                outcome: OccurrenceOutcome::Failed,
                worker_receipt_id: Some("receipt-example"),
                worktree: retained.to_str(),
                error: Some("worker failed"),
            },
        )
        .unwrap();
    let known = occurrences.snapshot().unwrap();

    let step = dispatch_workflow(
        &ctx,
        &mut occurrences,
        &known,
        &workflow,
        super::timestamp("2026-08-21T08:42:30Z"),
        &mut NoopExecutionObserver,
    );

    assert_eq!(step.executed_count, 0);
    assert_eq!(step.skipped_count, 1);
    assert_eq!(step.failed_count, 1);
    assert_eq!(
        step.action.as_ref().unwrap()["reason"],
        "retained_worktree_requires_cleanup"
    );
    assert_eq!(occurrences.snapshot().unwrap().len(), 1);

    fs::remove_dir(&retained).unwrap();
    assert!(blocking_retained_task_checkout(&known, &workflow).is_none());
}

#[test]
fn cancellation_before_workflow_start_abandons_and_retries_the_occurrence() {
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
    let workflow = list_workflows(&ctx)
        .unwrap()
        .into_iter()
        .find(|workflow| workflow.id == "scheduled-noop")
        .unwrap();
    let dispatch_at = super::timestamp("2026-08-21T08:42:30Z");
    let mut occurrences = OccurrenceStore::new(&ctx);

    let cancelled = dispatch_workflow(
        &ctx,
        &mut occurrences,
        &[],
        &workflow,
        dispatch_at,
        &mut AlwaysCancelled,
    );

    assert_eq!(cancelled.executed_count, 0);
    assert_eq!(cancelled.skipped_count, 1);
    assert_eq!(cancelled.failed_count, 1);
    assert_eq!(cancelled.action.as_ref().unwrap()["retryable"], true);
    assert!(occurrences.snapshot().unwrap().is_empty());

    let retried = dispatch_workflow(
        &ctx,
        &mut occurrences,
        &[],
        &workflow,
        dispatch_at,
        &mut NoopExecutionObserver,
    );
    assert_eq!(retried.executed_count, 1, "{:#?}", retried.action);
}

#[cfg(unix)]
#[test]
fn cancellation_at_worker_start_abandons_the_occurrence_without_running_codex() {
    let _env_lock = lock_env();
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    fs::create_dir_all(temp.path().join(".agent/tasks")).unwrap();
    fs::write(temp.path().join(".agent/tasks/nightly.md"), "Review it.\n").unwrap();
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "nightly-task"
kind = "codex_task"
schedule = "* * * * *"
prompt_file = ".agent/tasks/nightly.md"
checkout = "repo"
"#
        ),
    )
    .unwrap();
    let marker = temp.path().join("codex-started");
    let codex = temp.path().join("codex-stub.sh");
    fs::write(&codex, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).unwrap();
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let workflow = list_workflows(&ctx)
        .unwrap()
        .into_iter()
        .find(|workflow| workflow.id == "nightly-task")
        .unwrap();
    let mut occurrences = OccurrenceStore::new(&ctx);

    let cancelled = dispatch_workflow(
        &ctx,
        &mut occurrences,
        &[],
        &workflow,
        super::timestamp("2026-08-21T08:42:30Z"),
        &mut CancelAfterFirstCheck(AtomicUsize::new(0)),
    );

    assert_eq!(cancelled.executed_count, 0, "{:#?}", cancelled.action);
    assert_eq!(
        cancelled.action.as_ref().unwrap()["reason"],
        "cancelled_before_start"
    );
    assert_eq!(cancelled.action.as_ref().unwrap()["retryable"], true);
    assert!(occurrences.snapshot().unwrap().is_empty());
    assert!(!marker.exists(), "Codex must not spawn after cancellation");
}
