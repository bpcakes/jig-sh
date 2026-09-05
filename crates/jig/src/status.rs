use std::collections::BTreeMap;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use jig_contract::status_provider::v1::{Category, DiagnosticLevel, Outcome, Report};
use jig_owned_process::{
    OwnedProcessTreeError, ProcessOutputLimits, format_exit_status,
    run_owned_process_tree_with_output_limits,
};
use jig_ui::dashboard::{AcceptedProviderReport, ProviderReportError};
use serde::Serialize;
use serde_json::{Value, json};

use crate::cancellation::{
    ensure_status_collection_active, is_status_collection_cancellation,
    status_collection_cancellation,
};
use crate::context::{RepoContext, StatusProviderConfig};
use crate::execution::{
    ExecutionEvent, ExecutionObserver, HeartbeatSchedule, NoopExecutionObserver, PhasePosition,
};
use crate::runtime::{
    loop_status_snapshot_with_cancellation, open_plan_gate_snapshots_with_cancellation,
    refreshed_repository_context,
};
use crate::state::{now_ms, state_summary_with_cancellation};

pub(crate) mod git;

#[cfg(test)]
use git::input_freshness;
use git::{
    GitCheckoutObservation, GitProbeError, InputFreshness, git_text_with_cancellation,
    input_freshness_with_cancellation, observe_git_checkout_with_cancellation,
};

const STATUS_SCHEMA_VERSION: u64 = 1;
const PROVIDER_STDOUT_LIMIT: usize = 8 * 1024 * 1024;
const PROVIDER_STDERR_LIMIT: usize = 64 * 1024;
const STATUS_PROVIDER_CONCURRENCY: usize = 4;

#[cfg(any(not(unix), test))]
pub(crate) fn snapshot(ctx: &RepoContext) -> Result<Value> {
    snapshot_with_cancellation(ctx, &|| false)
}

#[cfg(any(not(unix), test))]
pub(crate) fn snapshot_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    snapshot_with_cancellation_and_observer(ctx, cancelled, &mut NoopExecutionObserver)
}

pub(crate) fn snapshot_with_cancellation_and_observer(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
    observer: &mut dyn ExecutionObserver,
) -> Result<Value> {
    ensure_collection_active(cancelled)?;
    let current = refreshed_repository_context(ctx)?;
    ensure_collection_active(cancelled)?;
    let ctx = &current;
    let runs = run_providers_concurrently(ctx, cancelled, observer)?;

    // Providers are contractually read-only. Observe repository and Jig state
    // after they return so a concurrent local edit is reflected as stale or
    // dirty rather than being hidden by an earlier snapshot.
    ensure_collection_active(cancelled)?;
    let (repository, root_git, mut errors) = repository_snapshot(ctx, cancelled)?;
    ensure_collection_active(cancelled)?;
    let (work, work_errors) = work_snapshot(ctx, cancelled)?;
    errors.extend(work_errors);
    ensure_collection_active(cancelled)?;
    let (loops, loop_error) = loop_snapshot(ctx, cancelled)?;
    if let Some(error) = loop_error {
        errors.push(error);
    }

    ensure_collection_active(cancelled)?;
    let mut git_inputs = BTreeMap::new();
    git_inputs.insert(".".to_string(), root_git);
    let mut providers = Vec::with_capacity(runs.len());
    for run in runs {
        ensure_collection_active(cancelled)?;
        providers.push(provider_snapshot(
            ctx.root(),
            run,
            &mut git_inputs,
            cancelled,
        )?);
    }
    ensure_collection_active(cancelled)?;
    let partial = !errors.is_empty()
        || providers
            .iter()
            .any(|provider| provider.status != "complete");

    serde_json::to_value(StatusSnapshot {
        ok: true,
        command: "status",
        schema_version: STATUS_SCHEMA_VERSION,
        observed_at_ms: now_ms(),
        outcome: if partial { "partial" } else { "complete" },
        repository,
        work,
        loops,
        providers,
        errors,
    })
    .map_err(Into::into)
}

