use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use jig_contract::{
    ActionRunner, Finding, FindingSeverity, PlannedTarget, ResultParser, RunConclusion, RunPlan,
    RunStatus, TargetId, TargetRunResult,
};
use jig_owned_process::{
    OwnedProcessTreeError, ProcessOutputLimits, run_owned_process_tree_with_output_limits,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::repository::{RepositoryCatalog, target_input_digest};
use crate::state::{
    ReceiptInput, TargetReceiptMetadata, complete_run, mark_run_running, mark_target_started,
    now_ms, record_target_receipt, record_target_result, run_by_id, start_run,
};

use super::tool_execution::run_native_tool_with_control;

const DEFAULT_TARGET_TIMEOUT_SECONDS: u64 = 30 * 60;
const GENERIC_TARGET_TOOL: &str = "jig.target_run";

#[derive(Debug, Serialize)]
pub(in crate::runtime) struct CheckRunExecution {
    pub(in crate::runtime) run: crate::state::DurableRun,
    pub(in crate::runtime) results: Vec<Value>,
    pub(in crate::runtime) failed_targets: Vec<TargetId>,
}

pub(in crate::runtime) struct ExecuteCheckRunRequest {
    pub(in crate::runtime) work_plan_id: Option<String>,
    pub(in crate::runtime) record_receipts: bool,
    pub(in crate::runtime) fail_fast: bool,
}

pub(in crate::runtime) fn execute_check_run(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    plan: RunPlan,
    request: ExecuteCheckRunRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<CheckRunExecution> {
    let (run, _lease) = start_check_run(ctx, catalog, plan, request.work_plan_id.clone())?;
    execute_started_check_run(ctx, catalog, run, request, &|| Ok(cancelled()))
}

pub(in crate::runtime) fn start_check_run(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    plan: RunPlan,
    work_plan_id: Option<String>,
) -> Result<(crate::state::DurableRun, crate::state::RunLease)> {
    crate::repository::validate_run_plan(ctx, catalog, &plan)?;
    start_run(ctx, plan, work_plan_id)
}

pub(in crate::runtime) fn execute_started_check_run(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    run: crate::state::DurableRun,
    request: ExecuteCheckRunRequest,
    cancelled: &dyn Fn() -> Result<bool>,
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

    for layer in &run.plan.execution_layers {
        for target_id in layer {
            let planned = planned_target(&run.plan, target_id)?;
            let dependency_failed = planned.depends_on.iter().any(|dependency| {
                conclusions
                    .get(dependency)
                    .is_some_and(|conclusion| *conclusion != RunConclusion::Success)
            });
            let skip_reason = match cancelled() {
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
                let capture = TargetCapture::not_started(conclusion, reason);
                finisher.finish(planned, None, capture, source_epoch.receipt_fingerprint())?
            } else if let Err(message) = source_epoch.prepare_target(ctx, planned) {
                let capture = TargetCapture::blocked(message)
                    .with_alias(catalog.aliases_for_target(&planned.target).first().cloned());
                finisher.finish(planned, None, capture, source_epoch.receipt_fingerprint())?
            } else {
                mark_target_started(ctx, &run_id, target_id.clone())?;
                let started_at_ms = now_ms();
                let (capture, fingerprint) =
                    run_target(ctx, catalog, planned, cancelled, &mut source_epoch);
                finisher.finish(planned, Some(started_at_ms), capture, fingerprint)?
            };

            let conclusion = result
                .conclusion
                .expect("finished target results always have a conclusion");
            conclusions.insert(target_id.clone(), conclusion);
            if matches!(
                conclusion,
                RunConclusion::Failure | RunConclusion::TimedOut | RunConclusion::Blocked
            ) {
                failed_targets.push(target_id.clone());
                stop_after_failure |= request.fail_fast;
            }
            record_target_result(ctx, &run_id, result)?;
            if let Some(compatibility) = compatibility {
                compatibility_results.push(compatibility);
            }
        }
    }

    let conclusion = aggregate_conclusion(conclusions.values().copied());
    complete_run(ctx, &run_id, conclusion)?;
    Ok(CheckRunExecution {
        run: run_by_id(ctx, &run_id)?,
        results: compatibility_results,
        failed_targets,
    })
}

pub(in crate::runtime) fn block_started_check_run(
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
    cancelled: &dyn Fn() -> Result<bool>,
    source_epoch: &mut ExecutionSourceEpoch,
) -> (TargetCapture, std::result::Result<String, String>) {
    let control = TargetExecutionControl::new(planned, cancelled);
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
            &control,
        ),
        ActionRunner::Native { operation } => {
            let timeout = match control.remaining() {
                Ok(timeout) => timeout,
                Err(conclusion) => {
                    return (
                        stopped_before_start(planned, conclusion).with_alias(
                            catalog.aliases_for_target(&planned.target).first().cloned(),
                        ),
                        source_epoch.receipt_fingerprint(),
                    );
                }
            };
            match run_native_tool_with_control(
                ctx,
                operation,
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
                Err(error)
                    if error
                        .downcast_ref::<OwnedProcessTreeError>()
                        .is_some_and(|error| matches!(error, OwnedProcessTreeError::TimedOut)) =>
                {
                    TargetCapture::not_started(
                        RunConclusion::TimedOut,
                        format!(
                            "native target '{}' exceeded its {timeout:?} timeout",
                            planned.target
                        ),
                    )
                }
                Err(error)
                    if error
                        .downcast_ref::<OwnedProcessTreeError>()
                        .is_some_and(|error| matches!(error, OwnedProcessTreeError::Cancelled)) =>
                {
                    TargetCapture::not_started(
                        RunConclusion::Cancelled,
                        format!("native target '{}' was cancelled", planned.target),
                    )
                }
                Err(error) => TargetCapture::blocked(format!(
                    "native runner '{operation}' for target '{}' could not start: {error:#}",
                    planned.target
                )),
            }
        }
    };
    let capture = control
        .enforce_poll_health(capture)
        .with_alias(catalog.aliases_for_target(&planned.target).first().cloned());
    source_epoch.finish_target(ctx, planned, capture)
}

