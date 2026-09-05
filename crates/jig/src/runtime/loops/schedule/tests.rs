use std::fs::{self, OpenOptions};
use std::thread;
use std::time::{Duration, Instant};

use chrono::DateTime;
use fs4::fs_std::FileExt;
use serde_json::json;
use tempfile::tempdir;

use super::super::engine::{ScheduledTick, WorkflowLeaseDisposition};
use super::super::occurrence::{OccurrenceFinalization, OccurrenceStatus, ScheduleOccurrence};
use super::super::state::{LOOP_CACHE_DIR, LeaseAcquire, LeaseStore};
use super::super::workflow::{RepositoryRevisionState, WorkflowCompletion, WorkflowOutcome};
use super::{
    DispatchStep, DispatchSummary, NoopExecutionObserver, OccurrenceStore, RunSummary,
    RunTickDisposition, ScheduleSpec, abandon_unexecuted_start_failure, dispatch_due_at,
    dispatch_workflow, include_occurrence_renewal_error, list_workflows,
    occurrence_from_finalization, scheduled_tick_state_errors,
};
use crate::command::LoopStatusRequest;
use crate::context::RepoContext;
use crate::test_env::TestRepoBuilder;

#[path = "tests/post_work_evidence.rs"]
mod post_work_evidence;
#[path = "tests/receipt_evidence.rs"]
mod receipt_evidence;
#[path = "tests/review_regressions.rs"]
mod review_regressions;
#[path = "tests/review_round14.rs"]
mod review_round14;

#[test]
fn schedule_window_coalesces_missed_occurrences() {
    let schedule = ScheduleSpec::parse("0 2 * * *", Some("UTC")).unwrap();
    let now = timestamp("2026-08-21T02:30:00Z");
    let window = schedule
        .window(now, Some(timestamp("2026-08-18T02:00:00Z")))
        .unwrap();

    assert_eq!(window.due_at_ms, Some(timestamp("2026-08-21T02:00:00Z")));
    assert_eq!(window.next_at_ms, timestamp("2026-08-22T02:00:00Z"));
    assert_eq!(
        schedule.window(now, window.due_at_ms).unwrap().due_at_ms,
        None
    );
}

#[test]
fn schedule_window_uses_canonical_occurrence_timestamps() {
    let schedule = ScheduleSpec::parse("* * * * *", Some("UTC")).unwrap();
    let first = schedule
        .window(timestamp("2026-08-21T02:30:00Z") + 767, None)
        .unwrap();
    let second = schedule
        .window(timestamp("2026-08-21T02:30:00Z") + 768, None)
        .unwrap();

    assert_eq!(first, second);
    assert_eq!(first.due_at_ms, Some(timestamp("2026-08-21T02:30:00Z")));
    assert_eq!(first.next_at_ms, timestamp("2026-08-21T02:31:00Z"));
}

#[test]
fn run_tick_disposition_preserves_terminal_status_policy() {
    assert_eq!(
        RunTickDisposition::from_tick(&json!({"status": "failed", "idle": false})),
        RunTickDisposition::Failed
    );
    assert_eq!(
        RunTickDisposition::from_tick(&json!({"status": "waiting", "idle": false})),
        RunTickDisposition::Stop("waiting")
    );
    assert_eq!(
        RunTickDisposition::from_tick(&json!({"status": "acted", "idle": true})),
        RunTickDisposition::Stop("idle")
    );
    assert_eq!(
        RunTickDisposition::from_tick(&json!({"status": "acted", "idle": false})),
        RunTickDisposition::Continue
    );
}

#[test]
fn failed_run_ticks_continue_but_determine_the_final_status() {
    let mut summary = RunSummary::default();

    assert!(!summary.observe(RunTickDisposition::Failed));
    assert!(summary.observe(RunTickDisposition::Stop("waiting")));
    assert_eq!(summary.status(), "failed");
}

#[test]
fn dispatch_summary_uses_one_attention_policy_for_status_and_success() {
    let mut summary = DispatchSummary {
        executed_count: 1,
        exhausted_attempt_count: 1,
        ..DispatchSummary::default()
    };

    assert_eq!(summary.status(), "needs_attention");
    assert!(!super::loop_status_is_success(summary.status()));

    summary.failed_count = 1;
    assert_eq!(summary.status(), "failed");
    assert!(!super::loop_status_is_success(summary.status()));
}

#[test]
fn dispatch_summary_distinguishes_deferred_work_from_idle() {
    let summary = DispatchSummary {
        due_count: 1,
        deferred_count: 1,
        ..DispatchSummary::default()
    };

    assert_eq!(summary.status(), "deferred");
    assert!(super::loop_status_is_success(summary.status()));
}

