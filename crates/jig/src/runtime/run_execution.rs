// agentic-loc-exception: target execution, cancellation, and durable result transitions share one lifecycle boundary.

use std::collections::BTreeMap;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::Result;
use jig_contract::{
    ActionEffect, ActionRunner, Finding, FindingSeverity, PlannedTarget, ResultParser,
    RunConclusion, RunPlan, RunStatus, TargetId, TargetRunResult,
};
use jig_owned_process::OwnedProcessTreeError;
use serde::Serialize;
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::execution::{
    ExecutionCancellation, ExecutionControl, ExecutionEvent, ExecutionObserver, ExecutionPhase,
    PhasePosition, SupervisedExecutionError, run_supervised_execution_command,
};
use crate::repository::{RepositoryCatalog, target_input_digest};
use crate::repository_path::{resolve_repository_working_directory, validate_runner_environment};
#[cfg(test)]
use crate::state::start_run;
use crate::state::{
    ReceiptInput, TargetReceiptMetadata, complete_run, mark_run_running, mark_target_started,
    now_ms, record_target_receipt, record_target_result, run_by_id,
};

use super::tool_execution::run_native_tool_with_control;

const GENERIC_TARGET_TOOL: &str = "jig.target_run";

#[derive(Debug, Serialize)]
pub(super) struct CheckRunExecution {
    pub(super) run: crate::state::DurableRun,
    pub(super) results: Vec<Value>,
    pub(super) failed_targets: Vec<TargetId>,
    pub(super) source_observations: SourceObservationMetrics,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub(super) struct SourceObservationMetrics {
    count: usize,
    elapsed_ms: u64,
}

impl SourceObservationMetrics {
    fn add(&mut self, other: Self) {
        self.count = self.count.saturating_add(other.count);
        self.elapsed_ms = self.elapsed_ms.saturating_add(other.elapsed_ms);
    }
}

pub(super) struct ExecuteCheckRunRequest {
    pub(super) work_plan_id: Option<String>,
    pub(super) record_receipts: bool,
    pub(super) fail_fast: bool,
}

#[cfg(test)]
pub(super) fn execute_check_run(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    plan: RunPlan,
    request: ExecuteCheckRunRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<CheckRunExecution> {
    let (run, _lease) = start_check_run(ctx, catalog, plan, request.work_plan_id.clone())?;
    execute_started_check_run(ctx, catalog, run, request, &|| Ok(cancelled()))
}

/// Executes a plan produced by this process immediately before this call.
///
/// External plans must enter through `start_check_run`, which re-derives the
/// plan from current authority. Internal callers already
/// hold that exact freshly derived value; the first target precondition still
/// rejects source drift before any target starts, so re-planning here would add
/// a full repository scan without strengthening execution safety.
pub(super) fn execute_freshly_planned_check_run(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    plan: RunPlan,
    request: ExecuteCheckRunRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<CheckRunExecution> {
    let repository_execution =
        crate::state::acquire_repository_execution_lease(ctx, &plan.effects)?;
    crate::repository::validate_current_repository_authority(ctx, &plan.config_digest)?;
    // A nonempty run gets the same source check from its first target
    // precondition. An empty affected plan has no such target, so it must prove
    // freshness here before it can become a durable success.
    if plan.targets.is_empty() {
        crate::repository::validate_run_plan_source(ctx, &plan)?;
        crate::repository::validate_current_repository_authority(ctx, &plan.config_digest)?;
    }
    let (run, _lease) = crate::state::start_run_with_execution_lease(
        ctx,
        plan,
        request.work_plan_id.clone(),
        repository_execution,
    )?;
    let mut control = ObservedRunControl { observer };
    execute_started_check_run_with_control(ctx, catalog, run, request, &mut control)
}

#[cfg(test)]
pub(super) fn start_check_run(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    plan: RunPlan,
    work_plan_id: Option<String>,
) -> Result<(crate::state::DurableRun, crate::state::RunLease)> {
    let repository_execution =
        crate::state::acquire_repository_execution_lease(ctx, &plan.effects)?;
    crate::repository::validate_run_plan(ctx, catalog, &plan)?;
    crate::state::start_run_with_execution_lease(ctx, plan, work_plan_id, repository_execution)
}

pub(super) fn start_check_run_with_event_cursor(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    plan: RunPlan,
    work_plan_id: Option<String>,
) -> Result<(
    crate::state::DurableRun,
    crate::state::RunLease,
    crate::state::RunEventCursor,
)> {
    let repository_execution =
        crate::state::try_acquire_repository_execution_lease(ctx, &plan.effects)?.ok_or_else(
            || {
                anyhow::anyhow!(
                    "repository execution is busy with an incompatible run; retry after it finishes or cancel that run first"
                )
            },
        )?;
    crate::repository::validate_run_plan(ctx, catalog, &plan)?;
    crate::state::start_run_with_event_cursor_and_execution_lease(
        ctx,
        plan,
        work_plan_id,
        repository_execution,
    )
}

pub(super) fn execute_started_check_run(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    run: crate::state::DurableRun,
    request: ExecuteCheckRunRequest,
    cancelled: &dyn Fn() -> Result<bool>,
) -> Result<CheckRunExecution> {
    let mut control = CancellationOnlyRunControl { cancelled };
    execute_started_check_run_with_control(ctx, catalog, run, request, &mut control)
}

fn execute_started_check_run_with_control(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    run: crate::state::DurableRun,
    request: ExecuteCheckRunRequest,
    control: &mut dyn RepositoryRunControl,
) -> Result<CheckRunExecution> {
    let run_id = run.result.run_id.clone();
    terminalize_started_run(ctx, &run_id, || {
        execute_started_check_run_inner(ctx, catalog, run, request, control)
    })
}

fn terminalize_started_run<T>(
    ctx: &RepoContext,
    run_id: &str,
    execute: impl FnOnce() -> Result<T>,
) -> Result<T> {
    match execute() {
        Ok(result) => Ok(result),
        Err(error) => match block_started_check_run(ctx, run_id, &error) {
            Ok(()) => Err(error),
            Err(terminalization_error) => Err(anyhow::anyhow!(
                "{error:#}\nAdditionally failed to terminalize repository run '{run_id}': {terminalization_error:#}"
            )),
        },
    }
}

fn execute_started_check_run_inner(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    run: crate::state::DurableRun,
    request: ExecuteCheckRunRequest,
    control: &mut dyn RepositoryRunControl,
) -> Result<CheckRunExecution> {
    let run_id = run.result.run_id.clone();
    mark_run_running(ctx, &run_id)?;

    let mut conclusions = BTreeMap::<TargetId, RunConclusion>::new();
    let mut compatibility_results = Vec::new();
    let mut failed_targets = Vec::new();
    let mut stop_after_failure = false;
    let mut source_epoch =
        ExecutionSourceEpoch::from_plan(run.plan.source.worktree_fingerprint.clone());
    let mut parallel_source_observations = SourceObservationMetrics::default();
    let finisher = TargetFinisher {
        ctx,
        catalog,
        run: &run,
        work_plan_id: run.work_plan_id.as_deref(),
        record_receipts: request.record_receipts,
    };
    let target_count = run.plan.targets.len();
    let mut target_index = 0;

    for layer in &run.plan.execution_layers {
        let planned_layer = layer
            .iter()
            .map(|target| planned_target(&run.plan, target))
            .collect::<Result<Vec<_>>>()?;
        let can_run_concurrently = !request.fail_fast
            && !stop_after_failure
            && planned_layer.len() > 1
            && planned_layer.iter().all(|planned| {
                planned.intent == jig_contract::ActionIntent::Check
                    && planned.effects.contains(&ActionEffect::ReadOnly)
                    && !planned.effects.contains(&ActionEffect::Worktree)
                    && !planned.effects.contains(&ActionEffect::External)
                    && planned.depends_on.iter().all(|dependency| {
                        conclusions.get(dependency) == Some(&RunConclusion::Success)
                    })
            });
        if can_run_concurrently {
            let positioned = planned_layer
                .iter()
                .enumerate()
                .map(|(offset, planned)| {
                    let position = PhasePosition::new(target_index + offset + 1, target_count)
                        .expect("planned target position must be valid");
                    (*planned, position)
                })
                .collect::<Vec<_>>();
            target_index += positioned.len();
            let outcomes =
                execute_parallel_read_only_layer(ctx, catalog, &run, control, &positioned)?;
            for ((target_id, planned), outcome) in
                layer.iter().zip(planned_layer.iter()).zip(outcomes)
            {
                parallel_source_observations.add(outcome.source_observations);
                let (result, compatibility) = finisher.finish(
                    planned,
                    outcome.started_at_ms,
                    outcome.capture,
                    outcome.fingerprint,
                )?;
                record_finished_target(
                    ctx,
                    &run_id,
                    target_id,
                    result,
                    compatibility,
                    request.fail_fast,
                    &mut conclusions,
                    &mut failed_targets,
                    &mut compatibility_results,
                    &mut stop_after_failure,
                )?;
            }
            continue;
        }

        for (target_id, planned) in layer.iter().zip(planned_layer) {
            target_index += 1;
            let dependency_failed = planned.depends_on.iter().any(|dependency| {
                conclusions
                    .get(dependency)
                    .is_some_and(|conclusion| *conclusion != RunConclusion::Success)
            });
            let skip_reason = match control.cancelled() {
                Ok(true) => Some((
                    RunConclusion::Cancelled,
                    "run cancellation was requested before the target started".to_owned(),
                )),
                Err(error) => Some((
                    RunConclusion::Blocked,
                    format!("run cancellation state could not be inspected: {error:#}"),
                )),
                Ok(false) if dependency_failed => Some((
                    RunConclusion::Skipped,
                    "a declared target dependency did not succeed".to_owned(),
                )),
                Ok(false) if stop_after_failure => Some((
                    RunConclusion::Skipped,
                    "the run stopped after an earlier failure because fail-fast was requested"
                        .to_owned(),
                )),
                Ok(false) => None,
            };

            let (result, compatibility) = if let Some((conclusion, reason)) = skip_reason {
                // A skipped target may spend time recording durable state but
                // does not take its own source precondition. Do not carry an
                // earlier target's observation across that unobserved gap.
                source_epoch.discard_reusable_observation();
                let capture = TargetCapture::not_started(conclusion, reason)
                    .with_alias(catalog.aliases_for_target(&planned.target).first().cloned());
                finisher.finish(
                    planned,
                    None,
                    capture,
                    Err(format!(
                        "target '{}' did not start, so no execution-time worktree fingerprint was observed",
                        planned.target
                    )),
                )?
            } else if let Err(error) = crate::repository::validate_current_repository_authority(
                ctx,
                &run.plan.config_digest,
            ) {
                source_epoch.discard_reusable_observation();
                let message = format!(
                    "target '{}' could not start because repository execution authority could not be verified: {error:#}",
                    planned.target
                );
                let capture = TargetCapture::blocked(message.clone())
                    .with_alias(catalog.aliases_for_target(&planned.target).first().cloned());
                finisher.finish(planned, None, capture, Err(message))?
            } else if let Err(message) = source_epoch.prepare_target(ctx, planned) {
                let capture = TargetCapture::blocked(message)
                    .with_alias(catalog.aliases_for_target(&planned.target).first().cloned());
                finisher.finish(planned, None, capture, source_epoch.receipt_fingerprint())?
            } else {
                mark_target_started(ctx, &run_id, target_id.clone())?;
                let started_at_ms = now_ms();
                let label = format!("Repository target '{}'", planned.target);
                let phase = ExecutionPhase::start(
                    control,
                    &label,
                    PhasePosition::new(target_index, target_count)
                        .expect("planned target position must be valid"),
                );
                let (capture, fingerprint) =
                    run_target(ctx, catalog, planned, control, &mut source_epoch);
                phase.finish(control, capture.conclusion == RunConclusion::Success);
                finisher.finish(planned, Some(started_at_ms), capture, fingerprint)?
            };

            record_finished_target(
                ctx,
                &run_id,
                target_id,
                result,
                compatibility,
                request.fail_fast,
                &mut conclusions,
                &mut failed_targets,
                &mut compatibility_results,
                &mut stop_after_failure,
            )?;
        }
    }

    if run.plan.targets.is_empty() {
        crate::repository::validate_run_plan_source(ctx, &run.plan)?;
        crate::repository::validate_current_repository_authority(ctx, &run.plan.config_digest)?;
    }
    let conclusion = aggregate_conclusion(conclusions.values().copied());
    complete_run(ctx, &run_id, conclusion)?;
    let mut source_observations = source_epoch.metrics();
    source_observations.add(parallel_source_observations);
    debug_assert!(source_observations.count <= target_count.saturating_mul(2));
    Ok(CheckRunExecution {
        run: run_by_id(ctx, &run_id)?,
        results: compatibility_results,
        failed_targets,
        source_observations,
    })
}

#[allow(clippy::too_many_arguments)]
fn record_finished_target(
    ctx: &RepoContext,
    run_id: &str,
    target_id: &TargetId,
    result: TargetRunResult,
    compatibility: Option<Value>,
    fail_fast: bool,
    conclusions: &mut BTreeMap<TargetId, RunConclusion>,
    failed_targets: &mut Vec<TargetId>,
    compatibility_results: &mut Vec<Value>,
    stop_after_failure: &mut bool,
) -> Result<()> {
    let conclusion = result
        .conclusion
        .expect("finished target results always have a conclusion");
    conclusions.insert(target_id.clone(), conclusion);
    if matches!(
        conclusion,
        RunConclusion::Failure | RunConclusion::TimedOut | RunConclusion::Blocked
    ) {
        failed_targets.push(target_id.clone());
        *stop_after_failure |= fail_fast;
    }
    record_target_result(ctx, run_id, result)?;
    if let Some(compatibility) = compatibility {
        compatibility_results.push(compatibility);
    }
    Ok(())
}

mod parallel;
use parallel::*;

pub(super) fn block_started_check_run(
    ctx: &RepoContext,
    run_id: &str,
    error: &anyhow::Error,
) -> Result<()> {
    let message = format!("repository run worker stopped unexpectedly: {error:#}");
    crate::state::block_nonterminal_run(ctx, run_id, &message)
}

fn planned_target<'a>(plan: &'a RunPlan, target: &TargetId) -> Result<&'a PlannedTarget> {
    plan.targets
        .iter()
        .find(|planned| &planned.target == target)
        .ok_or_else(|| anyhow::anyhow!("run plan references missing target '{target}'"))
}

fn run_target(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    planned: &PlannedTarget,
    run_control: &mut dyn RepositoryRunControl,
    source_epoch: &mut ExecutionSourceEpoch,
) -> (TargetCapture, std::result::Result<String, String>) {
    let mut control = TargetExecutionControl::new(ctx, planned, run_control);
    let capture = match &planned.runner {
        ActionRunner::Command {
            command,
            working_directory,
            environment,
        } => run_command_target(
            ctx,
            planned,
            command,
            working_directory.as_deref(),
            environment,
            &mut control,
        ),
        ActionRunner::Native { operation } => match control.remaining() {
            Err(stop) => stopped_before_start(planned, stop),
            Ok(timeout) => match run_native_tool_with_control(
                ctx,
                operation,
                Some(&planned.target),
                &json!(planned.arguments),
                timeout,
                &|| control.is_cancelled(),
            ) {
                Ok(output) => TargetCapture::from_process(
                    output.exit_status,
                    output.stdout,
                    output.stderr,
                    planned.result_parser,
                ),
                Err(error) => native_runner_error_capture(planned, operation, timeout, error),
            },
        },
    };
    let capture = control
        .enforce_poll_health(capture)
        .with_alias(catalog.aliases_for_target(&planned.target).first().cloned());
    let capture =
        enforce_current_repository_authority(ctx, catalog.config_digest(), planned, capture);
    source_epoch.finish_target(ctx, planned, capture)
}

fn native_runner_error_capture(
    planned: &PlannedTarget,
    operation: &str,
    timeout: Duration,
    error: anyhow::Error,
) -> TargetCapture {
    match error.downcast_ref::<OwnedProcessTreeError>() {
        Some(OwnedProcessTreeError::TimedOut) => TargetCapture::stopped_after_start(
            RunConclusion::TimedOut,
            format!(
                "native target '{}' exceeded its {timeout:?} timeout",
                planned.target
            ),
        ),
        Some(OwnedProcessTreeError::CancelledBeforeStart) => TargetCapture::not_started(
            RunConclusion::Cancelled,
            format!("native target '{}' was cancelled", planned.target),
        ),
        Some(OwnedProcessTreeError::Cancelled) => TargetCapture::stopped_after_start(
            RunConclusion::Cancelled,
            format!("native target '{}' was cancelled", planned.target),
        ),
        _ => TargetCapture::blocked(format!(
            "native runner '{operation}' for target '{}' failed: {error:#}",
            planned.target
        ))
        .with_maybe_executed(true),
    }
}

fn enforce_current_repository_authority(
    ctx: &RepoContext,
    expected_digest: &str,
    planned: &PlannedTarget,
    mut capture: TargetCapture,
) -> TargetCapture {
    let Err(error) = crate::repository::validate_current_repository_authority(ctx, expected_digest)
    else {
        return capture;
    };
    let message = format!(
        "repository execution authority could not be verified after target '{}': {error:#}",
        planned.target
    );
    if !capture.stderr.is_empty() && !capture.stderr.ends_with('\n') {
        capture.stderr.push('\n');
    }
    capture.stderr.push_str(&message);
    capture.stderr.push('\n');
    capture
        .findings
        .push(finding(message, "execution_authority"));
    if capture.conclusion == RunConclusion::Success {
        capture.conclusion = RunConclusion::Blocked;
        capture.receipt_exit_status = capture.receipt_exit_status.max(1);
    }
    capture
}

mod source_epoch;
use source_epoch::*;

trait RepositoryRunControl: ExecutionObserver {
    fn cancelled(&self) -> Result<bool>;
}

struct ObservedRunControl<'a> {
    observer: &'a mut dyn ExecutionControl,
}

