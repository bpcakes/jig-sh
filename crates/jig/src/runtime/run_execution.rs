use std::collections::BTreeMap;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use jig_contract::{
    ActionEffect, ActionRunner, Finding, FindingSeverity, PlannedTarget, ResultParser,
    RunConclusion, RunPlan, RunStatus, TargetId, TargetRunResult,
};
use jig_owned_process::OwnedProcessTreeError;
use serde::Serialize;
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::execution::{
    CompletedExecutionPhase, ExecutionCancellation, ExecutionControl, ExecutionEvent,
    ExecutionObserver, ExecutionPhase, ExecutionStream, PhasePosition, SupervisedExecutionError,
    run_supervised_execution_command,
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
use super::tool_execution::{NativeActionContext, run_prepared_native_action};

const GENERIC_TARGET_TOOL: &str = "jig.target_run";
const REPOSITORY_EXECUTION_LEASE_POLL_INTERVAL: Duration = Duration::from_millis(50);
const REPOSITORY_EXECUTION_WAIT_MESSAGE: &[u8] =
    b"Waiting for another repository execution to finish...\n";

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
        acquire_observed_repository_execution_lease(ctx, &plan.effects, observer)?;
    execute_freshly_planned_check_run_with_lease(
        ctx,
        catalog,
        plan,
        request,
        observer,
        repository_execution,
    )
}