#[test]
fn failed_workflow_keeps_a_separate_tick_receipt_error() {
    let tick = ScheduledTick::Errored {
        value: None,
        completion: WorkflowCompletion {
            outcome: WorkflowOutcome::Failed,
            error: Some("worker failed".into()),
            ..WorkflowCompletion::default()
        },
        lease_disposition: WorkflowLeaseDisposition::Acquired,
        state_errors: Vec::new(),
        error: "Failed to record loop tick receipt: disk full".into(),
        post_work_error: Some("Failed to record loop tick receipt: disk full".into()),
    };

    let errors = scheduled_tick_state_errors(&tick, "nightly", "nightly@100");

    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["kind"], "tick");
    assert_eq!(errors[0]["workflow_id"], "nightly");
    assert_eq!(errors[0]["occurrence_id"], "nightly@100");
    assert_eq!(
        errors[0]["error"],
        "Failed to record loop tick receipt: disk full"
    );
}

#[test]
fn tick_receipt_failure_preserves_the_typed_repository_revision_cutoff() {
    let tick = ScheduledTick::Errored {
        value: None,
        completion: WorkflowCompletion {
            repository_revision: RepositoryRevisionState::Changed,
            ..WorkflowCompletion::default()
        },
        lease_disposition: WorkflowLeaseDisposition::Acquired,
        state_errors: Vec::new(),
        error: "Failed to record loop tick receipt: disk full".into(),
        post_work_error: Some("Failed to record loop tick receipt: disk full".into()),
    };

    assert!(
        tick.completion()
            .repository_revision
            .requires_dispatch_stop()
    );
    assert!(tick.completion().repository_revision.changed());
    assert!(tick.value().is_none());
}

#[test]
fn persisted_finalization_keeps_renewal_failure_as_dispatch_state_evidence() {
    let mut step = DispatchStep::default();
    include_occurrence_renewal_error(
        &mut step,
        "nightly",
        "nightly@100",
        Some("injected renewal failure".into()),
    );
    let mut summary = DispatchSummary::default();

    summary.include(&step);

    assert_eq!(summary.status(), "failed");
    assert_eq!(summary.failed_count, 0);
    assert_eq!(summary.state_error_count, 1);
    assert_eq!(summary.state_errors[0]["kind"], "occurrence_renewal");
    assert_eq!(summary.state_errors[0]["workflow_id"], "nightly");
    assert_eq!(summary.state_errors[0]["occurrence_id"], "nightly@100");
}

#[test]
fn unexecuted_finalization_keeps_renewal_failure_as_dispatch_state_evidence() {
    let occurrence = ScheduleOccurrence {
        occurrence_id: "nightly@100".into(),
        workflow_id: "nightly".into(),
        scheduled_at_ms: 100,
        owner: "owner".into(),
        claim_expires_at_ms: 200,
        started_at_ms: 100,
        uses_shared_checkout: Some(false),
        finished_at_ms: None,
        acknowledged_at_ms: None,
        status: OccurrenceStatus::Running,
        worker_receipt_id: None,
        worktree: None,
        error: None,
    };
    let mut step = DispatchStep::default();

    let returned = occurrence_from_finalization(
        &mut step,
        "nightly",
        "nightly@100",
        OccurrenceFinalization {
            occurrence: occurrence.clone(),
            renewal_error: Some("injected renewal failure".into()),
            renewal_ownership_lost: false,
        },
        true,
    );

    assert_eq!(returned, occurrence);
    assert_eq!(step.state_errors.len(), 1);
    assert_eq!(step.state_errors[0]["kind"], "occurrence_renewal");
    assert_eq!(
        step.state_errors[0]["error"],
        "Occurrence renewal failed before terminal state was recorded: injected renewal failure"
    );
}

#[test]
fn unexecuted_finalization_suppresses_only_expected_ownership_loss() {
    let occurrence = ScheduleOccurrence {
        occurrence_id: "nightly@100".into(),
        workflow_id: "nightly".into(),
        scheduled_at_ms: 100,
        owner: "owner".into(),
        claim_expires_at_ms: 200,
        started_at_ms: 100,
        uses_shared_checkout: Some(false),
        finished_at_ms: None,
        acknowledged_at_ms: None,
        status: OccurrenceStatus::Running,
        worker_receipt_id: None,
        worktree: None,
        error: None,
    };
    let mut step = DispatchStep::default();

    let returned = occurrence_from_finalization(
        &mut step,
        "nightly",
        "nightly@100",
        OccurrenceFinalization {
            occurrence: occurrence.clone(),
            renewal_error: Some("occurrence ownership was deliberately removed".into()),
            renewal_ownership_lost: true,
        },
        true,
    );

    assert_eq!(returned, occurrence);
    assert!(step.state_errors.is_empty());
}