struct ExecutionSourceEpoch {
    fingerprint: std::result::Result<String, String>,
    validated: bool,
}

impl ExecutionSourceEpoch {
    fn from_plan(fingerprint: String) -> Self {
        Self {
            fingerprint: Ok(fingerprint),
            validated: false,
        }
    }

    fn receipt_fingerprint(&self) -> std::result::Result<String, String> {
        self.fingerprint.clone()
    }

    fn prepare_target(
        &mut self,
        ctx: &RepoContext,
        planned: &PlannedTarget,
    ) -> std::result::Result<(), String> {
        if !is_read_only(planned) {
            return Ok(());
        }
        if self.validated {
            return self.fingerprint.as_ref().map(|_| ()).map_err(Clone::clone);
        }

        let planned_fingerprint = self.fingerprint.clone()?;
        let current = collect_execution_fingerprint(ctx);
        self.fingerprint = current.clone();
        self.validated = true;
        match current {
            Ok(current) if current == planned_fingerprint => Ok(()),
            Ok(current) => Err(format!(
                "read-only target '{}' could not start because the worktree changed after plan validation (planned {planned_fingerprint}, current {current}); plan again",
                planned.target
            )),
            Err(error) => Err(format!(
                "could not establish the read-only worktree invariant before target '{}': {error}",
                planned.target
            )),
        }
    }

    fn finish_target(
        &mut self,
        ctx: &RepoContext,
        planned: &PlannedTarget,
        capture: TargetCapture,
    ) -> (TargetCapture, std::result::Result<String, String>) {
        let current = collect_execution_fingerprint(ctx);
        let capture = if is_read_only(planned) {
            match self.fingerprint.as_deref() {
                Ok(expected) => enforce_read_only_worktree(planned, expected, &current, capture),
                Err(error) => block_for_unverifiable_read_only(planned, error, capture),
            }
        } else {
            capture
        };
        self.fingerprint = current.clone();
        self.validated = true;
        (capture, current)
    }
}

fn collect_execution_fingerprint(ctx: &RepoContext) -> std::result::Result<String, String> {
    let current = crate::state::current_worktree_fingerprint(ctx);
    current.fingerprint.ok_or_else(|| {
        current
            .error
            .unwrap_or_else(|| "worktree fingerprint was unavailable".into())
    })
}

