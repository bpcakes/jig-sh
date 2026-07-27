use std::collections::BTreeMap;
use std::env;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::Result;
use jig_contract::status_provider::v1::{Category, DiagnosticLevel, Outcome, Report};
use serde::Serialize;
use serde_json::{Value, json};

use crate::context::{RepoContext, StatusProviderConfig};
use crate::doctor::{
    OwnedProcessTreeError, ProcessOutputLimits, run_owned_process_tree_with_output_limits,
};
use crate::process::format_exit_status;
use crate::runtime::{loop_status_snapshot, work_gates_snapshot};
use crate::state::{now_ms, state_summary};

mod git;
pub(crate) mod tui;

use git::{
    GitCheckoutObservation, InputFreshness, git_text, input_freshness, observe_git_checkout,
};

const STATUS_SCHEMA_VERSION: u64 = 1;
const PROVIDER_STDOUT_LIMIT: usize = 8 * 1024 * 1024;
const PROVIDER_STDERR_LIMIT: usize = 64 * 1024;

pub(crate) fn snapshot(ctx: &RepoContext) -> Result<Value> {
    snapshot_with_cancellation(ctx, &|| false)
}

pub(crate) fn snapshot_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    let runs = ctx
        .status_providers()
        .iter()
        .map(|provider| run_provider(ctx.root(), provider, cancelled))
        .collect::<Vec<_>>();

    // Providers are contractually read-only. Observe repository and Jig state
    // after they return so a concurrent local edit is reflected as stale or
    // dirty rather than being hidden by an earlier snapshot.
    let (repository, root_git, mut errors) = repository_snapshot(ctx);
    let (work, work_errors) = work_snapshot(ctx);
    errors.extend(work_errors);
    let (loops, loop_error) = loop_snapshot(ctx);
    if let Some(error) = loop_error {
        errors.push(error);
    }

    let mut git_inputs = BTreeMap::new();
    git_inputs.insert(".".to_string(), root_git);
    let providers = runs
        .into_iter()
        .map(|run| provider_snapshot(ctx.root(), run, &mut git_inputs))
        .collect::<Vec<_>>();
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
) -> (
    RepositorySnapshot,
    GitCheckoutObservation,
    Vec<StatusCollectionError>,
) {
    let root_git = observe_git_checkout(ctx.root());
    let mut errors = root_git
        .errors
        .iter()
        .map(|message| StatusCollectionError {
            scope: "repository".into(),
            code: "git_observation_failed",
            message: message.clone(),
        })
        .collect::<Vec<_>>();

    let branch = git_text(ctx.root(), &["symbolic-ref", "--quiet", "--short", "HEAD"]).ok();
    let upstream = local_upstream_snapshot(ctx.root(), &mut errors);
    (
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
    )
}

