use anyhow::{Result, bail};
use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use croner::Cron;
use serde_json::{Value, json};

use crate::command::{LoopDispatchRequest, LoopRunRequest, LoopTickRequest};
use crate::context::{RepoContext, parse_five_field_cron};
use crate::state::{ReceiptInput, now_ms, record_receipt};
use crate::tool_defs::LOOP_DISPATCH_TOOL;

use super::engine::{ScheduledTick, tick, tick_scheduled};
use super::occurrence::{
    OccurrenceClaim, OccurrenceFinish, OccurrenceGuard, OccurrenceOutcome, OccurrenceStatus,
    OccurrenceStore, ScheduleOccurrence,
};
use super::workflow::{
    ResolvedWorkflow, TuningOverrides, WorkflowRunPolicy, list_workflows, resolve_workflow,
};

mod policy;

use policy::{
    DispatchStep, DispatchSummary, RunSummary, RunTickDisposition, TerminalDetails, begin_execution,
};

#[derive(Clone, Debug)]
pub(super) struct ScheduleSpec {
    expression: String,
    timezone_name: String,
    cron: Cron,
    timezone: Tz,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ScheduleWindow {
    pub(super) due_at_ms: Option<u64>,
    pub(super) next_at_ms: u64,
}

impl ScheduleSpec {
    pub(super) fn parse(expression: &str, timezone_name: Option<&str>) -> Result<Self> {
        let cron = parse_five_field_cron(expression)?;
        let timezone_name = timezone_name.unwrap_or("UTC");
        let timezone = timezone_name
            .parse::<Tz>()
            .map_err(|_| anyhow::anyhow!("Invalid IANA timezone '{timezone_name}'"))?;
        Ok(Self {
            expression: expression.to_string(),
            timezone_name: timezone_name.to_string(),
            cron,
            timezone,
        })
    }

    pub(super) fn expression(&self) -> &str {
        &self.expression
    }

    pub(super) fn timezone_name(&self) -> &str {
        &self.timezone_name
    }

