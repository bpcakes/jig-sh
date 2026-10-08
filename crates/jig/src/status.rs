use std::path::Path;

use anyhow::{Result, anyhow};
use jig_context::RepoContext;
use jig_state::cancellation::{
    ensure_status_collection_active, is_status_collection_cancellation,
    status_collection_cancellation,
};
use jig_state::now_ms;
use serde::Serialize;
use serde_json::Value;

use crate::runtime::{loop_status_snapshot_with_cancellation, refreshed_repository_context};

pub(crate) mod git;

use git::{GitProbeError, git_text_with_cancellation, observe_git_checkout_with_cancellation};

const STATUS_SCHEMA_VERSION: u64 = jig_dashboard::STATUS_SCHEMA_VERSION;

#[cfg(test)]
pub(crate) fn snapshot(ctx: &RepoContext) -> Result<Value> {
    snapshot_with_cancellation(ctx, &|| false)
}

pub(crate) fn snapshot_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    ensure_collection_active(cancelled)?;
    let current = refreshed_repository_context(ctx)?;
    ensure_collection_active(cancelled)?;
    let ctx = &current;
    let (repository, mut errors) = repository_snapshot(ctx, cancelled)?;
    ensure_collection_active(cancelled)?;
    let (loops, loop_error) = loop_snapshot(ctx, cancelled)?;
    if let Some(error) = loop_error {
        errors.push(error);
    }

    ensure_collection_active(cancelled)?;
    let partial = !errors.is_empty();

    serde_json::to_value(StatusSnapshot {
        ok: true,
        command: "status",
        schema_version: STATUS_SCHEMA_VERSION,
        observed_at_ms: now_ms(),
        outcome: if partial { "partial" } else { "complete" },
        repository,
        loops,
        errors,
    })
    .map_err(Into::into)
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
    loops: Value,
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
) -> Result<(RepositorySnapshot, Vec<StatusCollectionError>)> {
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

mod dashboard;
pub(crate) use dashboard::repository_snapshot_with_cancellation as dashboard_repository_snapshot_with_cancellation;

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
