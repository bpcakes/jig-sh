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
    OccurrenceClaim, OccurrenceFinish, OccurrenceGuard, OccurrenceStore, ScheduleOccurrence,
};
use super::workflow::{ResolvedWorkflow, list_workflows};

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

#[derive(Default)]
struct DispatchStep {
    action: Option<Value>,
    due_count: u64,
    executed_count: u64,
    skipped_count: u64,
    failed_count: u64,
}

impl DispatchStep {
    fn action(action: Value) -> Self {
        Self {
            action: Some(action),
            ..Self::default()
        }
    }

    fn failure(workflow_id: &str, error: impl std::fmt::Display) -> Self {
        Self {
            action: Some(json!({
                "workflow_id": workflow_id,
                "status": "failed",
                "error": error.to_string(),
            })),
            failed_count: 1,
            ..Self::default()
        }
    }
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
    let mut due_count = 0_u64;
    let mut executed_count = 0_u64;
    let mut skipped_count = 0_u64;
    let mut failed_count = 0_u64;

    for workflow in workflows {
        let step = dispatch_workflow(
            ctx,
            &mut occurrences,
            &known_occurrences,
            &workflow,
            dispatch_at_ms,
        );
        due_count += step.due_count;
        executed_count += step.executed_count;
        skipped_count += step.skipped_count;
        failed_count += step.failed_count;
        if let Some(action) = step.action {
            actions.push(action);
        }
    }

    let needs_attention_count = u64::try_from(reconciled.len()).unwrap_or(u64::MAX);
    let status = if failed_count > 0 {
        "failed"
    } else if needs_attention_count > 0 {
        "needs_attention"
    } else if executed_count > 0 {
        "acted"
    } else {
        "idle"
    };
    let ended = now_ms();
    let evidence = json!({
        "kind": "loop_dispatch",
        "schema_version": 1,
        "dispatch_at_ms": dispatch_at_ms,
        "status": status,
        "due_count": due_count,
        "executed_count": executed_count,
        "skipped_count": skipped_count,
        "failed_count": failed_count,
        "needs_attention_count": needs_attention_count,
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
            exit_status: i32::from(failed_count > 0),
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
        "ok": failed_count == 0,
        "command": "loop dispatch",
        "receipt_id": receipt_id,
        "status": status,
        "dispatch_at_ms": dispatch_at_ms,
        "due_count": due_count,
        "executed_count": executed_count,
        "skipped_count": skipped_count,
        "failed_count": failed_count,
        "needs_attention_count": needs_attention_count,
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

    step.executed_count = 1;
    let guard =
        match OccurrenceGuard::start(occurrences.clone(), &claim, workflow.lease_ttl_seconds) {
            Ok(guard) => guard,
            Err(error) => {
                let error = format!("Failed to renew scheduled occurrence: {error:#}");
                step.failed_count = 1;
                step.action = Some(
                    match occurrences.finish(
                        &claim.occurrence_id,
                        &claim.owner,
                        OccurrenceFinish {
                            status: "failed",
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
    if tick
        .as_ref()
        .is_ok_and(|tick| tick.value["lease_acquired"].as_bool() == Some(false))
    {
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
                    "tick": tick.ok().map(|tick| tick.value),
                }));
                step
            }
            Err(error) => {
                step.failed_count = 1;
                step.action = Some(dispatch_state_failure(
                    workflow,
                    &claim,
                    window.next_at_ms,
                    tick.ok().map(|tick| tick.value),
                    format!("Failed to abandon deferred occurrence: {error:#}"),
                ));
                step
            }
        };
    }
    let (status, worker_receipt_id, worktree, error) = terminal_details(&tick);
    match guard.finish(OccurrenceFinish {
        status,
        worker_receipt_id: worker_receipt_id.as_deref(),
        worktree: worktree.as_deref(),
        error: error.as_deref(),
    }) {
        Ok(record) => {
            step.failed_count = u64::from(status == "failed");
            step.action = Some(dispatch_action(
                &record,
                status,
                window.next_at_ms,
                tick.ok().map(|tick| tick.value),
            ));
        }
        Err(finish_error) => {
            step.failed_count = 1;
            step.action = Some(dispatch_state_failure(
                workflow,
                &claim,
                window.next_at_ms,
                tick.ok().map(|tick| tick.value),
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

fn terminal_details(
    tick: &Result<ScheduledTick>,
) -> (&'static str, Option<String>, Option<String>, Option<String>) {
    match tick {
        Ok(tick) => {
            let failed = tick.value["status"].as_str() == Some("failed");
            (
                if failed { "failed" } else { "succeeded" },
                tick.completion.worker_receipt_id.clone(),
                tick.completion.worktree.clone(),
                tick.completion.error.clone(),
            )
        }
        Err(error) => ("failed", None, None, Some(format!("{error:#}"))),
    }
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

    let mut ticks = Vec::new();
    let mut status = "max_ticks_reached".to_string();
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
        let tick_status = tick["status"].as_str().unwrap_or("unknown").to_string();
        let idle = tick["idle"].as_bool().unwrap_or(false);
        ticks.push(tick);
        if matches!(
            tick_status.as_str(),
            "waiting" | "disabled" | "failed" | "needs_attention"
        ) {
            status = tick_status;
            break;
        }
        if idle {
            status = "idle".into();
            break;
        }
    }

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
    use tempfile::tempdir;

    use super::super::state::{LeaseAcquire, LeaseStore};
    use super::{ScheduleSpec, dispatch_due_at};
    use crate::command::LoopStatusRequest;
    use crate::context::RepoContext;
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
