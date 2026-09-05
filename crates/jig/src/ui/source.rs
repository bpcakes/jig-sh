use std::sync::{Arc, Mutex};

use jig_ui::dashboard::{
    DashboardSource, PlanBasis, PlanSnapshotResult, RecorderEpochId, RecorderMode, RecorderRefresh,
    RecorderRequest, SourceError,
};

use crate::context::RepoContext;

mod epoch;

use epoch::LocalObservationEpoch;
#[cfg(test)]
use epoch::MAX_AGGREGATION_KEYS;

/// Repository-backed dashboard source. The retained epoch is replaced only
/// after a complete publishable collection and the mutex is never held during
/// repository, state, loop, or gate work.
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