impl ExecutionObserver for ObservedRunControl<'_> {
    fn event(&mut self, event: ExecutionEvent<'_>) {
        self.observer.event(event);
    }
}

impl RepositoryRunControl for ObservedRunControl<'_> {
    fn cancelled(&self) -> Result<bool> {
        Ok(self.observer.cancelled())
    }
}

struct CancellationOnlyRunControl<'a> {
    cancelled: &'a dyn Fn() -> Result<bool>,
}

impl ExecutionObserver for CancellationOnlyRunControl<'_> {}

impl RepositoryRunControl for CancellationOnlyRunControl<'_> {
    fn cancelled(&self) -> Result<bool> {
        (self.cancelled)()
    }
}

struct TargetExecutionControl<'a> {
    started: Instant,
    timeout: Duration,
    run_control: &'a mut dyn RepositoryRunControl,
    poll_failure: Mutex<Option<String>>,
}

impl<'a> TargetExecutionControl<'a> {
    fn new(
        ctx: &RepoContext,
        planned: &PlannedTarget,
        run_control: &'a mut dyn RepositoryRunControl,
    ) -> Self {
        let timeout = planned
            .timeout_seconds
            .map(Duration::from_secs)
            .unwrap_or_else(|| ctx.command_timeout().duration());
        Self {
            started: Instant::now(),
            timeout,
            run_control,
            poll_failure: Mutex::new(None),
        }
    }