    pub(super) fn window(
        &self,
        now_ms: u64,
        last_scheduled_at_ms: Option<u64>,
    ) -> Result<ScheduleWindow> {
        let now = datetime_from_ms(now_ms)?.with_timezone(&self.timezone);
        let most_recent = previous_matching(&self.cron, &now)?;
        let next = next_matching(&self.cron, &now)?;
        let most_recent_ms = timestamp_ms(most_recent)?;
        let due_at_ms = (last_scheduled_at_ms.is_none_or(|last| most_recent_ms > last))
            .then_some(most_recent_ms);
        Ok(ScheduleWindow {
            due_at_ms,
            next_at_ms: timestamp_ms(next)?,
        })
    }
}

fn previous_matching<T: TimeZone>(cron: &Cron, now: &DateTime<T>) -> Result<DateTime<T>>
where
    T::Offset: Copy,
{
    let mut candidate = cron
        .find_previous_occurrence(now, true)
        .map_err(|error| anyhow::anyhow!("Failed to find due cron occurrence: {error}"))?;
    for _ in 0..8 {
        if cron
            .is_time_matching(&candidate)
            .map_err(|error| anyhow::anyhow!("Failed to validate due cron occurrence: {error}"))?
        {
            return Ok(candidate);
        }
        candidate = cron
            .find_previous_occurrence(&candidate, false)
            .map_err(|error| anyhow::anyhow!("Failed to find due cron occurrence: {error}"))?;
    }
    bail!("Cron evaluator did not produce a valid previous occurrence")
}

fn next_matching<T: TimeZone>(cron: &Cron, now: &DateTime<T>) -> Result<DateTime<T>>
where
    T::Offset: Copy,
{
    let mut candidate = cron
        .find_next_occurrence(now, false)
        .map_err(|error| anyhow::anyhow!("Failed to find next cron occurrence: {error}"))?;
    for _ in 0..8 {
        if cron
            .is_time_matching(&candidate)
            .map_err(|error| anyhow::anyhow!("Failed to validate next cron occurrence: {error}"))?
        {
            return Ok(candidate);
        }
        candidate = cron
            .find_next_occurrence(&candidate, false)
            .map_err(|error| anyhow::anyhow!("Failed to find next cron occurrence: {error}"))?;
    }
    bail!("Cron evaluator did not produce a valid next occurrence")
}

fn datetime_from_ms(timestamp_ms: u64) -> Result<DateTime<Utc>> {
    let timestamp_ms = i64::try_from(timestamp_ms)
        .map_err(|_| anyhow::anyhow!("Schedule timestamp exceeds supported range"))?;
    DateTime::from_timestamp_millis(timestamp_ms)
        .ok_or_else(|| anyhow::anyhow!("Schedule timestamp is outside Chrono's supported range"))
}

fn timestamp_ms<T: TimeZone>(timestamp: DateTime<T>) -> Result<u64> {
    u64::try_from(timestamp.timestamp_millis())
        .map_err(|_| anyhow::anyhow!("Cron occurrence predates the Unix epoch"))
}

pub(super) fn dispatch_due(ctx: &RepoContext, _: LoopDispatchRequest) -> Result<Value> {
    dispatch_due_at(ctx, now_ms())
}

pub(super) fn dispatch_due_at(ctx: &RepoContext, dispatch_at_ms: u64) -> Result<Value> {
    let started = now_ms();
    let workflows = list_workflows(ctx)?;
    let mut occurrences = OccurrenceStore::new(ctx);
    let reconciled = occurrences.reconcile_stale()?;
    let known_occurrences = occurrences.snapshot()?;
    let mut actions = Vec::new();
    let mut summary = DispatchSummary::default();

    for workflow in workflows {
        let step = dispatch_workflow(
            ctx,
            &mut occurrences,
            &known_occurrences,
            &workflow,
            dispatch_at_ms,
        );
        summary.include(&step);
        if let Some(action) = step.action {
            actions.push(action);
        }
    }

    summary.needs_attention_count = u64::try_from(
        occurrences
            .snapshot()?
            .iter()
            .filter(|occurrence| occurrence.status == OccurrenceStatus::NeedsAttention)
            .count(),
    )
    .unwrap_or(u64::MAX);
    let status = summary.status();
    let ok = !summary.requires_attention();
    let ended = now_ms();
    let evidence = json!({
        "kind": "loop_dispatch",
        "schema_version": 1,
        "dispatch_at_ms": dispatch_at_ms,
        "status": status,
        "due_count": summary.due_count,
        "executed_count": summary.executed_count,
        "skipped_count": summary.skipped_count,
        "failed_count": summary.failed_count,
        "needs_attention_count": summary.needs_attention_count,
        "reconciled_occurrences": reconciled,
        "actions": actions,
    });
    let receipt_id = record_receipt(
        ctx,
        ReceiptInput {
            tool_name: LOOP_DISPATCH_TOOL,
            args: json!({}),
            invoked_command_key: None,
            plan_id: None,
            started_at_ms: started,
            ended_at_ms: ended,
            exit_status: i32::from(!ok),
            stdout: "",
            stderr: "",
            evidence: Some(evidence.clone()),
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: true,
            worktree_fingerprint_override: None,
        },
    )?;
    Ok(json!({
        "ok": ok,
        "command": "loop dispatch",
        "receipt_id": receipt_id,
        "status": status,
        "dispatch_at_ms": dispatch_at_ms,
        "due_count": summary.due_count,
        "executed_count": summary.executed_count,
        "skipped_count": summary.skipped_count,
        "failed_count": summary.failed_count,
        "needs_attention_count": summary.needs_attention_count,
        "reconciled_occurrences": evidence["reconciled_occurrences"],
        "actions": evidence["actions"],
    }))
}

fn dispatch_workflow(
    ctx: &RepoContext,
    occurrences: &mut OccurrenceStore,
    known_occurrences: &[ScheduleOccurrence],
    workflow: &ResolvedWorkflow,
    dispatch_at_ms: u64,
) -> DispatchStep {
    let Some(schedule) = workflow.schedule.as_ref() else {
        return DispatchStep::default();
    };
    if !workflow.enabled {
        return DispatchStep::action(json!({
            "workflow_id": workflow.id,
            "status": "disabled",
        }));
    }
    let latest = OccurrenceStore::latest_for_workflow(known_occurrences, &workflow.id);
    let window = match schedule.window(
        dispatch_at_ms,
        latest.as_ref().map(|record| record.scheduled_at_ms),
    ) {
        Ok(window) => window,
        Err(error) => return DispatchStep::failure(&workflow.id, format!("{error:#}")),
    };
    let Some(due_at_ms) = window.due_at_ms else {
        return DispatchStep::action(json!({
            "workflow_id": workflow.id,
            "status": "not_due",
            "next_at_ms": window.next_at_ms,
        }));
    };
    let mut step = DispatchStep {
        due_count: 1,
        ..DispatchStep::default()
    };
    let claim = match occurrences.claim(&workflow.id, due_at_ms, workflow.lease_ttl_seconds) {
        Ok(claim) => claim,
        Err(error) => {
            step.failed_count = 1;
            step.action = DispatchStep::failure(&workflow.id, format!("{error:#}")).action;
            return step;
        }
    };
    let claim = match claim {
        OccurrenceClaim::AlreadyRecorded(record) => {
            step.skipped_count = 1;
            step.action = Some(json!({
                "workflow_id": workflow.id,
                "occurrence": record,
                "status": "already_recorded",
                "next_at_ms": window.next_at_ms,
            }));
            return step;
        }
        OccurrenceClaim::Acquired(claim) => claim,
    };

    let guard = match begin_execution(&mut step, || {
        OccurrenceGuard::start(occurrences.clone(), &claim, workflow.lease_ttl_seconds)
    }) {
        Ok(guard) => guard,
        Err(error) => {
            let error = format!("Failed to renew scheduled occurrence: {error:#}");
            step.failed_count = 1;
            step.action = Some(
                match occurrences.finish(
                    &claim.occurrence_id,
                    &claim.owner,
                    OccurrenceFinish {
                        outcome: OccurrenceOutcome::Failed,
                        worker_receipt_id: None,
                        worktree: None,
                        error: Some(&error),
                    },
                ) {
                    Ok(record) => dispatch_action(&record, "failed", window.next_at_ms, None),
                    Err(finish_error) => dispatch_state_failure(
                        workflow,
                        &claim,
                        window.next_at_ms,
                        None,
                        format!("{error}; recording the failure also failed: {finish_error:#}"),
                    ),
                },
            );
            return step;
        }
    };
    let cancelled = || guard.renewal_failed();
    let tick = tick_scheduled(ctx, &workflow.id, &claim.occurrence_id, &cancelled);
    if tick.as_ref().is_ok_and(|tick| {
        tick.value()
            .is_some_and(|value| value["lease_acquired"].as_bool() == Some(false))
    }) {
        return match guard.abandon() {
            Ok(abandoned) => {
                step.executed_count = 0;
                step.skipped_count = 1;
                step.action = Some(json!({
                    "workflow_id": workflow.id,
                    "occurrence": abandoned,
                    "status": "deferred",
                    "reason": "workflow_lease_held",
                    "next_at_ms": window.next_at_ms,
                    "tick": tick_value(&tick),
                }));
                step
            }
            Err(error) => {
                step.failed_count = 1;
                step.action = Some(dispatch_state_failure(
                    workflow,
                    &claim,
                    window.next_at_ms,
                    tick_value(&tick),
                    format!("Failed to abandon deferred occurrence: {error:#}"),
                ));
                step
            }
        };
    }
    let details = TerminalDetails::from_tick(&tick);
    match guard.finish(OccurrenceFinish {
        outcome: details.outcome,
        worker_receipt_id: details.worker_receipt_id.as_deref(),
        worktree: details.worktree.as_deref(),
        error: details.error.as_deref(),
    }) {
        Ok(record) => {
            step.failed_count = u64::from(details.outcome == OccurrenceOutcome::Failed);
            step.action = Some(dispatch_action(
                &record,
                details.outcome.status().as_str(),
                window.next_at_ms,
                tick_value(&tick),
            ));
        }
        Err(finish_error) => {
            step.failed_count = 1;
            step.action = Some(dispatch_state_failure(
                workflow,
                &claim,
                window.next_at_ms,
                tick_value(&tick),
                format!("Failed to finish scheduled occurrence: {finish_error:#}"),
            ));
        }
    }
    step
}

fn dispatch_state_failure(
    workflow: &ResolvedWorkflow,
    occurrence: &ScheduleOccurrence,
    next_at_ms: u64,
    tick: Option<Value>,
    error: impl std::fmt::Display,
) -> Value {
    json!({
        "workflow_id": workflow.id,
        "occurrence": occurrence,
        "status": "failed",
        "next_at_ms": next_at_ms,
        "tick": tick,
        "error": error.to_string(),
    })
}

fn tick_value(tick: &Result<ScheduledTick>) -> Option<Value> {
    tick.as_ref().ok().and_then(ScheduledTick::value).cloned()
}

fn dispatch_action(
    occurrence: &ScheduleOccurrence,
    status: &str,
    next_at_ms: u64,
    tick: Option<Value>,
) -> Value {
    json!({
        "workflow_id": occurrence.workflow_id,
        "occurrence": occurrence,
        "status": status,
        "next_at_ms": next_at_ms,
        "tick": tick,
    })
}

pub(super) fn run_until(ctx: &RepoContext, request: LoopRunRequest) -> Result<Value> {
    if request.until != "idle" {
        bail!(
            "Unsupported loop run stop condition '{}'. Use --until idle.",
            request.until
        );
    }
    if request.max_ticks == 0 {
        bail!("--max-ticks must be greater than zero");
    }
    let workflow = resolve_workflow(
        ctx,
        request.workflow.as_deref(),
        TuningOverrides {
            lease_ttl_seconds: request.lease_ttl_seconds,
            max_attempts: request.max_attempts,
            backoff_seconds: request.backoff_seconds,
        },
    )?;
    if workflow.run_policy() == WorkflowRunPolicy::SingleTick {
        bail!(
            "Loop workflow '{}' runs one task per tick and does not support `loop run`; use `loop tick --workflow {}` for a manual run or `loop dispatch` for scheduled execution",
            workflow.id,
            workflow.id,
        );
    }

    let mut ticks = Vec::new();
    let mut summary = RunSummary::default();
    for _ in 0..request.max_ticks {
        let tick = tick(
            ctx,
            LoopTickRequest {
                workflow: request.workflow.clone(),
                lease_ttl_seconds: request.lease_ttl_seconds,
                max_attempts: request.max_attempts,
                backoff_seconds: request.backoff_seconds,
            },
        )?;
        let disposition = RunTickDisposition::from_tick(&tick);
        ticks.push(tick);
        if summary.observe(disposition) {
            break;
        }
    }
    let status = summary.status();

    Ok(json!({
        "ok": status != "failed",
        "command": "loop run",
        "until": request.until,
        "status": status,
        "tick_count": ticks.len(),
        "ticks": ticks,
    }))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::DateTime;
    use serde_json::json;
    use tempfile::tempdir;

    use super::super::state::{LOOP_CACHE_DIR, LeaseAcquire, LeaseStore};
    use super::super::workflow::WorkflowCompletion;
    use super::{
        DispatchStep, DispatchSummary, RunSummary, RunTickDisposition, ScheduleSpec,
        TerminalDetails, begin_execution, dispatch_due_at,
    };
    use crate::command::LoopStatusRequest;
    use crate::context::RepoContext;
    use crate::runtime::loops::engine::ScheduledTick;
    use crate::test_env::TestRepoBuilder;

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
            needs_attention_count: 1,
            ..DispatchSummary::default()
        };

