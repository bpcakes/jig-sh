use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use fs4::fs_std::FileExt;
use jig_contract::{
    Finding, FindingSeverity, RunConclusion, RunPlan, RunResult, RunStatus, TargetId,
    TargetRunResult,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::cell::Cell;

use crate::context::RepoContext;

use super::jsonl::{
    RawJsonlRecord, append_jsonl, jsonl_end_offset, scan_jsonl_raw, scan_jsonl_raw_from,
};
use super::records::RunEventRecord;
use super::support::{ensure_state_layout, new_id, now_ms};

const RUNS_FILE: &str = "runs.jsonl";
const RUN_LEASE_DIR: &str = ".agent/.cache/run-leases";
const EVENT_QUEUED: &str = "queued";
const EVENT_RUNNING: &str = "running";
const EVENT_TARGET_STARTED: &str = "target_started";
const EVENT_TARGET_COMPLETED: &str = "target_completed";
const EVENT_COMPLETED: &str = "completed";
const EVENT_CANCEL_REQUESTED: &str = "cancel_requested";

#[cfg(test)]
thread_local! {
    static FULL_RUN_EVENT_PARSE_COUNT: Cell<usize> = const { Cell::new(0) };
}

/// The accepted plan and current state reconstructed from the append-only run log.
#[derive(Clone, Debug, JsonSchema, Serialize)]
pub(crate) struct DurableRun {
    pub(crate) plan: RunPlan,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) work_plan_id: Option<String>,
    pub(crate) result: RunResult,
    pub(crate) cancel_requested: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RunEventCursor(u64);

pub(crate) struct RunLease {
    file: Option<File>,
    path: PathBuf,
}

impl Drop for RunLease {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            let _ = FileExt::unlock(&file);
            drop(file);
        }
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Deserialize)]
struct RunEventIdentity {
    run_id: String,
    event: String,
}

pub(crate) fn run_event_cursor(ctx: &RepoContext) -> Result<RunEventCursor> {
    ensure_state_layout(ctx)?;
    Ok(RunEventCursor(jsonl_end_offset(
        &ctx.state_file(RUNS_FILE),
    )?))
}

pub(crate) fn run_cancel_requested_since(
    ctx: &RepoContext,
    run_id: &str,
    cursor: &mut RunEventCursor,
    cancelled: &dyn Fn() -> bool,
) -> Result<bool> {
    let path = ctx.state_file(RUNS_FILE);
    let mut requested = false;
    let (offset, _) = scan_jsonl_raw_from(&path, cursor.0, cancelled, |raw| {
        let event = parse_run_event_identity(raw, &path)?;
        if event.run_id == run_id && event.event == EVENT_CANCEL_REQUESTED {
            requested = true;
        }
        Ok(())
    })?;
    cursor.0 = offset;
    Ok(requested)
}

pub(crate) fn start_run(
    ctx: &RepoContext,
    plan: RunPlan,
    work_plan_id: Option<String>,
) -> Result<(DurableRun, RunLease)> {
    ensure_state_layout(ctx)?;
    let run_id = new_id("run");
    let lease = acquire_run_lease(ctx, &run_id)?;
    let timestamp_ms = now_ms();
    append_event(
        ctx,
        RunEventRecord {
            id: new_id("run_event"),
            run_id: run_id.clone(),
            event: EVENT_QUEUED.into(),
            timestamp_ms,
            work_plan_id: work_plan_id.clone(),
            plan: Some(plan.clone()),
            target: None,
            result: None,
            conclusion: None,
        },
    )?;
    let run = DurableRun {
        result: RunResult::queued(
            run_id,
            plan.id.clone(),
            timestamp_ms,
            plan.targets
                .iter()
                .map(|target| {
                    TargetRunResult::queued(
                        target.target.clone(),
                        plan.config_digest.clone(),
                        target.input_digest.clone(),
                    )
                })
                .collect(),
        ),
        plan,
        work_plan_id,
        cancel_requested: false,
    };
    Ok((run, lease))
}

fn acquire_run_lease(ctx: &RepoContext, run_id: &str) -> Result<RunLease> {
    let (file, path) = open_run_lease(ctx, run_id)?;
    file.lock_exclusive()
        .with_context(|| format!("Failed to acquire worker lease for run '{run_id}'"))?;
    Ok(RunLease {
        file: Some(file),
        path,
    })
}