#[test]
fn schedule_window_uses_explicit_timezone() {
    let schedule = ScheduleSpec::parse("0 9 * * MON-FRI", Some("Europe/Prague")).unwrap();
    let window = schedule
        .window(timestamp("2026-08-21T08:00:00Z"), None)
        .unwrap();

    assert_eq!(window.due_at_ms, Some(timestamp("2026-08-21T07:00:00Z")));
    assert_eq!(window.next_at_ms, timestamp("2026-08-24T07:00:00Z"));
    assert_eq!(schedule.expression(), "0 9 * * MON-FRI");
    assert_eq!(schedule.timezone_name(), "Europe/Prague");
}

#[test]
fn schedule_window_skips_nonexistent_spring_forward_time() {
    let schedule = ScheduleSpec::parse("30 2 * * *", Some("Europe/Prague")).unwrap();
    let window = schedule
        .window(timestamp("2026-03-29T02:00:00Z"), None)
        .unwrap();

    assert_eq!(window.due_at_ms, Some(timestamp("2026-03-28T01:30:00Z")));
    assert_eq!(window.next_at_ms, timestamp("2026-03-30T00:30:00Z"));
}

#[test]
fn schedule_window_runs_repeated_fall_back_wall_time_once() {
    let schedule = ScheduleSpec::parse("30 2 * * *", Some("Europe/Prague")).unwrap();
    let first = timestamp("2026-10-25T00:30:00Z");
    let window = schedule
        .window(timestamp("2026-10-25T01:45:00Z"), Some(first))
        .unwrap();

    assert_eq!(window.due_at_ms, None);
    assert_eq!(window.next_at_ms, timestamp("2026-10-26T01:30:00Z"));
}

#[test]
fn dispatcher_claims_each_due_occurrence_once() {
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
    let dispatch_at = timestamp("2026-08-21T08:42:30Z");

    let first = dispatch_due_at(&ctx, dispatch_at).unwrap();
    fs::remove_dir_all(temp.path().join(LOOP_CACHE_DIR)).unwrap();
    let second = dispatch_due_at(&ctx, dispatch_at).unwrap();

    assert_eq!(first["status"], "acted", "{first:#}");
    assert_eq!(first["due_count"], 1);
    assert_eq!(first["executed_count"], 1);
    assert_eq!(
        first["actions"][0]["occurrence"]["scheduled_at_ms"],
        timestamp("2026-08-21T08:42:00Z")
    );
    assert_eq!(second["status"], "idle", "{second:#}");
    assert_eq!(second["due_count"], 0);
    assert_eq!(second["executed_count"], 0);
    assert!(
        temp.path()
            .join(super::super::state::LOOP_RUNTIME_DIR)
            .join("schedule.json")
            .is_file()
    );

    let status = super::super::engine::status(
        &ctx,
        LoopStatusRequest {
            workflow: Some("scheduled-noop".into()),
        },
    )
    .unwrap();
    assert_eq!(
        status["workflows"][0]["schedule_state"]["last_status"],
        "succeeded"
    );
    assert!(
        status["workflows"][0]["schedule_state"]["next_at_ms"]
            .as_u64()
            .is_some()
    );
    assert_eq!(status["scheduled_occurrences"].as_array().unwrap().len(), 1);
    serde_json::from_value::<jig_ui::dashboard::StatusLoopObservation>(status).unwrap();
}

#[test]
fn dispatcher_retries_after_a_pre_execution_lease_error() {
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
    let cache_dir = temp.path().join(LOOP_CACHE_DIR);
    fs::create_dir_all(cache_dir.join("leases.json")).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let dispatch_at = timestamp("2026-08-21T08:42:30Z");

    let workflow = list_workflows(&ctx)
        .unwrap()
        .into_iter()
        .find(|workflow| workflow.id == "scheduled-noop")
        .unwrap();
    let mut occurrences = OccurrenceStore::new(&ctx);
    let failed = dispatch_workflow(
        &ctx,
        &mut occurrences,
        &workflow,
        dispatch_at,
        &mut NoopExecutionObserver,
    );

    assert_eq!(failed.action.as_ref().unwrap()["status"], "failed");
    assert_eq!(failed.executed_count, 0);
    assert!(OccurrenceStore::new(&ctx).snapshot().unwrap().is_empty());

    fs::remove_dir(cache_dir.join("leases.json")).unwrap();
    let retried = dispatch_workflow(
        &ctx,
        &mut occurrences,
        &workflow,
        dispatch_at,
        &mut NoopExecutionObserver,
    );
    assert_eq!(retried.action.as_ref().unwrap()["status"], "succeeded");
    assert_eq!(retried.executed_count, 1);
}