fn run_providers_concurrently(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
    observer: &mut dyn ExecutionObserver,
) -> Result<Vec<ProviderRun>> {
    ensure_collection_active(cancelled)?;
    let providers = ctx.status_providers();
    if providers.is_empty() {
        return Ok(Vec::new());
    }

    run_provider_tasks(ctx.root(), providers, cancelled, observer, &run_provider)
}

enum ProviderWorkerMessage {
    Started {
        index: usize,
    },
    Finished {
        index: usize,
        run: Box<ProviderRun>,
    },
    Panicked {
        index: usize,
        duration: Duration,
        message: String,
    },
}

type ProviderRunner<'a> =
    dyn Fn(&Path, &StatusProviderConfig, &dyn Fn() -> bool) -> ProviderRun + Sync + 'a;

fn run_provider_tasks(
    root: &Path,
    providers: &[StatusProviderConfig],
    cancelled: &dyn Fn() -> bool,
    observer: &mut dyn ExecutionObserver,
    runner: &ProviderRunner<'_>,
) -> Result<Vec<ProviderRun>> {
    if providers.is_empty() {
        return Ok(Vec::new());
    }

    let shared_cancelled = Arc::new(AtomicBool::new(false));
    let next_provider = AtomicUsize::new(0);
    let started = Instant::now();
    let mut heartbeat = HeartbeatSchedule::new();
    let mut ordered = (0..providers.len())
        .map(|_| None)
        .collect::<Vec<Option<ProviderRun>>>();
    let mut phases_started = vec![false; providers.len()];
    let mut worker_panics = Vec::new();
    let mut externally_cancelled = false;
    std::thread::scope(|scope| -> Result<()> {
        let (sender, receiver) = mpsc::channel();
        let worker_count = providers.len().min(STATUS_PROVIDER_CONCURRENCY);
        let mut handles = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let sender = sender.clone();
            let shared_cancelled = Arc::clone(&shared_cancelled);
            let next_provider = &next_provider;
            handles.push(scope.spawn(move || {
                loop {
                    if shared_cancelled.load(Ordering::SeqCst) {
                        break;
                    }
                    let index = next_provider.fetch_add(1, Ordering::SeqCst);
                    if index >= providers.len() || shared_cancelled.load(Ordering::SeqCst) {
                        break;
                    }
                    let item_started = Instant::now();
                    if sender
                        .send(ProviderWorkerMessage::Started { index })
                        .is_err()
                    {
                        break;
                    }
                    let run = std::panic::catch_unwind(AssertUnwindSafe(|| {
                        runner(root, &providers[index], &|| {
                            shared_cancelled.load(Ordering::SeqCst)
                        })
                    }));
                    match run {
                        Ok(run) => {
                            if sender
                                .send(ProviderWorkerMessage::Finished {
                                    index,
                                    run: Box::new(run),
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(payload) => {
                            shared_cancelled.store(true, Ordering::SeqCst);
                            let message = panic_payload_message(payload);
                            let _ = sender.send(ProviderWorkerMessage::Panicked {
                                index,
                                duration: item_started.elapsed(),
                                message,
                            });
                            break;
                        }
                    }
                }
            }));
        }
        drop(sender);

        loop {
            if cancelled() {
                externally_cancelled = true;
                shared_cancelled.store(true, Ordering::SeqCst);
            }
            let elapsed = started.elapsed();
            if heartbeat.due(elapsed) {
                observer.event(ExecutionEvent::Heartbeat {
                    label: "status providers",
                    elapsed,
                });
            }
            match receiver.recv_timeout(Duration::from_millis(25)) {
                Ok(ProviderWorkerMessage::Started { index }) => {
                    phases_started[index] = true;
                    observer.event(ExecutionEvent::PhaseStarted {
                        label: &providers[index].id,
                        position: PhasePosition::new(index + 1, providers.len())
                            .expect("status providers are enumerated within a nonempty list"),
                    });
                }
                Ok(ProviderWorkerMessage::Finished { index, run }) => {
                    observer.event(ExecutionEvent::PhaseFinished {
                        label: &providers[index].id,
                        success: run.failure.is_none(),
                        elapsed: Duration::from_millis(run.duration_ms),
                    });
                    phases_started[index] = false;
                    ordered[index] = Some(*run);
                }
                Ok(ProviderWorkerMessage::Panicked {
                    index,
                    duration,
                    message,
                }) => {
                    observer.event(ExecutionEvent::PhaseFinished {
                        label: &providers[index].id,
                        success: false,
                        elapsed: duration,
                    });
                    phases_started[index] = false;
                    worker_panics.push((index, message));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        for handle in handles {
            if handle.join().is_err() {
                return Err(anyhow!("A status provider worker panicked"));
            }
        }
        Ok(())
    })?;

    if externally_cancelled {
        return Err(status_collection_cancellation());
    }
    if !worker_panics.is_empty() {
        worker_panics.sort_by_key(|(index, _)| *index);
        let diagnostics = worker_panics
            .into_iter()
            .map(|(index, message)| {
                format!(
                    "Status provider '{}' worker panicked: {message}",
                    providers[index].id
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        return Err(anyhow!("Status provider workers panicked: {diagnostics}"));
    }
    debug_assert!(phases_started.iter().all(|started| !started));
    ensure_collection_active(cancelled)?;

    ordered
        .into_iter()
        .enumerate()
        .map(|(index, run)| {
            run.ok_or_else(|| {
                anyhow!(
                    "Status provider '{}' did not return a result",
                    providers[index].id
                )
            })
        })
        .collect()
}

fn panic_payload_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

fn ensure_collection_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
}

fn propagate_git_cancellation<T>(result: std::result::Result<T, GitProbeError>) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(GitProbeError::Cancelled) => Err(status_collection_cancellation()),
        Err(GitProbeError::Failed(message)) => Err(anyhow!(message)),
    }
}

#[derive(Serialize)]
struct StatusSnapshot {
    ok: bool,
    command: &'static str,
    schema_version: u64,
    observed_at_ms: u64,
    outcome: &'static str,
    repository: RepositorySnapshot,
    work: Value,
    loops: Value,
    providers: Vec<ProviderSnapshot>,
    errors: Vec<StatusCollectionError>,
}

#[derive(Serialize)]
struct RepositorySnapshot {
    name: String,
    default_branch: String,
    head_revision: Option<String>,
    branch: Option<String>,
    detached: bool,
    dirty: Option<bool>,
    upstream: Option<UpstreamSnapshot>,
}

#[derive(Serialize)]
struct UpstreamSnapshot {
    reference: String,
    ahead: u64,
    behind: u64,
    state: &'static str,
    basis: &'static str,
}

#[derive(Serialize)]
struct StatusCollectionError {
    scope: String,
    code: &'static str,
    message: String,
}

fn repository_snapshot(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<(
    RepositorySnapshot,
    GitCheckoutObservation,
    Vec<StatusCollectionError>,
)> {
    ensure_collection_active(cancelled)?;
    let root_git = propagate_git_cancellation(observe_git_checkout_with_cancellation(
        ctx.root(),
        cancelled,
    ))?;
    ensure_collection_active(cancelled)?;
    let mut errors = root_git
        .errors
        .iter()
        .map(|message| StatusCollectionError {
            scope: "repository".into(),
            code: "git_observation_failed",
            message: message.clone(),
        })
        .collect::<Vec<_>>();

    let branch = match git_text_with_cancellation(
        ctx.root(),
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
        cancelled,
    ) {
        Ok(branch) => Some(branch),
        Err(GitProbeError::Failed(_)) => None,
        Err(GitProbeError::Cancelled) => return Err(status_collection_cancellation()),
    };
    ensure_collection_active(cancelled)?;
    let upstream = local_upstream_snapshot(ctx.root(), &mut errors, cancelled)?;
    Ok((
        RepositorySnapshot {
            name: ctx.repo_name().to_string(),
            default_branch: ctx.default_branch().to_string(),
            head_revision: root_git.revision.clone(),
            detached: root_git.revision.is_some() && branch.is_none(),
            branch,
            dirty: root_git.dirty,
            upstream,
        },
        root_git,
        errors,
    ))
}

fn local_upstream_snapshot(
    root: &Path,
    errors: &mut Vec<StatusCollectionError>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<UpstreamSnapshot>> {
    ensure_collection_active(cancelled)?;
    let reference = match git_text_with_cancellation(
        root,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
        cancelled,
    ) {
        Ok(reference) => reference,
        Err(GitProbeError::Failed(_)) => return Ok(None),
        Err(GitProbeError::Cancelled) => return Err(status_collection_cancellation()),
    };
    ensure_collection_active(cancelled)?;
    let counts = match git_text_with_cancellation(
        root,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
        cancelled,
    ) {
        Ok(counts) => counts,
        Err(GitProbeError::Failed(message)) => {
            errors.push(StatusCollectionError {
                scope: "repository.upstream".into(),
                code: "git_upstream_comparison_failed",
                message,
            });
            return Ok(None);
        }
        Err(GitProbeError::Cancelled) => return Err(status_collection_cancellation()),
    };
    ensure_collection_active(cancelled)?;
    let mut fields = counts.split_whitespace();
    let ahead = fields.next().and_then(|field| field.parse::<u64>().ok());
    let behind = fields.next().and_then(|field| field.parse::<u64>().ok());
    let (Some(ahead), Some(behind)) = (ahead, behind) else {
        errors.push(StatusCollectionError {
            scope: "repository.upstream".into(),
            code: "git_upstream_output_invalid",
            message: format!("git rev-list returned unexpected counts: {counts:?}"),
        });
        return Ok(None);
    };
    let state = match (ahead, behind) {
        (0, 0) => "in_sync",
        (_, 0) => "ahead",
        (0, _) => "behind",
        _ => "diverged",
    };
    Ok(Some(UpstreamSnapshot {
        reference,
        ahead,
        behind,
        state,
        basis: "local_tracking_ref",
    }))
}

fn work_snapshot(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<(Value, Vec<StatusCollectionError>)> {
    ensure_collection_active(cancelled)?;
    let state = match state_summary_with_cancellation(ctx, cancelled) {
        Ok(state) => state,
        Err(error) if is_status_collection_cancellation(&error) => return Err(error),
        Err(error) => {
            ensure_collection_active(cancelled)?;
            return Ok((
                json!({
                    "state": null,
                    "gates": [],
                }),
                vec![StatusCollectionError {
                    scope: "work.state".into(),
                    code: "work_state_unavailable",
                    message: format!("{error:#}"),
                }],
            ));
        }
    };
    ensure_collection_active(cancelled)?;

    let mut errors = Vec::new();
    let open_plan_ids = state["open_plans"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(|plan| plan["plan_id"].as_str())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let gate_snapshots = if open_plan_ids.is_empty() {
        Ok(BTreeMap::new())
    } else {
        open_plan_gate_snapshots_with_cancellation(ctx, &open_plan_ids, cancelled)
    };
    let mut gates = Vec::with_capacity(open_plan_ids.len());
    for plan_id in open_plan_ids {
        ensure_collection_active(cancelled)?;
        gates.push(match &gate_snapshots {
            Ok(snapshots) => match snapshots.get(&plan_id) {
                Some(snapshot) => json!({
                    "plan_id": plan_id,
                    "snapshot": snapshot,
                    "error": null,
                }),
                None => {
                    let message = format!(
                        "Batched gate evaluation did not return requested plan '{plan_id}'"
                    );
                    errors.push(StatusCollectionError {
                        scope: format!("work.gates.{plan_id}"),
                        code: "work_gates_unavailable",
                        message: message.clone(),
                    });
                    json!({
                        "plan_id": plan_id,
                        "snapshot": null,
                        "error": message,
                    })
                }
            },
            Err(error) if is_status_collection_cancellation(error) => {
                return Err(status_collection_cancellation());
            }
            Err(error) => {
                let message = format!("{error:#}");
                errors.push(StatusCollectionError {
                    scope: format!("work.gates.{plan_id}"),
                    code: "work_gates_unavailable",
                    message: message.clone(),
                });
                json!({
                    "plan_id": plan_id,
                    "snapshot": null,
                    "error": message,
                })
            }
        });
        ensure_collection_active(cancelled)?;
    }

    Ok((
        json!({
            "state": state,
            "gates": gates,
        }),
        errors,
    ))
}

fn loop_snapshot(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<(Value, Option<StatusCollectionError>)> {
    ensure_collection_active(cancelled)?;
    let snapshot = match loop_status_snapshot_with_cancellation(ctx, cancelled) {
        Ok(snapshot) => (snapshot, None),
        Err(error) if is_status_collection_cancellation(&error) => return Err(error),
        Err(error) => (
            Value::Null,
            Some(StatusCollectionError {
                scope: "loops".into(),
                code: "loop_status_unavailable",
                message: format!("{error:#}"),
            }),
        ),
    };
    ensure_collection_active(cancelled)?;
    Ok(snapshot)
}

#[derive(Debug)]
struct ProviderRun {
    id: String,
    duration_ms: u64,
    report: Option<ValidatedReport>,
    failure: Option<ProviderFailure>,
}

type ValidatedReport = AcceptedProviderReport;

#[derive(Serialize)]
struct ProviderSnapshot {
    id: String,
    status: &'static str,
    duration_ms: u64,
    report: Option<Value>,
    summary: Option<ProviderSummary>,
    input_freshness: Vec<InputFreshness>,
    error: Option<ProviderFailure>,
}

#[derive(Debug, Serialize)]
struct ProviderFailure {
    code: &'static str,
    message: String,
    exit_status: Option<i32>,
    stderr: Option<String>,
    stderr_truncated: bool,
}

fn run_provider(
    root: &Path,
    provider: &StatusProviderConfig,
    cancelled: &dyn Fn() -> bool,
) -> ProviderRun {
    let started = Instant::now();
    let result = run_provider_inner_with_cancellation(root, provider, cancelled);
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    match result {
        Ok(report) => ProviderRun {
            id: provider.id.clone(),
            duration_ms,
            report: Some(report),
            failure: None,
        },
        Err(failure) => ProviderRun {
            id: provider.id.clone(),
            duration_ms,
            report: None,
            failure: Some(failure),
        },
    }
}

#[cfg(all(test, unix))]
fn run_provider_inner(
    root: &Path,
    provider: &StatusProviderConfig,
) -> std::result::Result<ValidatedReport, ProviderFailure> {
    run_provider_inner_with_cancellation(root, provider, &|| false)
}

fn run_provider_inner_with_cancellation(
    root: &Path,
    provider: &StatusProviderConfig,
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<ValidatedReport, ProviderFailure> {
    run_provider_inner_with_limits_and_cancellation(
        root,
        provider,
        PROVIDER_STDOUT_LIMIT,
        PROVIDER_STDERR_LIMIT,
        cancelled,
    )
}

#[cfg(all(test, unix))]
fn run_provider_inner_with_limits(
    root: &Path,
    provider: &StatusProviderConfig,
    stdout_limit: usize,
    stderr_limit: usize,
) -> std::result::Result<ValidatedReport, ProviderFailure> {
    run_provider_inner_with_limits_and_cancellation(
        root,
        provider,
        stdout_limit,
        stderr_limit,
        &|| false,
    )
}

fn run_provider_inner_with_limits_and_cancellation(
    root: &Path,
    provider: &StatusProviderConfig,
    stdout_limit: usize,
    stderr_limit: usize,
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<ValidatedReport, ProviderFailure> {
    let mut command = Command::new(&provider.argv[0]);
    command
        .args(&provider.argv[1..])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    sanitize_observer_environment(&mut command);

    let output = run_owned_process_tree_with_output_limits(
        &mut command,
        Duration::from_secs(provider.timeout_seconds),
        ProcessOutputLimits {
            stdout: stdout_limit,
            stderr: stderr_limit,
        },
        cancelled,
    )
    .map_err(|error| process_failure(provider, error))?;
    let stderr = output.stderr.as_ref().map(|capture| {
        let text = capture.to_string_lossy();
        text.trim().to_string()
    });
    let stderr_truncated = output
        .stderr
        .as_ref()
        .is_some_and(|capture| capture.truncated || !capture.complete);

    if !output.status.success() {
        return Err(ProviderFailure {
            code: "exit_nonzero",
            message: format!(
                "Status provider '{}' failed with {}",
                provider.id,
                format_exit_status(&output.status)
            ),
            exit_status: output.status.code(),
            stderr: stderr.filter(|text| !text.is_empty()),
            stderr_truncated,
        });
    }

    let stdout = output.stdout.ok_or_else(|| ProviderFailure {
        code: "stdout_missing",
        message: format!("Status provider '{}' stdout was not captured", provider.id),
        exit_status: output.status.code(),
        stderr: stderr.clone().filter(|text| !text.is_empty()),
        stderr_truncated,
    })?;
    if !stdout.complete {
        return Err(ProviderFailure {
            code: "stdout_incomplete",
            message: format!(
                "Status provider '{}' stdout capture did not complete",
                provider.id
            ),
            exit_status: output.status.code(),
            stderr: stderr.filter(|text| !text.is_empty()),
            stderr_truncated,
        });
    }
    if stdout.truncated {
        return Err(ProviderFailure {
            code: "stdout_limit_exceeded",
            message: format!(
                "Status provider '{}' stdout exceeded the {} byte limit",
                provider.id, stdout_limit
            ),
            exit_status: output.status.code(),
            stderr: stderr.filter(|text| !text.is_empty()),
            stderr_truncated,
        });
    }

    let stdout = std::str::from_utf8(&stdout.bytes).map_err(|error| ProviderFailure {
        code: "stdout_not_utf8",
        message: format!(
            "Status provider '{}' stdout was not UTF-8: {error}",
            provider.id
        ),
        exit_status: output.status.code(),
        stderr: stderr.clone().filter(|text| !text.is_empty()),
        stderr_truncated,
    })?;
    decode_report(provider, stdout).map_err(|mut failure| {
        failure.exit_status = output.status.code();
        failure.stderr = stderr.filter(|text| !text.is_empty());
        failure.stderr_truncated = stderr_truncated;
        failure
    })
}

fn process_failure(
    provider: &StatusProviderConfig,
    error: OwnedProcessTreeError,
) -> ProviderFailure {
    let (code, message) = match error {
        OwnedProcessTreeError::Start(error) => (
            "start_failed",
            format!("Status provider '{}' could not start: {error}", provider.id),
        ),
        OwnedProcessTreeError::TimedOut => (
            "timed_out",
            format!(
                "Status provider '{}' timed out after {} seconds",
                provider.id, provider.timeout_seconds
            ),
        ),
        OwnedProcessTreeError::CancelledBeforeStart => (
            "cancelled",
            format!("Status provider '{}' was cancelled", provider.id),
        ),
        OwnedProcessTreeError::Cancelled => (
            "cancelled",
            format!("Status provider '{}' was cancelled", provider.id),
        ),
        OwnedProcessTreeError::OutputLimitExceeded(stream) => (
            "output_limit_exceeded",
            format!(
                "Status provider '{}' exceeded its {stream} output limit",
                provider.id
            ),
        ),
        OwnedProcessTreeError::Await => (
            "await_failed",
            format!("Status provider '{}' could not be awaited", provider.id),
        ),
        OwnedProcessTreeError::Cleanup => (
            "cleanup_failed",
            format!(
                "Status provider '{}' process tree could not be cleaned up safely",
                provider.id
            ),
        ),
    };
    ProviderFailure {
        code,
        message,
        exit_status: None,
        stderr: None,
        stderr_truncated: false,
    }
}

fn decode_report(
    provider: &StatusProviderConfig,
    stdout: &str,
) -> std::result::Result<ValidatedReport, ProviderFailure> {
    let raw = serde_json::from_str::<Value>(stdout).map_err(|error| ProviderFailure {
        code: "invalid_json",
        message: format!(
            "Status provider '{}' did not emit exactly one JSON document: {error}",
            provider.id
        ),
        exit_status: None,
        stderr: None,
        stderr_truncated: false,
    })?;
    let accepted = AcceptedProviderReport::from_raw(raw).map_err(|error| ProviderFailure {
        code: "invalid_report",
        message: match error {
            ProviderReportError::Decode(message) => format!(
                "Status provider '{}' did not emit a jig.status-provider/v1 report: {message}",
                provider.id
            ),
            ProviderReportError::Validation(message) => format!(
                "Status provider '{}' emitted an invalid report: {message}",
                provider.id
            ),
        },
        exit_status: None,
        stderr: None,
        stderr_truncated: false,
    })?;
    if accepted.decoded().provider.id != provider.id {
        return Err(ProviderFailure {
            code: "provider_id_mismatch",
            message: format!(
                "Configured status provider id '{}' does not match report provider id '{}'",
                provider.id,
                accepted.decoded().provider.id
            ),
            exit_status: None,
            stderr: None,
            stderr_truncated: false,
        });
    }
    Ok(accepted)
}

fn sanitize_observer_environment(command: &mut Command) {
    crate::shell::sanitize_bash_environment(command);
    // A status command is aimed at a known checkout. Inherited GIT_DIR,
    // GIT_WORK_TREE, replacement refs, command-scoped config, or quarantine
    // variables must not redirect either a provider or Jig's own comparisons.
    crate::bootstrap::scrub_git_repository_environment_except(command, &[]);
    // Providers and Jig's own probes are observational. Restore this control
    // only after removing inherited Git variables so `git status` cannot
    // refresh stat data through an optional index lock.
    command.env("GIT_OPTIONAL_LOCKS", "0");
}

fn provider_snapshot(
    root: &Path,
    run: ProviderRun,
    git_inputs: &mut BTreeMap<String, GitCheckoutObservation>,
    cancelled: &dyn Fn() -> bool,
) -> Result<ProviderSnapshot> {
    ensure_collection_active(cancelled)?;
    let Some(report) = run.report else {
        return Ok(ProviderSnapshot {
            id: run.id,
            status: "failed",
            duration_ms: run.duration_ms,
            report: None,
            summary: None,
            input_freshness: Vec::new(),
            error: run.failure,
        });
    };
    let status = match report.decoded().outcome {
        Outcome::Complete => "complete",
        Outcome::Partial => "partial",
    };
    let mut freshness = Vec::with_capacity(report.decoded().inputs.len());
    for input in &report.decoded().inputs {
        ensure_collection_active(cancelled)?;
        freshness.push(propagate_git_cancellation(
            input_freshness_with_cancellation(root, input, git_inputs, cancelled),
        )?);
    }
    ensure_collection_active(cancelled)?;
    Ok(ProviderSnapshot {
        id: run.id,
        status,
        duration_ms: run.duration_ms,
        summary: Some(ProviderSummary::from_report(report.decoded())),
        report: Some(report.raw().clone()),
        input_freshness: freshness,
        error: None,
    })
}

mod dashboard;
pub(crate) use dashboard::{
    provider_snapshot_with_cancellation as dashboard_provider_snapshot_with_cancellation,
    repository_snapshot_with_cancellation as dashboard_repository_snapshot_with_cancellation,
};

mod summary;
use summary::ProviderSummary;

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