pub(crate) fn reconcile_run_for_inspection(ctx: &RepoContext, run_id: &str) -> Result<DurableRun> {
    let run = run_by_id(ctx, run_id)?;
    if run.result.status == RunStatus::Completed {
        return Ok(run);
    }

    let (file, path) = open_run_lease(ctx, run_id)?;
    match file.try_lock_exclusive() {
        Ok(true) => {
            let _lease = RunLease {
                file: Some(file),
                path,
            };
            let current = run_by_id(ctx, run_id)?;
            if current.result.status != RunStatus::Completed {
                block_nonterminal_run(
                    ctx,
                    run_id,
                    "repository run worker lease is no longer held; the worker process likely exited",
                )?;
            }
            run_by_id(ctx, run_id)
        }
        Ok(false) => Ok(run),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(run),
        Err(error) => {
            Err(error).with_context(|| format!("Failed to inspect worker lease for run '{run_id}'"))
        }
    }
}

fn open_run_lease(ctx: &RepoContext, run_id: &str) -> Result<(File, PathBuf)> {
    ensure_state_layout(ctx)?;
    let lease_dir = ctx.root().join(RUN_LEASE_DIR);
    fs::create_dir_all(&lease_dir)
        .with_context(|| format!("Failed to create {}", lease_dir.display()))?;
    let path = lease_dir.join(format!("{run_id}.lock"));
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("Failed to open run lease {}", path.display()))?;
    Ok((file, path))
}

pub(crate) fn mark_run_running(ctx: &RepoContext, run_id: &str) -> Result<()> {
    append_simple_event(ctx, run_id, EVENT_RUNNING, None, None)
}

pub(crate) fn mark_target_started(ctx: &RepoContext, run_id: &str, target: TargetId) -> Result<()> {
    append_simple_event(ctx, run_id, EVENT_TARGET_STARTED, Some(target), None)
}

pub(crate) fn record_target_result(
    ctx: &RepoContext,
    run_id: &str,
    result: TargetRunResult,
) -> Result<()> {
    append_event(
        ctx,
        RunEventRecord {
            id: new_id("run_event"),
            run_id: run_id.to_owned(),
            event: EVENT_TARGET_COMPLETED.into(),
            timestamp_ms: result.ended_at_ms.unwrap_or_else(now_ms),
            work_plan_id: None,
            plan: None,
            target: Some(result.target.clone()),
            result: Some(result),
            conclusion: None,
        },
    )
}

pub(crate) fn complete_run(
    ctx: &RepoContext,
    run_id: &str,
    conclusion: RunConclusion,
) -> Result<()> {
    append_simple_event(ctx, run_id, EVENT_COMPLETED, None, Some(conclusion))
}

pub(crate) fn block_nonterminal_run(ctx: &RepoContext, run_id: &str, message: &str) -> Result<()> {
    let run = run_by_id(ctx, run_id)?;
    if run.result.status == RunStatus::Completed {
        return Ok(());
    }

    for mut target in run
        .result
        .targets
        .into_iter()
        .filter(|target| target.status != RunStatus::Completed)
    {
        target.status = RunStatus::Completed;
        target.conclusion = Some(RunConclusion::Blocked);
        target.ended_at_ms = Some(super::support::now_ms());
        let mut finding = Finding::new(FindingSeverity::Error, message);
        finding.source = Some("jig".into());
        target.findings.push(finding);
        record_target_result(ctx, run_id, target)?;
    }
    complete_run(ctx, run_id, RunConclusion::Blocked)
}

pub(crate) fn request_run_cancel(ctx: &RepoContext, run_id: &str) -> Result<DurableRun> {
    let run = run_by_id(ctx, run_id)?;
    if run.result.status == RunStatus::Completed || run.cancel_requested {
        return Ok(run);
    }
    append_simple_event(ctx, run_id, EVENT_CANCEL_REQUESTED, None, None)?;
    run_by_id(ctx, run_id)
}

pub(crate) fn run_by_id(ctx: &RepoContext, run_id: &str) -> Result<DurableRun> {
    ensure_state_layout(ctx)?;
    let path = ctx.state_file(RUNS_FILE);
    let mut events = Vec::new();
    scan_jsonl_raw(&path, &|| false, |raw| {
        let identity = parse_run_event_identity(raw, &path)?;
        if identity.run_id == run_id {
            events.push(parse_run_event(raw, &path)?);
        }
        Ok(())
    })?;
    fold_events(run_id, events)
}