#[test]
fn occurrence_guard_start_failure_abandons_the_unexecuted_claim() {
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
    let mut occurrences = OccurrenceStore::new(&ctx);
    let super::OccurrenceClaim::Acquired(claim) = occurrences
        .claim(&workflow.id, 100, workflow.lease_ttl_seconds)
        .unwrap()
    else {
        panic!("expected occurrence claim");
    };

    let step = abandon_unexecuted_start_failure(
        DispatchStep {
            due_count: 1,
            ..DispatchStep::default()
        },
        &mut occurrences,
        &workflow,
        &claim,
        200,
        "injected occurrence renewal start failure".into(),
    );

    assert_eq!(step.executed_count, 0);
    assert_eq!(step.skipped_count, 1);
    assert_eq!(step.failed_count, 1);
    assert_eq!(step.action.as_ref().unwrap()["retryable"], true);
    assert!(occurrences.snapshot().unwrap().is_empty());
}

#[test]
fn abandonment_state_failure_still_counts_the_due_work_as_skipped() {
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
    let mut occurrences = OccurrenceStore::new(&ctx);
    let super::OccurrenceClaim::Acquired(claim) = occurrences
        .claim(&workflow.id, 100, workflow.lease_ttl_seconds)
        .unwrap()
    else {
        panic!("expected occurrence claim");
    };
    fs::write(
        temp.path().join(".agent/runtime/loop/schedule.json"),
        "not JSON\n",
    )
    .unwrap();

    let step = abandon_unexecuted_start_failure(
        DispatchStep {
            due_count: 1,
            ..DispatchStep::default()
        },
        &mut occurrences,
        &workflow,
        &claim,
        200,
        "injected start failure".into(),
    );

    assert_eq!(step.executed_count, 0);
    assert_eq!(step.skipped_count, 1);
    assert_eq!(step.failed_count, 1);
    assert!(
        step.action.as_ref().unwrap()["error"]
            .as_str()
            .unwrap()
            .contains("abandoning the unexecuted occurrence also failed")
    );
}