        assert_eq!(summary.status(), "needs_attention");
        assert!(summary.requires_attention());

        summary.failed_count = 1;
        assert_eq!(summary.status(), "failed");
        assert!(summary.requires_attention());
    }

    #[test]
    fn execution_count_starts_only_after_guard_start_succeeds() {
        let mut step = DispatchStep::default();

        assert!(
            begin_execution::<()>(&mut step, || anyhow::bail!("injected start failure")).is_err()
        );
        assert_eq!(step.executed_count, 0);

        begin_execution(&mut step, || Ok(())).unwrap();
        assert_eq!(step.executed_count, 1);
    }

    #[test]
    fn scheduled_tick_preserves_needs_attention_as_an_occurrence_outcome() {
        let tick = Ok(ScheduledTick::Reported {
            value: json!({"status": "needs_attention"}),
            completion: WorkflowCompletion::default(),
        });

        let details = TerminalDetails::from_tick(&tick);

        assert_eq!(
            details.outcome,
            super::super::occurrence::OccurrenceOutcome::NeedsAttention
        );
    }

    #[test]
    fn scheduled_tick_error_keeps_worker_completion_evidence() {
        let tick = Ok(ScheduledTick::Errored {
            value: Some(json!({"status": "failed"})),
            completion: WorkflowCompletion {
                worker_receipt_id: Some("receipt-worker".into()),
                worktree: Some("/tmp/retained-worktree".into()),
                error: Some("worker failed".into()),
            },
            error: "tick receipt failed".into(),
        });

        let details = TerminalDetails::from_tick(&tick);

        assert_eq!(
            details.outcome,
            super::super::occurrence::OccurrenceOutcome::Failed
        );
        assert_eq!(details.worker_receipt_id.as_deref(), Some("receipt-worker"));
        assert_eq!(details.worktree.as_deref(), Some("/tmp/retained-worktree"));
        assert_eq!(
            details.error.as_deref(),
            Some("tick receipt failed; workflow completion: worker failed")
        );
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
        serde_json::from_value::<jig_ui::LoopsView>(status).unwrap();
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
        assert_eq!(deferred["status"], "idle", "{deferred:#}");
        assert_eq!(deferred["actions"][0]["status"], "deferred");
        assert_eq!(deferred["executed_count"], 0);
        assert_eq!(deferred["skipped_count"], 1);
        leases
            .release("workflow:scheduled-noop", &lease.owner)
            .unwrap();

        let retried = dispatch_due_at(&ctx, dispatch_at).unwrap();
        assert_eq!(retried["status"], "acted", "{retried:#}");
        assert_eq!(retried["executed_count"], 1);
    }

    fn timestamp(value: &str) -> u64 {
        u64::try_from(
            DateTime::parse_from_rfc3339(value)
                .unwrap()
                .timestamp_millis(),
        )
        .unwrap()
    }
}