fn parse_run_event_identity(
    raw: RawJsonlRecord<'_>,
    path: &std::path::Path,
) -> Result<RunEventIdentity> {
    serde_json::from_slice(raw.bytes).with_context(|| {
        format!(
            "Failed to parse run event identity {} in {}",
            raw.line_number,
            path.display()
        )
    })
}

fn append_simple_event(
    ctx: &RepoContext,
    run_id: &str,
    event: &str,
    target: Option<TargetId>,
    conclusion: Option<RunConclusion>,
) -> Result<()> {
    append_event(
        ctx,
        RunEventRecord {
            id: new_id("run_event"),
            run_id: run_id.to_owned(),
            event: event.into(),
            timestamp_ms: now_ms(),
            work_plan_id: None,
            plan: None,
            target,
            result: None,
            conclusion,
        },
    )
}

fn append_event(ctx: &RepoContext, event: RunEventRecord) -> Result<()> {
    append_jsonl(&ctx.state_file(RUNS_FILE), &event)
}

fn parse_run_event(raw: RawJsonlRecord<'_>, path: &std::path::Path) -> Result<RunEventRecord> {
    #[cfg(test)]
    FULL_RUN_EVENT_PARSE_COUNT.with(|counter| counter.set(counter.get() + 1));
    serde_json::from_slice(raw.bytes).with_context(|| {
        format!(
            "Failed to parse run record {} in {}",
            raw.line_number,
            path.display()
        )
    })
}

fn fold_events(run_id: &str, events: Vec<RunEventRecord>) -> Result<DurableRun> {
    let mut run: Option<DurableRun> = None;
    for event in events {
        match event.event.as_str() {
            EVENT_QUEUED => {
                if run.is_some() {
                    bail!("run '{run_id}' has more than one queued event");
                }
                let plan = event
                    .plan
                    .ok_or_else(|| anyhow::anyhow!("run '{run_id}' queued event has no plan"))?;
                let targets = plan
                    .targets
                    .iter()
                    .map(|target| {
                        TargetRunResult::queued(
                            target.target.clone(),
                            plan.config_digest.clone(),
                            target.input_digest.clone(),
                        )
                    })
                    .collect();
                run = Some(DurableRun {
                    result: RunResult::queued(run_id, plan.id.clone(), event.timestamp_ms, targets),
                    plan,
                    work_plan_id: event.work_plan_id,
                    cancel_requested: false,
                });
            }
            EVENT_RUNNING => {
                let current = require_run(&mut run, run_id, EVENT_RUNNING)?;
                ensure_nonterminal(current, run_id, EVENT_RUNNING)?;
                current.result.status = RunStatus::Running;
                current.result.updated_at_ms = event.timestamp_ms;
            }
            EVENT_TARGET_STARTED => {
                let current = require_run(&mut run, run_id, EVENT_TARGET_STARTED)?;
                ensure_nonterminal(current, run_id, EVENT_TARGET_STARTED)?;
                let target = event.target.ok_or_else(|| {
                    anyhow::anyhow!("run '{run_id}' target_started event has no target")
                })?;
                let result = target_result_mut(current, run_id, &target)?;
                if result.status == RunStatus::Completed {
                    bail!("run '{run_id}' target '{target}' started after completion");
                }
                result.status = RunStatus::Running;
                result.started_at_ms = Some(event.timestamp_ms);
                current.result.status = RunStatus::Running;
                current.result.updated_at_ms = event.timestamp_ms;
            }
            EVENT_TARGET_COMPLETED => {
                let current = require_run(&mut run, run_id, EVENT_TARGET_COMPLETED)?;
                ensure_nonterminal(current, run_id, EVENT_TARGET_COMPLETED)?;
                let result = event.result.ok_or_else(|| {
                    anyhow::anyhow!("run '{run_id}' target_completed event has no result")
                })?;
                if event.target.as_ref() != Some(&result.target) {
                    bail!("run '{run_id}' target_completed identity does not match its result");
                }
                if result.status != RunStatus::Completed || result.conclusion.is_none() {
                    bail!(
                        "run '{run_id}' target '{}' has a nonterminal result",
                        result.target
                    );
                }
                let stored = target_result_mut(current, run_id, &result.target)?;
                if stored.status == RunStatus::Completed {
                    bail!(
                        "run '{run_id}' target '{}' completed more than once",
                        result.target
                    );
                }
                *stored = result;
                current.result.status = RunStatus::Running;
                current.result.updated_at_ms = event.timestamp_ms;
            }
            EVENT_COMPLETED => {
                let current = require_run(&mut run, run_id, EVENT_COMPLETED)?;
                ensure_nonterminal(current, run_id, EVENT_COMPLETED)?;
                if current
                    .result
                    .targets
                    .iter()
                    .any(|target| target.status != RunStatus::Completed)
                {
                    bail!("run '{run_id}' completed before every target reached a conclusion");
                }
                current.result.status = RunStatus::Completed;
                current.result.conclusion = Some(event.conclusion.ok_or_else(|| {
                    anyhow::anyhow!("run '{run_id}' completed event has no conclusion")
                })?);
                current.result.updated_at_ms = event.timestamp_ms;
            }
            EVENT_CANCEL_REQUESTED => {
                let current = require_run(&mut run, run_id, EVENT_CANCEL_REQUESTED)?;
                // Cancellation and terminal completion may race across MCP
                // request and worker threads. A physically later cancellation
                // event is an idempotent observation, not stream corruption.
                if current.result.status == RunStatus::Completed {
                    continue;
                }
                current.cancel_requested = true;
                current.result.updated_at_ms = event.timestamp_ms;
            }
            _ => {}
        }
    }
    run.ok_or_else(|| anyhow::anyhow!("run '{run_id}' was not found"))
}