    fn remaining(&self) -> std::result::Result<Duration, TargetStop> {
        match self.poll_cancelled() {
            Ok(true) => return Err(TargetStop::Cancelled),
            Ok(false) => {}
            Err(message) => return Err(TargetStop::Blocked(message)),
        }
        let remaining = self.timeout.saturating_sub(self.started.elapsed());
        if remaining.is_zero() {
            Err(TargetStop::TimedOut)
        } else {
            Ok(remaining)
        }
    }

    fn is_cancelled(&self) -> bool {
        self.poll_cancelled().unwrap_or(true)
    }

    fn poll_cancelled(&self) -> std::result::Result<bool, String> {
        if let Some(message) = self.poll_failure() {
            return Err(message);
        }
        match self.run_control.cancelled() {
            Ok(cancelled) => Ok(cancelled),
            Err(error) => {
                let message = format!("cancellation state could not be inspected: {error:#}");
                *self
                    .poll_failure
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(message.clone());
                Err(message)
            }
        }
    }

    fn poll_failure(&self) -> Option<String> {
        self.poll_failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn enforce_poll_health(&self, mut capture: TargetCapture) -> TargetCapture {
        let Some(message) = self.poll_failure() else {
            return capture;
        };
        capture.stderr.push_str(&format!("{message}\n"));
        capture.findings.push(finding(message, "cancellation"));
        if matches!(
            capture.conclusion,
            RunConclusion::Success | RunConclusion::Cancelled
        ) {
            capture.conclusion = RunConclusion::Blocked;
            capture.receipt_exit_status = capture.receipt_exit_status.max(1);
        }
        capture
    }
}

impl ExecutionObserver for TargetExecutionControl<'_> {
    fn event(&mut self, event: ExecutionEvent<'_>) {
        self.run_control.event(event);
    }
}