fn local_upstream_snapshot(
    root: &Path,
    errors: &mut Vec<StatusCollectionError>,
) -> Option<UpstreamSnapshot> {
    let reference = git_text(
        root,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
    .ok()?;
    let counts = match git_text(
        root,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
    ) {
        Ok(counts) => counts,
        Err(message) => {
            errors.push(StatusCollectionError {
                scope: "repository.upstream".into(),
                code: "git_upstream_comparison_failed",
                message,
            });
            return None;
        }
    };
    let mut fields = counts.split_whitespace();
    let ahead = fields.next().and_then(|field| field.parse::<u64>().ok());
    let behind = fields.next().and_then(|field| field.parse::<u64>().ok());
    let (Some(ahead), Some(behind)) = (ahead, behind) else {
        errors.push(StatusCollectionError {
            scope: "repository.upstream".into(),
            code: "git_upstream_output_invalid",
            message: format!("git rev-list returned unexpected counts: {counts:?}"),
        });
        return None;
    };
    let state = match (ahead, behind) {
        (0, 0) => "in_sync",
        (_, 0) => "ahead",
        (0, _) => "behind",
        _ => "diverged",
    };
    Some(UpstreamSnapshot {
        reference,
        ahead,
        behind,
        state,
        basis: "local_tracking_ref",
    })
}

fn work_snapshot(ctx: &RepoContext) -> (Value, Vec<StatusCollectionError>) {
    let state = match state_summary(ctx) {
        Ok(state) => state,
        Err(error) => {
            return (
                json!({
                    "state": null,
                    "gates": [],
                }),
                vec![StatusCollectionError {
                    scope: "work.state".into(),
                    code: "work_state_unavailable",
                    message: format!("{error:#}"),
                }],
            );
        }
    };

    let mut errors = Vec::new();
    let gates = state["open_plans"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(|plan| plan["plan_id"].as_str())
        .map(
            |plan_id| match work_gates_snapshot(ctx, Some(plan_id.to_string())) {
                Ok(snapshot) => json!({
                    "plan_id": plan_id,
                    "snapshot": snapshot,
                    "error": null,
                }),
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
            },
        )
        .collect::<Vec<_>>();

    (
        json!({
            "state": state,
            "gates": gates,
        }),
        errors,
    )
}

fn loop_snapshot(ctx: &RepoContext) -> (Value, Option<StatusCollectionError>) {
    match loop_status_snapshot(ctx) {
        Ok(snapshot) => (snapshot, None),
        Err(error) => (
            Value::Null,
            Some(StatusCollectionError {
                scope: "loops".into(),
                code: "loop_status_unavailable",
                message: format!("{error:#}"),
            }),
        ),
    }
}

struct ProviderRun {
    id: String,
    duration_ms: u64,
    report: Option<ValidatedReport>,
    failure: Option<ProviderFailure>,
}

#[derive(Debug)]
struct ValidatedReport {
    raw: Value,
    decoded: Report,
}

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

#[cfg(test)]
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

#[cfg(test)]
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
        OwnedProcessTreeError::Cancelled => (
            "cancelled",
            format!("Status provider '{}' was cancelled", provider.id),
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
    let decoded =
        serde_json::from_value::<Report>(raw.clone()).map_err(|error| ProviderFailure {
            code: "invalid_report",
            message: format!(
                "Status provider '{}' did not emit a jig.status-provider/v1 report: {error}",
                provider.id
            ),
            exit_status: None,
            stderr: None,
            stderr_truncated: false,
        })?;
    decoded.validate().map_err(|error| ProviderFailure {
        code: "invalid_report",
        message: format!(
            "Status provider '{}' emitted an invalid report: {error}",
            provider.id
        ),
        exit_status: None,
        stderr: None,
        stderr_truncated: false,
    })?;
    if decoded.provider.id != provider.id {
        return Err(ProviderFailure {
            code: "provider_id_mismatch",
            message: format!(
                "Configured status provider id '{}' does not match report provider id '{}'",
                provider.id, decoded.provider.id
            ),
            exit_status: None,
            stderr: None,
            stderr_truncated: false,
        });
    }
    Ok(ValidatedReport { raw, decoded })
}

fn sanitize_observer_environment(command: &mut Command) {
    crate::shell::sanitize_bash_environment(command);
    // A status command is aimed at a known checkout. Inherited GIT_DIR,
    // GIT_WORK_TREE, replacement refs, command-scoped config, or quarantine
    // variables must not redirect either a provider or Jig's own comparisons.
    for (name, _) in env::vars_os() {
        if name
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("GIT_")
        {
            command.env_remove(name);
        }
    }
}

fn provider_snapshot(
    root: &Path,
    run: ProviderRun,
    git_inputs: &mut BTreeMap<String, GitCheckoutObservation>,
) -> ProviderSnapshot {
    let Some(report) = run.report else {
        return ProviderSnapshot {
            id: run.id,
            status: "failed",
            duration_ms: run.duration_ms,
            report: None,
            summary: None,
            input_freshness: Vec::new(),
            error: run.failure,
        };
    };
    let status = match report.decoded.outcome {
        Outcome::Complete => "complete",
        Outcome::Partial => "partial",
    };
    let input_freshness = report
        .decoded
        .inputs
        .iter()
        .map(|input| input_freshness(root, input, git_inputs))
        .collect();
    ProviderSnapshot {
        id: run.id,
        status,
        duration_ms: run.duration_ms,
        summary: Some(ProviderSummary::from_report(&report.decoded)),
        report: Some(report.raw),
        input_freshness,
        error: None,
    }
}

#[derive(Serialize)]
struct ProviderSummary {
    work_packages: u64,
    work_packages_with_blockers: u64,
    blockers: u64,
    acceptance_checks: u64,
    diagnostics: DiagnosticCounts,
    specification: CategoryCounts,
    implementation: CategoryCounts,
    verification: CategoryCounts,
    acceptance: CategoryCounts,
}

impl ProviderSummary {
    fn from_report(report: &Report) -> Self {
        let mut summary = Self {
            work_packages: report.work_packages.len() as u64,
            work_packages_with_blockers: 0,
            blockers: 0,
            acceptance_checks: 0,
            diagnostics: DiagnosticCounts::default(),
            specification: CategoryCounts::default(),
            implementation: CategoryCounts::default(),
            verification: CategoryCounts::default(),
            acceptance: CategoryCounts::default(),
        };
        for package in &report.work_packages {
            if !package.blockers.is_empty() {
                summary.work_packages_with_blockers += 1;
            }
            summary.blockers += package.blockers.len() as u64;
            summary.acceptance_checks += package.acceptance_checks.len() as u64;
            summary.specification.add(package.specification.category);
            summary.implementation.add(package.implementation.category);
            summary.verification.add(package.verification.category);
            for check in &package.acceptance_checks {
                summary.acceptance.add(check.category);
            }
        }
        for diagnostic in &report.diagnostics {
            summary.diagnostics.total += 1;
            match diagnostic.level {
                DiagnosticLevel::Info => summary.diagnostics.info += 1,
                DiagnosticLevel::Warning => summary.diagnostics.warning += 1,
                DiagnosticLevel::Error => summary.diagnostics.error += 1,
            }
        }
        summary
    }
}

#[derive(Default, Serialize)]
struct DiagnosticCounts {
    total: u64,
    info: u64,
    warning: u64,
    error: u64,
}

#[derive(Default, Serialize)]
struct CategoryCounts {
    unknown: u64,
    pending: u64,
    ready: u64,
    active: u64,
    blocked: u64,
    complete: u64,
    failed: u64,
}

impl CategoryCounts {
    fn add(&mut self, category: Category) {
        match category {
            Category::Unknown => self.unknown += 1,
            Category::Pending => self.pending += 1,
            Category::Ready => self.ready += 1,
            Category::Active => self.active += 1,
            Category::Blocked => self.blocked += 1,
            Category::Complete => self.complete += 1,
            Category::Failed => self.failed += 1,
        }
    }
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
