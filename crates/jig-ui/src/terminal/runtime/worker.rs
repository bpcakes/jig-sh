use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
    mpsc::{self, Receiver},
};

use super::scheduler::{ScheduledRequest, WorkKind};
use crate::{
    dashboard::{DashboardSource, RecorderRefresh, SourceError, StatusRefresh},
    terminal::model::{App, Tab},
};
use anyhow::Result;
use jig_tui::CooperativeWorker;

pub(super) enum RefreshResult {
    Status(StatusRefresh),
    Recorder(RecorderRefresh),
    Plan(crate::dashboard::PlanSnapshotResult),
}

pub(super) struct RefreshWorker {
    request: ScheduledRequest,
    phases: Receiver<(u64, crate::dashboard::StatusPhase)>,
    phase_state: Arc<AtomicU8>,
    worker: CooperativeWorker<Result<RefreshResult, SourceError>>,
}

const PHASE_NONE: u8 = 0;
const PHASE_PROVIDERS: u8 = 1;
const PHASE_LOCAL_EPOCH: u8 = 2;
const PHASE_CANCELLING: u8 = 3;
const PHASE_FINISHED: u8 = 4;

impl RefreshWorker {
    pub(super) fn spawn(
        source: Arc<dyn DashboardSource>,
        request: ScheduledRequest,
    ) -> Result<Self> {
        let worker_request = request.clone();
        let (phase_sender, phases) = mpsc::channel();
        let phase_state = Arc::new(AtomicU8::new(PHASE_NONE));
        let worker_phase_state = Arc::clone(&phase_state);
        let worker = CooperativeWorker::spawn("jig-dashboard-refresh", move |cancelled| {
            let is_cancelled = || {
                cancelled.is_cancelled()
                    || worker_phase_state.load(Ordering::SeqCst) == PHASE_CANCELLING
            };
            let result = match worker_request.kind {
                WorkKind::Status(request) => source
                    .status(
                        request,
                        &|phase| {
                            record_phase(&worker_phase_state, phase);
                            let _ = phase_sender.send((worker_request.generation, phase));
                        },
                        &is_cancelled,
                    )
                    .map(RefreshResult::Status),
                WorkKind::Recorder(request) => source
                    .recorder(request, &is_cancelled)
                    .map(RefreshResult::Recorder),
                WorkKind::Plan { basis, plan_id, .. } => source
                    .plan(basis, plan_id, &is_cancelled)
                    .map(RefreshResult::Plan),
            };
            worker_phase_state.store(PHASE_FINISHED, Ordering::SeqCst);
            result
        })?;
        Ok(Self {
            request,
            phases,
            phase_state,
            worker,
        })
    }

    pub(super) fn try_phase(&self) -> Option<(u64, crate::dashboard::StatusPhase)> {
        self.phases.try_recv().ok()
    }

    pub(super) fn try_finish(
        &mut self,
    ) -> Option<(ScheduledRequest, Result<RefreshResult, SourceError>)> {
        self.worker
            .try_finish()
            .map(|result| match result {
                Ok(value) => value,
                Err(message) => Err(SourceError::InternalContract { message }),
            })
            .map(|result| (self.request.clone(), result))
    }

    pub(super) fn cancel_and_join(&mut self) {
        self.worker.cancel_and_join();
    }

    pub(super) fn claim_provider_cancellation(&self) -> bool {
        self.phase_state
            .compare_exchange(
                PHASE_PROVIDERS,
                PHASE_CANCELLING,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
    }
}

fn record_phase(state: &AtomicU8, phase: crate::dashboard::StatusPhase) {
    match phase {
        crate::dashboard::StatusPhase::Providers => {
            let _ = state.compare_exchange(
                PHASE_NONE,
                PHASE_PROVIDERS,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
        }
        crate::dashboard::StatusPhase::LocalEpoch => {
            let _ = state.compare_exchange(
                PHASE_PROVIDERS,
                PHASE_LOCAL_EPOCH,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
        }
    }
}

pub(super) fn apply_refresh_result(
    app: &mut App,
    request: &ScheduledRequest,
    result: Result<RefreshResult, SourceError>,
) -> bool {
    match &request.kind {
        WorkKind::Status(status_request) => {
            let tab = Tab::Status;
            app.domain_mut(tab).set_refreshing(false);
            match result {
                Ok(RefreshResult::Status(refresh))
                    if refresh.recorder.timeline_limit == status_request.timeline_limit.get() =>
                {
                    app.accept_status_refresh(refresh)
                }
                Ok(RefreshResult::Status(_)) => {
                    app.accept_error(
                        tab,
                        "status refresh returned a recorder projection for a different timeline limit"
                            .to_string(),
                    );
                    false
                }
                Ok(RefreshResult::Recorder(_) | RefreshResult::Plan(_)) => {
                    app.accept_error(
                        tab,
                        "dashboard worker returned mismatched data for a status request"
                            .to_string(),
                    );
                    false
                }
                Err(error) => {
                    app.accept_error(tab, error.to_string());
                    false
                }
            }
        }
        WorkKind::Recorder(recorder_request) => {
            let tab = Tab::Work;
            app.domain_mut(tab).set_refreshing(false);
            match result {
                Ok(RefreshResult::Recorder(refresh))
                    if refresh.recorder.timeline_limit == recorder_request.timeline_limit.get() =>
                {
                    app.accept_recorder_refresh(refresh)
                }
                Ok(RefreshResult::Recorder(_)) => {
                    app.accept_error(
                        tab,
                        "recorder refresh returned a different timeline limit".to_string(),
                    );
                    false
                }
                Ok(RefreshResult::Status(_) | RefreshResult::Plan(_)) => {
                    app.accept_error(
                        tab,
                        "dashboard worker returned mismatched data for a recorder request"
                            .to_string(),
                    );
                    false
                }
                Err(error) => {
                    app.accept_error(tab, error.to_string());
                    false
                }
            }
        }
        WorkKind::Plan { basis, plan_id, .. } => match result {
            Ok(RefreshResult::Plan(result)) => {
                app.accept_plan_result(*basis, plan_id, result);
                false
            }
            Ok(RefreshResult::Status(_) | RefreshResult::Recorder(_)) => {
                app.accept_plan_error(
                    plan_id,
                    "dashboard worker returned domain data for a plan request".to_string(),
                );
                false
            }
            Err(error) => {
                app.accept_plan_error(plan_id, error.to_string());
                false
            }
        },
    }
}

#[cfg(test)]
mod tests;