fn block_for_unverifiable_read_only(
    planned: &PlannedTarget,
    error: &str,
    mut capture: TargetCapture,
) -> TargetCapture {
    let message = format!(
        "could not verify the read-only worktree invariant for target '{}': {error}",
        planned.target
    );
    capture.stderr.push_str(&format!("{message}\n"));
    capture.findings.push(finding(message, "effect_policy"));
    if capture.conclusion == RunConclusion::Success {
        capture.conclusion = RunConclusion::Blocked;
        capture.receipt_exit_status = capture.receipt_exit_status.max(1);
    }
    capture
}

struct TargetExecutionControl<'a> {
    started: Instant,
    timeout: Duration,
    cancelled: &'a dyn Fn() -> Result<bool>,
    poll_failure: Mutex<Option<String>>,
}

impl<'a> TargetExecutionControl<'a> {
    fn new(planned: &PlannedTarget, cancelled: &'a dyn Fn() -> Result<bool>) -> Self {
        let timeout = Duration::from_secs(
            planned
                .timeout_seconds
                .unwrap_or(DEFAULT_TARGET_TIMEOUT_SECONDS)
                .max(1),
        );
        Self {
            started: Instant::now(),
            timeout,
            cancelled,
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
        match (self.cancelled)() {
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
        capture.conclusion = RunConclusion::Blocked;
        capture.receipt_exit_status = capture.receipt_exit_status.max(1);
        capture
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

fn is_read_only(planned: &PlannedTarget) -> bool {
    planned
        .effects
        .contains(&jig_contract::ActionEffect::ReadOnly)
        && !planned
            .effects
            .contains(&jig_contract::ActionEffect::Worktree)
}

fn enforce_read_only_worktree(
    planned: &PlannedTarget,
    expected: &str,
    current: &std::result::Result<String, String>,
    mut capture: TargetCapture,
) -> TargetCapture {
    debug_assert!(is_read_only(planned));

    match current.as_deref() {
        Ok(actual) if actual == expected => capture,
        Ok(actual) => {
            let message = format!(
                "the worktree fingerprint changed while read-only target '{}' was running (before {expected}, after {actual})",
                planned.target
            );
            capture.stderr.push_str(&format!("{message}\n"));
            capture.findings.push(finding(message, "effect_policy"));
            if capture.conclusion == RunConclusion::Success {
                capture.conclusion = RunConclusion::Failure;
                capture.receipt_exit_status = capture.receipt_exit_status.max(1);
            }
            capture
        }
        Err(error) => block_for_unverifiable_read_only(planned, error, capture),
    }
}

fn run_command_target(
    ctx: &RepoContext,
    planned: &PlannedTarget,
    command_key: &str,
    working_directory: Option<&str>,
    environment: &BTreeMap<String, String>,
    control: &TargetExecutionControl<'_>,
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
    let working_directory = match resolve_working_directory(ctx.root(), working_directory) {
        Ok(path) => path,
        Err(error) => {
            return TargetCapture::blocked(format!(
                "target '{}' has an invalid working directory: {error:#}",
                planned.target
            ))
            .with_command_key(command_key);
        }
    };
    if let Err(error) = validate_environment(environment) {
        return TargetCapture::blocked(format!(
            "target '{}' has an invalid runner environment: {error:#}",
            planned.target
        ))
        .with_command_key(command_key);
    }

    let mut command = Command::new("bash");
    command
        .current_dir(working_directory)
        .arg("-c")
        .arg(command_text)
        .envs(environment)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let timeout = match control.remaining() {
        Ok(timeout) => timeout,
        Err(conclusion) => {
            return stopped_before_start(planned, conclusion).with_command_key(command_key);
        }
    };
    match run_owned_process_tree_with_output_limits(
        &mut command,
        timeout,
        ProcessOutputLimits::default(),
        || control.is_cancelled(),
    ) {
        Ok(output) => {
            let exit_status = output.status.code().unwrap_or(1);
            let captured = bounded_output(output.stdout, "stdout").and_then(|stdout| {
                bounded_output(output.stderr, "stderr").map(|stderr| (stdout, stderr))
            });
            match captured {
                Ok((stdout, stderr)) => TargetCapture::from_process(
                    exit_status,
                    stdout,
                    stderr,
                    planned.result_parser,
                )
                .with_command_key(command_key),
                Err(error) => TargetCapture::blocked(format!(
                    "command runner '{command_key}' for target '{}' did not produce a complete bounded capture: {error:#}",
                    planned.target
                ))
                .with_command_key(command_key),
            }
        }
        Err(OwnedProcessTreeError::TimedOut) => TargetCapture::not_started(
            RunConclusion::TimedOut,
            format!(
                "target '{}' exceeded its {timeout:?} timeout",
                planned.target
            ),
        )
        .with_command_key(command_key),
        Err(OwnedProcessTreeError::Cancelled) => TargetCapture::not_started(
            RunConclusion::Cancelled,
            format!("target '{}' was cancelled", planned.target),
        )
        .with_command_key(command_key),
        Err(error) => TargetCapture::blocked(format!(
            "command runner '{command_key}' for target '{}' failed: {error}",
            planned.target
        ))
        .with_command_key(command_key),
    }
}

fn bounded_output(
    output: Option<jig_owned_process::BoundedProcessOutput>,
    stream: &str,
) -> Result<String> {
    let output = output.with_context(|| format!("{stream} was not captured"))?;
    if !output.complete {
        bail!("{stream} capture did not complete");
    }
    let mut value = output.to_string_lossy();
    if output.truncated {
        value.push_str("\n[output truncated by Jig]\n");
    }
    Ok(value)
}

fn resolve_working_directory(root: &Path, configured: Option<&str>) -> Result<PathBuf> {
    let Some(configured) = configured else {
        return root
            .canonicalize()
            .with_context(|| format!("failed to resolve repository root {}", root.display()));
    };
    let configured_path = Path::new(configured);
    if configured_path.as_os_str().is_empty()
        || configured_path.is_absolute()
        || configured_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("working_directory must be a non-empty relative repository path");
    }
    let canonical_root = root
        .canonicalize()
        .with_context(|| format!("failed to resolve repository root {}", root.display()))?;
    let candidate = root.join(configured_path).canonicalize().with_context(|| {
        format!(
            "failed to resolve configured path {}",
            root.join(configured_path).display()
        )
    })?;
    if !candidate.starts_with(&canonical_root) {
        bail!("working_directory resolves outside the repository");
    }
    if !candidate.is_dir() {
        bail!("working_directory is not a directory");
    }
    Ok(candidate)
}

fn validate_environment(environment: &BTreeMap<String, String>) -> Result<()> {
    for (name, value) in environment {
        if name.is_empty() || name.contains(['=', '\0']) {
            bail!("environment variable name {name:?} is invalid");
        }
        if value.contains('\0') {
            bail!("environment variable {name:?} contains a NUL byte");
        }
    }
    Ok(())
}

struct TargetFinisher<'a> {
    ctx: &'a RepoContext,
    catalog: &'a RepositoryCatalog,
    run: &'a crate::state::DurableRun,
    work_plan_id: Option<&'a str>,
    record_receipts: bool,
}

impl TargetFinisher<'_> {
    fn finish(
        &self,
        planned: &PlannedTarget,
        started_at_ms: Option<u64>,
        capture: TargetCapture,
        worktree_fingerprint: std::result::Result<String, String>,
    ) -> Result<(TargetRunResult, Option<Value>)> {
        let ended_at_ms = now_ms();
        let tool_name = capture.alias.as_deref().unwrap_or(GENERIC_TARGET_TOOL);
        let input_digest = match &worktree_fingerprint {
            Ok(fingerprint) => target_input_digest(self.catalog, &planned.target, fingerprint)?,
            Err(_) => planned.input_digest.clone(),
        };
        let receipt_id = self
            .record_receipts
            .then(|| {
                record_target_receipt(
                    self.ctx,
                    ReceiptInput {
                        tool_name,
                        args: json!({
                            "run_id": self.run.result.run_id,
                            "target": planned.target,
                        }),
                        invoked_command_key: capture.command_key.clone(),
                        plan_id: self.work_plan_id.map(str::to_owned),
                        started_at_ms: started_at_ms.unwrap_or(ended_at_ms),
                        ended_at_ms,
                        exit_status: capture.receipt_exit_status,
                        stdout: &capture.stdout,
                        stderr: &capture.stderr,
                        evidence: None,
                        session_override: None,
                        collect_git_metadata: true,
                        collect_worktree_fingerprint: false,
                        worktree_fingerprint_override: Some(worktree_fingerprint),
                    },
                    TargetReceiptMetadata {
                        run_id: self.run.result.run_id.clone(),
                        target: planned.target.clone(),
                        config_digest: self.run.plan.config_digest.clone(),
                        input_digest: input_digest.clone(),
                        findings: capture.findings.clone(),
                    },
                )
            })
            .transpose()?;

        let mut result = TargetRunResult::queued(
            planned.target.clone(),
            self.run.plan.config_digest.clone(),
            input_digest,
        );
        result.status = RunStatus::Completed;
        result.conclusion = Some(capture.conclusion);
        result.started_at_ms = started_at_ms;
        result.ended_at_ms = Some(ended_at_ms);
        result.exit_code = capture.exit_code;
        result.receipt_id.clone_from(&receipt_id);
        result.findings.clone_from(&capture.findings);

        let compatibility = started_at_ms.map(|_| {
            let alias = self
                .catalog
                .aliases_for_target(&planned.target)
                .first()
                .cloned();
            json!({
                "target": planned.target,
                "tool": alias,
                "response": {
                    "ok": true,
                    "tool": alias.as_deref().unwrap_or(GENERIC_TARGET_TOOL),
                    "command_key": capture.command_key,
                    "args": {},
                    "result": {
                        "exit_status": capture.receipt_exit_status,
                        "stdout": capture.stdout,
                        "stderr": capture.stderr,
                    },
                    "receipt_id": receipt_id,
                },
            })
        });
        Ok((result, compatibility))
    }
}

fn aggregate_conclusion(conclusions: impl Iterator<Item = RunConclusion>) -> RunConclusion {
    let conclusions = conclusions.collect::<Vec<_>>();
    if conclusions.contains(&RunConclusion::Cancelled) {
        RunConclusion::Cancelled
    } else if conclusions.contains(&RunConclusion::Failure) {
        RunConclusion::Failure
    } else if conclusions.contains(&RunConclusion::TimedOut) {
        RunConclusion::TimedOut
    } else if conclusions.contains(&RunConclusion::Blocked) {
        RunConclusion::Blocked
    } else {
        RunConclusion::Success
    }
}

struct TargetCapture {
    conclusion: RunConclusion,
    exit_code: Option<i32>,
    receipt_exit_status: i32,
    stdout: String,
    stderr: String,
    findings: Vec<Finding>,
    command_key: Option<String>,
    alias: Option<String>,
}

impl TargetCapture {
    fn from_process(
        exit_status: i32,
        stdout: String,
        stderr: String,
        parser: ResultParser,
    ) -> Self {
        let ParsedFindings {
            mut findings,
            succeeded: findings_parse_succeeded,
        } = parse_findings(parser, &stdout);
        let conclusion = if exit_status == 0 && findings_parse_succeeded {
            RunConclusion::Success
        } else {
            if exit_status != 0 {
                findings.push(finding(
                    format!("target process exited with status {exit_status}"),
                    "exit_code",
                ));
            }
            RunConclusion::Failure
        };
        let receipt_exit_status = if conclusion == RunConclusion::Success {
            exit_status
        } else {
            exit_status.max(1)
        };
        Self {
            conclusion,
            exit_code: Some(exit_status),
            receipt_exit_status,
            stdout,
            stderr,
            findings,
            command_key: None,
            alias: None,
        }
    }