fn require_run<'a>(
    run: &'a mut Option<DurableRun>,
    run_id: &str,
    event: &str,
) -> Result<&'a mut DurableRun> {
    run.as_mut()
        .ok_or_else(|| anyhow::anyhow!("run '{run_id}' has a {event} event before queued"))
}

fn ensure_nonterminal(run: &DurableRun, run_id: &str, event: &str) -> Result<()> {
    if run.result.status == RunStatus::Completed {
        bail!("run '{run_id}' has a {event} event after completion");
    }
    Ok(())
}

fn target_result_mut<'a>(
    run: &'a mut DurableRun,
    run_id: &str,
    target: &TargetId,
) -> Result<&'a mut TargetRunResult> {
    run.result
        .targets
        .iter_mut()
        .find(|result| &result.target == target)
        .ok_or_else(|| anyhow::anyhow!("run '{run_id}' references unplanned target '{target}'"))
}

#[cfg(test)]
mod tests {
    use jig_contract::{
        ActionIntent, ActionRunner, PlannedTarget, RunConclusion, RunPlan, RunStatus,
        SourceIdentity,
    };
    use tempfile::tempdir;

    use super::*;
    use crate::test_env::TestRepoBuilder;

    fn context() -> (tempfile::TempDir, RepoContext) {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(["rust_test_command"])
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        (temp, ctx)
    }

    fn plan() -> RunPlan {
        let target: TargetId = "repo:test".parse().unwrap();
        RunPlan::new(
            "run-plan_1",
            "sha256:config",
            SourceIdentity::new(Some("abc".into()), "sha256:worktree"),
            vec![PlannedTarget::new(
                target.clone(),
                ActionIntent::Check,
                ActionRunner::command("test"),
                "sha256:input",
            )],
            vec![vec![target]],
        )
    }

    #[test]
    fn lifecycle_round_trips_from_append_only_events() {
        let (_temp, ctx) = context();
        let (started, _lease) = start_run(&ctx, plan(), Some("plan_work".into())).unwrap();
        let run_id = started.result.run_id;
        let target: TargetId = "repo:test".parse().unwrap();

        mark_run_running(&ctx, &run_id).unwrap();
        mark_target_started(&ctx, &run_id, target.clone()).unwrap();
        let mut result = TargetRunResult::queued(target, "sha256:config", "sha256:input");
        result.status = RunStatus::Completed;
        result.conclusion = Some(RunConclusion::Success);
        result.started_at_ms = Some(2);
        result.ended_at_ms = Some(3);
        result.exit_code = Some(0);
        record_target_result(&ctx, &run_id, result).unwrap();
        complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();

        let reloaded = run_by_id(&ctx, &run_id).unwrap();
        assert_eq!(reloaded.work_plan_id.as_deref(), Some("plan_work"));
        assert_eq!(reloaded.result.status, RunStatus::Completed);
        assert_eq!(reloaded.result.conclusion, Some(RunConclusion::Success));
        assert_eq!(
            reloaded.result.targets[0].conclusion,
            Some(RunConclusion::Success)
        );
    }

