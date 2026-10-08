use anyhow::Result;
use jig_context::RepoContext;

use super::repository_snapshot;

fn dashboard_repository(
    repository: super::RepositorySnapshot,
) -> jig_dashboard::StatusRepositoryObservation {
    jig_dashboard::StatusRepositoryObservation {
        name: repository.name,
        default_branch: repository.default_branch,
        head_revision: repository.head_revision,
        branch: repository.branch,
        detached: repository.detached,
        dirty: repository.dirty,
        upstream: repository
            .upstream
            .map(|upstream| jig_dashboard::UpstreamObservation {
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
) -> Vec<jig_dashboard::StatusCollectionError> {
    errors
        .into_iter()
        .map(|error| jig_dashboard::StatusCollectionError {
            scope: error.scope,
            code: error.code.to_string(),
            message: error.message,
        })
        .collect()
}

pub(crate) fn repository_snapshot_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<(
    jig_dashboard::StatusRepositoryObservation,
    Vec<jig_dashboard::StatusCollectionError>,
)> {
    let (repository, errors) = repository_snapshot(ctx, cancelled)?;
    Ok((
        dashboard_repository(repository),
        dashboard_repository_errors(errors),
    ))
}