    fn not_started(conclusion: RunConclusion, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            conclusion,
            exit_code: None,
            receipt_exit_status: 1,
            stdout: String::new(),
            stderr: message.clone(),
            findings: vec![finding(message, "jig")],
            command_key: None,
            alias: None,
        }
    }

    fn blocked(message: impl Into<String>) -> Self {
        Self::not_started(RunConclusion::Blocked, message)
    }

    fn with_command_key(mut self, command_key: impl Into<String>) -> Self {
        self.command_key = Some(command_key.into());
        self
    }

    fn with_alias(mut self, alias: Option<String>) -> Self {
        self.alias = alias;
        self
    }
}

struct ParsedFindings {
    findings: Vec<Finding>,
    succeeded: bool,
}

fn parse_findings(parser: ResultParser, stdout: &str) -> ParsedFindings {
    if parser == ResultParser::ExitCode {
        return ParsedFindings {
            findings: Vec::new(),
            succeeded: true,
        };
    }
    let mut findings = Vec::new();
    let mut succeeded = true;
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<Finding>(line) {
            Ok(parsed) => findings.push(parsed),
            Err(error) => {
                succeeded = false;
                findings.push(finding(
                    format!("result parser rejected JSON line: {error}"),
                    "result_parser",
                ));
            }
        }
    }
    ParsedFindings {
        findings,
        succeeded,
    }
}

