use std::collections::BTreeMap;

use anyhow::Result;
use jig_contract::status_provider::v1::Outcome;
use jig_ui::dashboard::{
    ProviderFailure as DashboardProviderFailure, ProviderSummary as DashboardProviderSummary,
    StatusProvider, StatusProviderSnapshot,
};

use super::{
    NoopExecutionObserver, ProviderFailure, ensure_collection_active,
    input_freshness_with_cancellation, now_ms, propagate_git_cancellation, repository_snapshot,
    run_providers_concurrently,
};
use crate::context::RepoContext;

pub(crate) struct DashboardProviderCollection {
    pub(crate) snapshot: StatusProviderSnapshot,
    pub(crate) repository: jig_ui::dashboard::StatusRepositoryObservation,
    pub(crate) repository_errors: Vec<jig_ui::dashboard::StatusCollectionError>,
}

fn dashboard_repository(
    repository: super::RepositorySnapshot,
) -> jig_ui::dashboard::StatusRepositoryObservation {
    jig_ui::dashboard::StatusRepositoryObservation {
        name: repository.name,
        default_branch: repository.default_branch,
        head_revision: repository.head_revision,
        branch: repository.branch,
        detached: repository.detached,
        dirty: repository.dirty,
        upstream: repository
            .upstream
            .map(|upstream| jig_ui::dashboard::UpstreamObservation {
                reference: upstream.reference,
                ahead: upstream.ahead,
                behind: upstream.behind,
                state: upstream.state.to_string(),
                basis: upstream.basis.to_string(),
            }),
    }
}

fn dashboard_repository_errors(
    errors: Vec<super::StatusCollectionError>,
) -> Vec<jig_ui::dashboard::StatusCollectionError> {
    errors
        .into_iter()
        .map(|error| jig_ui::dashboard::StatusCollectionError {
            scope: error.scope,
            code: error.code.to_string(),
            message: error.message,
        })
        .collect()
}

impl ProviderFailure {
    fn into_dashboard(self) -> DashboardProviderFailure {
        DashboardProviderFailure {
            code: self.code.to_string(),
            message: self.message,
            exit_status: self.exit_status,
            stderr: self.stderr,
            stderr_truncated: self.stderr_truncated,
        }
    }
}

pub(crate) fn repository_snapshot_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<(
    jig_ui::dashboard::StatusRepositoryObservation,
    Vec<jig_ui::dashboard::StatusCollectionError>,
)> {
    let (repository, _root_git, errors) = repository_snapshot(ctx, cancelled)?;
    Ok((
        dashboard_repository(repository),
        dashboard_repository_errors(errors),
    ))
}

pub(crate) fn provider_snapshot_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<DashboardProviderCollection> {
    ensure_collection_active(cancelled)?;
    let runs = run_providers_concurrently(ctx, cancelled, &mut NoopExecutionObserver)?;
    ensure_collection_active(cancelled)?;
    let (repository, root_git, repository_errors) = repository_snapshot(ctx, cancelled)?;
    let mut git_inputs = BTreeMap::from([(".".to_string(), root_git)]);
    let mut providers = Vec::with_capacity(runs.len());
    for run in runs {
        ensure_collection_active(cancelled)?;
        let id = run.id;
        let duration_ms = run.duration_ms;
        let Some(report) = run.report else {
            let error = run
                .failure
                .expect("a provider run without a report carries a failure")
                .into_dashboard();
            providers.push(StatusProvider {
                id,
                status: "failed".to_string(),
                duration_ms,
                report: None,
                summary: None,
                input_freshness: Vec::new(),
                error: Some(error),
            });
            continue;
        };
        let status = match report.decoded().outcome {
            Outcome::Complete => "complete",
            Outcome::Partial => "partial",
        };
        let mut input_freshness = Vec::with_capacity(report.decoded().inputs.len());
        for input in &report.decoded().inputs {
            input_freshness.push(
                propagate_git_cancellation(input_freshness_with_cancellation(
                    ctx.root(),
                    input,
                    &mut git_inputs,
                    cancelled,
                ))?
                .into_dashboard(),
            );
        }
        providers.push(StatusProvider {
            id,
            status: status.to_string(),
            duration_ms,
            summary: Some(DashboardProviderSummary::from_report(report.decoded())),
            report: Some(report),
            input_freshness,
            error: None,
        });
    }
    ensure_collection_active(cancelled)?;
    Ok(DashboardProviderCollection {
        snapshot: StatusProviderSnapshot {
            observed_at_ms: now_ms(),
            providers,
            errors: Vec::new(),
        },
        repository: dashboard_repository(repository),
        repository_errors: dashboard_repository_errors(repository_errors),
    })
}
