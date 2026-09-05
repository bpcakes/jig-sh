use std::sync::{Arc, Mutex};

use jig_ui::dashboard::{
    DashboardSource, PlanBasis, PlanSnapshotResult, RecorderEpochId, RecorderMode, RecorderRefresh,
    RecorderRequest, SourceError, StatusOutcome, StatusPhase, StatusRefresh, StatusRequest,
    StatusSnapshot,
};

use crate::context::RepoContext;

mod epoch;

use epoch::LocalObservationEpoch;
#[cfg(test)]
use epoch::MAX_AGGREGATION_KEYS;

/// Repository-backed dashboard source. The retained epoch is replaced only
/// after a complete publishable collection and the mutex is never held during
/// repository, provider, state, loop, or gate work.
pub(crate) struct RepoDashboardSource {
    context: RepoContext,
    state: Mutex<SourceState>,
}

struct SourceState {
    last_epoch_id: Option<RecorderEpochId>,
    retained: Option<Arc<LocalObservationEpoch>>,
}

impl RepoDashboardSource {
    pub(crate) fn new(context: RepoContext) -> Self {
        Self {
            context,
            state: Mutex::new(SourceState {
                last_epoch_id: None,
                retained: None,
            }),
        }
    }

    fn allocate_epoch(&self) -> Result<RecorderEpochId, SourceError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| SourceError::InternalContract {
                message: "recorder epoch cache was poisoned".to_string(),
            })?;
        let id = state
            .last_epoch_id
            .map_or(Ok(RecorderEpochId::FIRST), RecorderEpochId::checked_next)?;
        state.last_epoch_id = Some(id);
        Ok(id)
    }

    fn retained_epoch(&self) -> Result<Option<Arc<LocalObservationEpoch>>, SourceError> {
        self.state
            .lock()
            .map(|state| state.retained.clone())
            .map_err(|_| SourceError::InternalContract {
                message: "recorder epoch cache was poisoned".to_string(),
            })
    }

    fn collect(
        &self,
        context: &RepoContext,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Arc<LocalObservationEpoch>, SourceError> {
        let id = self.allocate_epoch()?;
        LocalObservationEpoch::collect(context, id, cancelled).map(Arc::new)
    }

    fn collect_and_retain(
        &self,
        context: &RepoContext,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Arc<LocalObservationEpoch>, SourceError> {
        let epoch = self.collect(context, cancelled)?;
        self.retain(epoch)
    }

    fn retain(
        &self,
        epoch: Arc<LocalObservationEpoch>,
    ) -> Result<Arc<LocalObservationEpoch>, SourceError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| SourceError::InternalContract {
                message: "recorder epoch cache was poisoned".to_string(),
            })?;
        if state
            .retained
            .as_ref()
            .is_none_or(|retained| retained.id() < epoch.id())
        {
            state.retained = Some(Arc::clone(&epoch));
        }
        Ok(epoch)
    }

    fn collect_observed_and_retain(
        &self,
        context: &RepoContext,
        repository: jig_ui::dashboard::StatusRepositoryObservation,
        repository_errors: Vec<jig_ui::dashboard::StatusCollectionError>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Arc<LocalObservationEpoch>, SourceError> {
        let id = self.allocate_epoch()?;
        let epoch = Arc::new(LocalObservationEpoch::collect_with_repository(
            context,
            id,
            Some((repository, repository_errors)),
            cancelled,
        )?);
        self.retain(epoch)
    }
}

impl DashboardSource for RepoDashboardSource {
    fn recorder(
        &self,
        request: RecorderRequest,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<RecorderRefresh, SourceError> {
        let epoch = match request.mode {
            RecorderMode::Refresh => {
                let current = crate::runtime::refreshed_repository_context(&self.context)
                    .map_err(|error| epoch::collection_error(error, cancelled))?;
                self.collect_and_retain(&current, cancelled)?
            }
            RecorderMode::ReuseCurrent => {
                self.retained_epoch()?.ok_or(SourceError::NoCurrentEpoch)?
            }
        };
        Ok(RecorderRefresh {
            recorder: epoch.recorder(request.timeline_limit)?,
            status_local: epoch.status_local(),
        })
    }

    fn status(
        &self,
        request: StatusRequest,
        phase_changed: &dyn Fn(StatusPhase),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<StatusRefresh, SourceError> {
        let current = crate::runtime::refreshed_repository_context(&self.context)
            .map_err(|error| epoch::collection_error(error, cancelled))?;
        phase_changed(StatusPhase::Providers);
        let provider_collection =
            crate::status::dashboard_provider_snapshot_with_cancellation(&current, cancelled)
                .map_err(|error| epoch::collection_error(error, cancelled))?;
        phase_changed(StatusPhase::LocalEpoch);
        let local = self.collect_observed_and_retain(
            &current,
            provider_collection.repository,
            provider_collection.repository_errors,
            cancelled,
        )?;
        let providers = provider_collection.snapshot;
        let status_local = local.status_local();
        let partial = !providers.errors.is_empty()
            || providers
                .providers
                .iter()
                .any(|provider| provider.status != "complete")
            || !status_local.errors.is_empty();
        let status = StatusSnapshot {
            ok: true,
            command: jig_ui::dashboard::STATUS_COMMAND.to_string(),
            schema_version: jig_ui::dashboard::STATUS_SCHEMA_VERSION,
            observed_at_ms: crate::state::now_ms(),
            outcome: if partial {
                StatusOutcome::Partial
            } else {
                StatusOutcome::Complete
            },
            repository: status_local.repository,
            work: status_local.work,
            loops: status_local.loops,
            providers: providers.providers,
            errors: status_local
                .errors
                .into_iter()
                .chain(providers.errors)
                .collect(),
        };
        Ok(StatusRefresh {
            status,
            recorder: local.recorder(request.timeline_limit)?,
            local_observed_at_ms: status_local.observed_at_ms,
            provider_observed_at_ms: providers.observed_at_ms,
        })
    }

    fn plan(
        &self,
        basis: PlanBasis,
        plan_id: String,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PlanSnapshotResult, SourceError> {
        let epoch = match basis {
            PlanBasis::RecorderEpoch(id) => {
                let Some(epoch) = self.retained_epoch()? else {
                    return Ok(PlanSnapshotResult::StaleRecorderEpoch);
                };
                if epoch.id() != id {
                    return Ok(PlanSnapshotResult::StaleRecorderEpoch);
                }
                epoch
            }
            PlanBasis::Fresh => {
                let current = crate::runtime::refreshed_repository_context(&self.context)
                    .map_err(|error| epoch::collection_error(error, cancelled))?;
                let id = self.allocate_epoch()?;
                return LocalObservationEpoch::fresh_plan(&current, id, &plan_id, cancelled);
            }
        };
        epoch.plan(&plan_id, cancelled)
    }
}

#[cfg(test)]
mod tests;