fn finding(message: impl Into<String>, source: &str) -> Finding {
    let mut finding = Finding::new(FindingSeverity::Error, message);
    finding.source = Some(source.into());
    finding
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_env::TestRepoBuilder;
    use tempfile::tempdir;

    #[test]
    fn working_directory_rejects_parent_escape() {
        let temp = tempdir().unwrap();
        assert!(
            resolve_working_directory(temp.path(), Some("../outside"))
                .unwrap_err()
                .to_string()
                .contains("relative repository path")
        );
    }

    #[test]
    fn json_lines_parser_normalizes_findings_and_rejects_bad_lines() {
        let valid = r#"{"severity":"warning","message":"unused","source":"lint"}"#;
        let parsed = parse_findings(ResultParser::JsonLines, valid);
        assert_eq!(parsed.findings[0].severity, FindingSeverity::Warning);
        assert!(parsed.succeeded);

        let parsed = parse_findings(ResultParser::JsonLines, "not-json");
        assert!(!parsed.succeeded);
    }

    #[test]
    fn tool_findings_cannot_spoof_result_parser_failure() {
        let valid =
            r#"{"severity":"warning","message":"named like the parser","source":"result_parser"}"#;
        let capture =
            TargetCapture::from_process(0, valid.into(), String::new(), ResultParser::JsonLines);

        assert_eq!(capture.conclusion, RunConclusion::Success);
        assert_eq!(capture.findings[0].source.as_deref(), Some("result_parser"));
    }

    #[test]
    fn bounded_output_rejects_missing_and_incomplete_captures() {
        assert!(
            bounded_output(None, "stdout")
                .unwrap_err()
                .to_string()
                .contains("not captured")
        );
        let incomplete = jig_owned_process::BoundedProcessOutput {
            bytes: b"partial".to_vec(),
            truncated: false,
            complete: false,
        };

        assert!(
            bounded_output(Some(incomplete), "stderr")
                .unwrap_err()
                .to_string()
                .contains("did not complete")
        );
    }

    #[test]
    fn cancellation_poll_failures_block_target_execution() {
        let target: TargetId = "repo:test".parse().unwrap();
        let planned = PlannedTarget::new(
            target,
            jig_contract::ActionIntent::Check,
            ActionRunner::command("rust_test_command"),
            "sha256:input",
        );
        let control = TargetExecutionControl::new(&planned, &|| {
            Err(anyhow::anyhow!("durable state is unavailable"))
        });

        let stop = control.remaining().unwrap_err();
        assert!(matches!(stop, TargetStop::Blocked(_)));
        let capture = control.enforce_poll_health(TargetCapture::from_process(
            0,
            String::new(),
            String::new(),
            ResultParser::ExitCode,
        ));

        assert_eq!(capture.conclusion, RunConclusion::Blocked);
        assert_eq!(capture.receipt_exit_status, 1);
        assert!(capture.stderr.contains("durable state is unavailable"));
        assert_eq!(capture.findings[0].source.as_deref(), Some("cancellation"));
    }

    #[test]
    fn an_accepted_run_becomes_blocked_when_its_worker_stops() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(["rust_test_command"])
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let target: TargetId = "repo:test".parse().unwrap();
        let plan = RunPlan::new(
            "run-plan_1",
            "sha256:config",
            jig_contract::SourceIdentity::new(None, "sha256:worktree"),
            vec![PlannedTarget::new(
                target.clone(),
                jig_contract::ActionIntent::Check,
                ActionRunner::command("rust_test_command"),
                "sha256:input",
            )],
            vec![vec![target]],
        );
        let (run, _lease) = start_run(&ctx, plan, None).unwrap();

        block_started_check_run(&ctx, &run.result.run_id, &anyhow::anyhow!("state failure"))
            .unwrap();
        let terminal = run_by_id(&ctx, &run.result.run_id).unwrap();

        assert_eq!(terminal.result.status, RunStatus::Completed);
        assert_eq!(terminal.result.conclusion, Some(RunConclusion::Blocked));
        assert_eq!(
            terminal.result.targets[0].conclusion,
            Some(RunConclusion::Blocked)
        );
        assert!(
            terminal.result.targets[0].findings[0]
                .message
                .contains("state failure")
        );
    }
}