pub(super) fn execute_freshly_planned_check_run_without_lease_wait(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    plan: RunPlan,
    request: ExecuteCheckRunRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<CheckRunExecution> {
    let repository_execution =
        crate::state::acquire_repository_execution_lease_without_wait(ctx, &plan.effects)?;
    execute_freshly_planned_check_run_with_lease(
        ctx,
        catalog,
        plan,
        request,
        observer,
        repository_execution,
    )
}

fn execute_freshly_planned_check_run_with_lease(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    plan: RunPlan,
    request: ExecuteCheckRunRequest,
    observer: &mut dyn ExecutionControl,
    repository_execution: crate::state::RepositoryExecutionLease,
) -> Result<CheckRunExecution> {
    validate_prepared_work_plan_identity(&plan, request.work_plan_id.as_deref())?;
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

fn acquire_observed_repository_execution_lease(
    ctx: &RepoContext,
    effects: &[ActionEffect],
    observer: &mut dyn ExecutionControl,
) -> Result<crate::state::RepositoryExecutionLease> {
    if let Some(lease) = crate::state::try_acquire_repository_execution_lease(ctx, effects)? {
        return Ok(lease);
    }
    if observer.cancelled() {
        bail!("repository execution was cancelled while waiting for another repository execution");
    }
    observer.event(ExecutionEvent::Output {
        stream: ExecutionStream::Stderr,
        bytes: REPOSITORY_EXECUTION_WAIT_MESSAGE,
    });
    observer.flush()?;
    loop {
        if observer.cancelled() {
            bail!(
                "repository execution was cancelled while waiting for another repository execution"
            );
        }
        if let Some(lease) = crate::state::try_acquire_repository_execution_lease(ctx, effects)? {
            return Ok(lease);
        }
        std::thread::sleep(REPOSITORY_EXECUTION_LEASE_POLL_INTERVAL);
    }
}

#[cfg(test)]
pub(super) fn start_check_run(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    plan: RunPlan,
    work_plan_id: Option<String>,
) -> Result<(crate::state::DurableRun, crate::state::RunLease)> {
    validate_prepared_work_plan_identity(&plan, work_plan_id.as_deref())?;
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
    validate_prepared_work_plan_identity(&plan, work_plan_id.as_deref())?;
    let repository_execution =
        crate::state::acquire_repository_execution_lease_without_wait(ctx, &plan.effects)?;
    crate::repository::validate_run_plan(ctx, catalog, &plan)?;
    crate::state::start_run_with_event_cursor_and_execution_lease(
        ctx,
        plan,
        work_plan_id,
        repository_execution,
    )
}

fn validate_prepared_work_plan_identity(plan: &RunPlan, supplied: Option<&str>) -> Result<()> {
    let mut prepared = plan
        .targets
        .iter()
        .filter_map(|target| target.prepared_native_input.as_ref())
        .map(|input| input.work_plan_id.as_deref());
    let Some(expected) = prepared.next() else {
        return Ok(());
    };
    if prepared.any(|candidate| candidate != expected) {
        bail!(
            "run plan '{}' contains inconsistent prepared work-plan identities",
            plan.id
        );
    }
    if expected != supplied {
        bail!(
            "run plan '{}' was prepared for work_plan_id {:?}, but execution supplied {:?}",
            plan.id,
            expected,
            supplied
        );
    }
    Ok(())
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
            let execution = execute_parallel_read_only_layer(
                ctx,
                catalog,
                &run,
                control,
                &mut source_epoch,
                &positioned,
            )?;
            for ((target_id, planned), outcome) in layer
                .iter()
                .zip(planned_layer.iter())
                .zip(execution.outcomes)
            {
                let (result, compatibility) =
                    finisher.finish(planned, outcome.completed, outcome.fingerprint)?;
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
                    CompletedTargetCapture::now(None, capture),
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
                finisher.finish(
                    planned,
                    CompletedTargetCapture::now(None, capture),
                    Err(message),
                )?
            } else if let Err(message) = source_epoch.prepare_target(ctx, planned) {
                let capture = TargetCapture::blocked(message)
                    .with_alias(catalog.aliases_for_target(&planned.target).first().cloned());
                finisher.finish(
                    planned,
                    CompletedTargetCapture::now(None, capture),
                    source_epoch.receipt_fingerprint(),
                )?
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
                let capture = run_target_capture(
                    ctx,
                    catalog,
                    &run_id,
                    run.work_plan_id.as_deref(),
                    planned,
                    control,
                );
                let completed = CompletedTargetCapture::now(Some(started_at_ms), capture);
                let (completed, fingerprint) =
                    source_epoch.finish_completed_target(ctx, planned, completed);
                phase.finish(control, completed.succeeded());
                finisher.finish(planned, completed, fingerprint)?
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
    let source_observations = source_epoch.metrics();
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

fn run_target_capture(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    run_id: &str,
    work_plan_id: Option<&str>,
    planned: &PlannedTarget,
    run_control: &mut dyn RepositoryRunControl,
) -> TargetCapture {
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
        ActionRunner::Native { operation, .. } if operation == jig_contract::tool::FILE_BUDGET => {
            match control.remaining() {
                Err(stop) => stopped_before_start(planned, stop),
                Ok(timeout) => {
                    let deadline = Instant::now()
                        .checked_add(timeout)
                        .unwrap_or_else(Instant::now);
                    match planned.prepared_native_input.as_ref() {
                        Some(prepared_input) => {
                            match run_prepared_native_action(NativeActionContext {
                                repository: ctx,
                                prepared_input,
                                deadline,
                                cancelled: &|| control.is_cancelled(),
                                run_id,
                                target: &planned.target,
                                work_plan_id,
                            }) {
                                Ok(result) => TargetCapture::from_native_action(result),
                                Err(error) => {
                                    native_runner_error_capture(planned, operation, timeout, error)
                                }
                            }
                        }
                        None => TargetCapture::blocked(format!(
                            "native target '{}' has no authenticated prepared input",
                            planned.target
                        )),
                    }
                }
            }
        }
        ActionRunner::Native { operation, .. } => match control.remaining() {
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
    enforce_current_repository_authority(ctx, catalog.config_digest(), planned, capture)
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

    fn flush(&mut self) -> Result<()> {
        self.observer.flush()
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

mod target;
use target::*;

mod target_result;
use target_result::*;

#[cfg(test)]
mod tests;