    #[test]
    fn run_lookup_only_deserializes_full_events_for_the_requested_run() {
        let (_temp, ctx) = context();
        let (_first, _first_lease) = start_run(&ctx, plan(), None).unwrap();
        let (second, _second_lease) = start_run(&ctx, plan(), None).unwrap();
        FULL_RUN_EVENT_PARSE_COUNT.with(|counter| counter.set(0));

        let loaded = run_by_id(&ctx, &second.result.run_id).unwrap();

        assert_eq!(loaded.result.run_id, second.result.run_id);
        FULL_RUN_EVENT_PARSE_COUNT.with(|counter| assert_eq!(counter.get(), 1));
    }

    #[test]
    fn cancellation_requests_are_idempotent() {
        let (_temp, ctx) = context();
        let (started, _lease) = start_run(&ctx, plan(), None).unwrap();
        let run_id = started.result.run_id;

        let first = request_run_cancel(&ctx, &run_id).unwrap();
        let second = request_run_cancel(&ctx, &run_id).unwrap();

        assert!(first.cancel_requested);
        assert!(second.cancel_requested);
        let events = std::fs::read_to_string(ctx.state_file(RUNS_FILE)).unwrap();
        assert_eq!(events.matches(EVENT_CANCEL_REQUESTED).count(), 1);
    }

    #[test]
    fn queued_runs_are_published_with_a_lease_and_terminal_leases_are_removed() {
        let (_temp, ctx) = context();
        let (started, lease) = start_run(&ctx, plan(), None).unwrap();
        let run_id = started.result.run_id;
        let lease_path = ctx
            .root()
            .join(RUN_LEASE_DIR)
            .join(format!("{run_id}.lock"));

        assert!(lease_path.exists());
        let inspected = reconcile_run_for_inspection(&ctx, &run_id).unwrap();
        assert_eq!(inspected.result.status, RunStatus::Queued);

        drop(lease);
        assert!(!lease_path.exists());
        let recovered = reconcile_run_for_inspection(&ctx, &run_id).unwrap();
        assert_eq!(recovered.result.status, RunStatus::Completed);
        assert_eq!(recovered.result.conclusion, Some(RunConclusion::Blocked));
        assert!(!lease_path.exists());
    }

    #[test]
    fn cancellation_observed_after_completion_does_not_corrupt_the_run() {
        let (_temp, ctx) = context();
        let (started, _lease) = start_run(&ctx, plan(), None).unwrap();
        let run_id = started.result.run_id;
        let target: TargetId = "repo:test".parse().unwrap();
        let mut result = TargetRunResult::queued(target, "sha256:config", "sha256:input");
        result.status = RunStatus::Completed;
        result.conclusion = Some(RunConclusion::Success);
        record_target_result(&ctx, &run_id, result).unwrap();
        complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();

        append_simple_event(&ctx, &run_id, EVENT_CANCEL_REQUESTED, None, None).unwrap();
        let reloaded = run_by_id(&ctx, &run_id).unwrap();

        assert_eq!(reloaded.result.status, RunStatus::Completed);
        assert_eq!(reloaded.result.conclusion, Some(RunConclusion::Success));
        assert!(!reloaded.cancel_requested);
    }

    #[test]
    fn unknown_future_events_are_ignored() {
        let (_temp, ctx) = context();
        let (started, _lease) = start_run(&ctx, plan(), None).unwrap();
        append_event(
            &ctx,
            RunEventRecord {
                id: "run_event_future".into(),
                run_id: started.result.run_id.clone(),
                event: "future_annotation".into(),
                timestamp_ms: now_ms(),
                work_plan_id: None,
                plan: None,
                target: None,
                result: None,
                conclusion: None,
            },
        )
        .unwrap();

        assert_eq!(
            run_by_id(&ctx, &started.result.run_id)
                .unwrap()
                .result
                .status,
            RunStatus::Queued
        );
    }

    #[test]
    fn corrupt_known_lifecycle_is_rejected() {
        let (_temp, ctx) = context();
        let (started, _lease) = start_run(&ctx, plan(), None).unwrap();
        complete_run(&ctx, &started.result.run_id, RunConclusion::Success).unwrap();

        let error = run_by_id(&ctx, &started.result.run_id).unwrap_err();
        assert!(error.to_string().contains("before every target"));
    }
}