impl ExecutionCancellation for TargetExecutionControl<'_> {
    fn cancelled(&self) -> bool {
        self.is_cancelled()
    }
}

enum TargetStop {
    Cancelled,
    TimedOut,
    Blocked(String),
}

fn stopped_before_start(planned: &PlannedTarget, stop: TargetStop) -> TargetCapture {
    match stop {
        TargetStop::Cancelled => TargetCapture::not_started(
            RunConclusion::Cancelled,
            format!("target '{}' was cancelled", planned.target),
        ),
        TargetStop::TimedOut => TargetCapture::not_started(
            RunConclusion::TimedOut,
            format!("target '{}' timed out", planned.target),
        ),
        TargetStop::Blocked(message) => TargetCapture::blocked(format!(
            "target '{}' could not start because {message}",
            planned.target
        )),
    }
}

fn run_command_target(
    ctx: &RepoContext,
    planned: &PlannedTarget,
    command_key: &str,
    working_directory: Option<&str>,
    environment: &BTreeMap<String, String>,
    control: &mut TargetExecutionControl<'_>,
) -> TargetCapture {
    let command_text = match ctx.command_for_key(command_key) {
        Ok(command) => command,
        Err(error) => {
            return TargetCapture::blocked(format!(
                "command runner '{command_key}' for target '{}' is unavailable: {error:#}",
                planned.target
            ))
            .with_command_key(command_key);
        }
    };
    let working_directory =
        match resolve_repository_working_directory(ctx.root(), working_directory) {
            Ok(path) => path,
            Err(error) => {
                return TargetCapture::blocked(format!(
                    "target '{}' has an invalid working directory: {error:#}",
                    planned.target
                ))
                .with_command_key(command_key);
            }
        };
    if let Err(error) = validate_runner_environment(environment) {
        return TargetCapture::blocked(format!(
            "target '{}' has an invalid runner environment: {error:#}",
            planned.target
        ))
        .with_command_key(command_key);
    }

    // Runner commands and their environment are checked-in execution
    // authority, equivalent in trust to a repository shell script. Preserve
    // the caller's ordinary environment and allow reviewed overrides such as
    // PATH; Jig-owned native probes use narrower scrubbed environments.
    let mut command = Command::new("bash");
    command
        .current_dir(working_directory)
        .arg("-c")
        .arg(command_text)
        .envs(environment);
    let timeout = match control.remaining() {
        Ok(timeout) => timeout,
        Err(conclusion) => {
            return stopped_before_start(planned, conclusion).with_command_key(command_key);
        }
    };
    let label = format!(
        "Command runner '{command_key}' for target '{}'",
        planned.target
    );
    match run_supervised_execution_command(
        &mut command,
        timeout,
        ctx.command_output_limit(),
        &label,
        control,
    ) {
        Ok(output) => TargetCapture::from_process(
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
            planned.result_parser,
        )
        .with_command_key(command_key),
        Err(SupervisedExecutionError::TimedOut) => TargetCapture::stopped_after_start(
            RunConclusion::TimedOut,
            format!(
                "target '{}' exceeded its {timeout:?} timeout",
                planned.target
            ),
        )
        .with_command_key(command_key),
        Err(SupervisedExecutionError::CancelledBeforeStart) => TargetCapture::not_started(
            RunConclusion::Cancelled,
            format!("target '{}' was cancelled", planned.target),
        )
        .with_command_key(command_key),
        Err(SupervisedExecutionError::Cancelled) => TargetCapture::stopped_after_start(
            RunConclusion::Cancelled,
            format!("target '{}' was cancelled", planned.target),
        )
        .with_command_key(command_key),
        Err(SupervisedExecutionError::OutputLimitExceeded {
            stream,
            stdout,
            stderr,
        }) => TargetCapture::failed_with_output(
            format!(
                "command runner '{command_key}' for target '{}' exceeded the {} byte {stream} capture limit",
                planned.target,
                ctx.command_output_limit().bytes()
            ),
            "execution_policy",
            stdout,
            stderr,
        )
        .with_command_key(command_key),
        Err(SupervisedExecutionError::Failed {
            error,
            process_started,
        }) => {
            TargetCapture::blocked(format!(
                "command runner '{command_key}' for target '{}' failed: {error:#}",
                planned.target
            ))
            .with_maybe_executed(process_started)
            .with_command_key(command_key)
        }
    }
}

mod target_result;
use target_result::*;

#[cfg(test)]
mod tests;
