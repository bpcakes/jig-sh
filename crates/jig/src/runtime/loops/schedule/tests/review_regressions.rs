use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::sync::atomic::{AtomicUsize, Ordering};

use tempfile::tempdir;

use super::super::super::engine::status_at_with_cancellation;
use super::super::super::occurrence::{OccurrenceFinish, OccurrenceOutcome};
use super::super::super::state::AttemptStore;
use super::super::{NoopExecutionObserver, OccurrenceStore, dispatch_workflow, list_workflows};
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
fn status_uses_its_snapshot_clock_for_attempt_backoff() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let workflow = list_workflows(&ctx)
        .unwrap()
        .into_iter()
        .find(|workflow| workflow.id == "noop-status")
        .unwrap();
    let mut attempts = AttemptStore::new(&ctx);
    attempts
        .record_attempt_for_version(&workflow, "ExampleProject", None, "failed")
        .unwrap();

    let output = status_at_with_cancellation(
        &ctx,
        LoopStatusRequest { workflow: None },
        &|| false,
        u64::MAX,
    )
    .unwrap();

    assert_eq!(output["waiting_attempts"], serde_json::json!([]));
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
    let step = dispatch_workflow(
        &ctx,
        &mut occurrences,
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
    let super::super::OccurrenceClaim::Acquired(_) = occurrences
        .claim_scheduled("nightly-task", 200, 60, true)
        .unwrap()
    else {
        panic!("removing the retained checkout must unblock the workflow");
    };
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

#[cfg(unix)]
#[test]
fn codex_setup_failure_retries_the_same_scheduled_occurrence() {
    let _env_lock = lock_env();
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    for args in [
        vec!["init"],
        vec!["config", "user.email", "fixture@example.com"],
        vec!["config", "user.name", "Fixture"],
        vec!["add", "."],
        vec!["commit", "-m", "fixture"],
    ] {
        let output = std::process::Command::new("git")
            .current_dir(temp.path())
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    fs::create_dir_all(temp.path().join(".agent/tasks")).unwrap();
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
    let codex = temp.path().join("codex-stub.sh");
    fs::write(&codex, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).unwrap();
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let workflow = list_workflows(&ctx)
        .unwrap()
        .into_iter()
        .find(|workflow| workflow.id == "nightly-task")
        .unwrap();
    let dispatch_at = super::timestamp("2026-08-21T08:42:30Z");
    let mut occurrences = OccurrenceStore::new(&ctx);

    let failed = dispatch_workflow(
        &ctx,
        &mut occurrences,
        &workflow,
        dispatch_at,
        &mut NoopExecutionObserver,
    );

    assert_eq!(failed.executed_count, 0, "{:#?}", failed.action);
    assert_eq!(
        failed.action.as_ref().unwrap()["reason"],
        "pre_execution_error"
    );
    assert_eq!(failed.action.as_ref().unwrap()["retryable"], true);
    assert!(occurrences.snapshot().unwrap().is_empty());

    fs::write(temp.path().join(".agent/tasks/nightly.md"), "Review it.\n").unwrap();
    let retried = dispatch_workflow(
        &ctx,
        &mut occurrences,
        &workflow,
        dispatch_at,
        &mut NoopExecutionObserver,
    );
    assert_eq!(retried.executed_count, 1, "{:#?}", retried.action);
    assert_eq!(
        retried.action.as_ref().unwrap()["status"],
        "succeeded",
        "{:#?}",
        retried.action
    );
}

#[cfg(unix)]
#[test]
fn unexecuted_setup_with_a_retained_checkout_requires_attention() {
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
checkout = "worktree"
"#
        ),
    )
    .unwrap();
    let git = temp.path().join("git-stub.sh");
    fs::write(
        &git,
        r#"#!/bin/sh
set -eu
case " $* " in
  *" check-ignore --quiet -- "*) exit 0 ;;
  *" rev-parse HEAD "*) printf 'initial-head\n' ;;
  *" worktree add "*)
    previous=
    for argument in "$@"; do
      if [ "$previous" = "--detach" ]; then
        mkdir -p "$argument"
        : > "$argument/.git"
      fi
      previous=$argument
    done
    ;;
  *" status --porcelain=v1 "*) exit 0 ;;
  *" worktree remove "*) exit 4 ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
    let missing_codex = temp.path().join("missing-codex");
    let _git = EnvVarGuard::set(crate::bootstrap::GIT_BIN_ENV, git.as_os_str());
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", missing_codex.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let workflow = list_workflows(&ctx)
        .unwrap()
        .into_iter()
        .find(|workflow| workflow.id == "nightly-task")
        .unwrap();
    let mut occurrences = OccurrenceStore::new(&ctx);

    let failed = dispatch_workflow(
        &ctx,
        &mut occurrences,
        &workflow,
        super::timestamp("2026-08-21T08:42:30Z"),
        &mut NoopExecutionObserver,
    );

    assert_eq!(failed.executed_count, 0, "{:#?}", failed.action);
    assert_eq!(failed.action.as_ref().unwrap()["status"], "needs_attention");
    let records = occurrences.snapshot().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].status,
        super::super::OccurrenceStatus::NeedsAttention
    );
    assert!(
        records[0]
            .worktree
            .as_deref()
            .is_some_and(|path| std::path::Path::new(path).exists())
    );

    let blocked = dispatch_workflow(
        &ctx,
        &mut occurrences,
        &workflow,
        super::timestamp("2026-08-21T08:43:30Z"),
        &mut NoopExecutionObserver,
    );
    assert_eq!(
        blocked.action.as_ref().unwrap()["reason"],
        "occurrence_requires_attention"
    );
    assert_eq!(occurrences.snapshot().unwrap().len(), 1);
}