#[test]
fn dispatcher_persistently_fails_while_occurrence_needs_attention() {
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
    let cache = temp.path().join(LOOP_CACHE_DIR);
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("schedule.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "occurrences": {
                "scheduled-noop@1787301600000": {
                    "occurrence_id": "scheduled-noop@1787301600000",
                    "workflow_id": "scheduled-noop",
                    "scheduled_at_ms": 1_787_301_600_000_u64,
                    "owner": "crashed-dispatcher",
                    "claim_expires_at_ms": 1,
                    "started_at_ms": 1,
                    "status": "running"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let dispatch_at = timestamp("2026-08-21T08:42:30Z");

    let first = dispatch_due_at(&ctx, dispatch_at).unwrap();
    let second = dispatch_due_at(&ctx, dispatch_at).unwrap();

    assert_eq!(first["status"], "needs_attention", "{first:#}");
    assert_eq!(first["ok"], false);
    assert_eq!(first["needs_attention_count"], 1);
    assert_eq!(first["reconciled_occurrences"].as_array().unwrap().len(), 1);
    assert_eq!(second["status"], "needs_attention", "{second:#}");
    assert_eq!(second["ok"], false);
    assert_eq!(second["needs_attention_count"], 1);
    assert!(
        second["reconciled_occurrences"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let mut occurrences = super::super::occurrence::OccurrenceStore::new(&ctx);
    occurrences
        .acknowledge("scheduled-noop@1787301600000")
        .unwrap();
    let acknowledged = dispatch_due_at(&ctx, dispatch_at).unwrap();
    assert_eq!(acknowledged["status"], "acted", "{acknowledged:#}");
    assert_eq!(acknowledged["due_count"], 1);
    assert_eq!(acknowledged["executed_count"], 1);
    assert_eq!(acknowledged["needs_attention_count"], 0);
}

#[test]
fn dispatcher_defers_without_consuming_occurrence_when_workflow_lease_is_held() {
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
    let LeaseAcquire::Acquired(lease) = leases.acquire("workflow:scheduled-noop", 60).unwrap()
    else {
        panic!("expected workflow lease");
    };
    let dispatch_at = timestamp("2026-08-21T08:42:30Z");

    let deferred = dispatch_due_at(&ctx, dispatch_at).unwrap();
    assert_eq!(deferred["status"], "deferred", "{deferred:#}");
    assert_eq!(deferred["actions"][0]["status"], "deferred");
    assert_eq!(deferred["executed_count"], 0);
    assert_eq!(deferred["deferred_count"], 1);
    assert_eq!(
        deferred["skipped_count"], 1,
        "schema-version-1 skipped_count keeps its broad compatibility meaning"
    );
    leases
        .release("workflow:scheduled-noop", &lease.owner)
        .unwrap();

    let retried = dispatch_due_at(&ctx, dispatch_at).unwrap();
    assert_eq!(retried["status"], "acted", "{retried:#}");
    assert_eq!(retried["executed_count"], 1);
}

#[test]
fn dispatcher_retries_an_expired_unexecuted_claim_when_workflow_lease_is_held() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[loop]
lease_ttl_seconds = 60

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
    let LeaseAcquire::Acquired(lease) = leases.acquire("workflow:scheduled-noop", 60).unwrap()
    else {
        panic!("expected workflow lease");
    };

    let receipt_lock_dir = temp.path().join(".agent/.cache/state-locks");
    fs::create_dir_all(&receipt_lock_dir).unwrap();
    let receipt_lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(receipt_lock_dir.join("receipts.jsonl.lock"))
        .unwrap();
    receipt_lock.lock_exclusive().unwrap();

    let root = temp.path().to_path_buf();
    let dispatch_at = timestamp("2026-08-21T08:42:30Z");
    let dispatcher = thread::spawn(move || {
        let ctx = RepoContext::load_from(&root).unwrap();
        dispatch_due_at(&ctx, dispatch_at)
    });

    let runtime_dir = temp.path().join(super::super::state::LOOP_RUNTIME_DIR);
    let schedule_path = runtime_dir.join("schedule.json");
    let running_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if fs::read(&schedule_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .is_some_and(|schedule| {
                schedule["occurrences"]
                    .as_object()
                    .is_some_and(|occurrences| !occurrences.is_empty())
            })
        {
            break;
        }
        assert!(
            Instant::now() < running_deadline,
            "dispatcher did not publish its occurrence claim"
        );
        thread::sleep(Duration::from_millis(10));
    }

    let mut competing_dispatcher = OccurrenceStore::new(&ctx);
    // The sampled maximum time deterministically expires the claim without making
    // the later successful retry depend on a production lease expiring in real time.
    let reconciled = competing_dispatcher
        .reconcile_stale_for_test(u64::MAX)
        .unwrap();
    assert_eq!(reconciled.len(), 1);
    FileExt::unlock(&receipt_lock).unwrap();

    let deferred = dispatcher.join().unwrap().unwrap();
    assert_eq!(deferred["status"], "deferred", "{deferred:#}");
    assert_eq!(deferred["actions"][0]["status"], "deferred", "{deferred:#}");
    assert_eq!(
        deferred["actions"][0]["occurrence"]["status"], "needs_attention",
        "removed dispatch evidence must preserve the stale state that actually existed"
    );
    assert_eq!(deferred["deferred_count"], 1, "{deferred:#}");
    assert_eq!(deferred["needs_attention_count"], 0, "{deferred:#}");
    let occurrences = OccurrenceStore::new(&ctx);
    assert!(occurrences.snapshot().unwrap().is_empty());

    leases
        .release("workflow:scheduled-noop", &lease.owner)
        .unwrap();
    let retried = dispatch_due_at(&ctx, dispatch_at).unwrap();
    assert_eq!(retried["status"], "acted", "{retried:#}");
    assert_eq!(retried["executed_count"], 1);
}

#[test]
fn held_workflow_lease_is_abandoned_even_when_tick_receipt_fails() {
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
    fs::create_dir_all(temp.path().join(".agent/state/receipts.jsonl")).unwrap();
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
        timestamp("2026-08-21T08:42:30Z"),
        &mut NoopExecutionObserver,
    );

    assert_eq!(step.executed_count, 0);
    assert_eq!(step.deferred_count, 1);
    assert_eq!(step.skipped_count, 1);
    assert_eq!(step.action.as_ref().unwrap()["status"], "deferred");
    assert!(
        step.state_errors.iter().any(|error| error["error"]
            .as_str()
            .is_some_and(|error| error.contains("Failed to record loop tick receipt"))),
        "receipt failure should remain dispatch state evidence"
    );
    assert!(occurrences.snapshot().unwrap().is_empty());
}

fn timestamp(value: &str) -> u64 {
    u64::try_from(
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .timestamp_millis(),
    )
    .unwrap()
}
