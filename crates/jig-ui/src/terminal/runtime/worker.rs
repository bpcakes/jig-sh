use super::*;

pub(super) enum RefreshResult {
    Status(StatusRefresh),
    Recorder(RecorderRefresh),
    Plan(crate::dashboard::PlanSnapshotResult),
}

#[derive(Clone, Debug)]
pub(super) enum WorkerRequest {
    Domain(Tab),
    Plan {
        basis: crate::dashboard::PlanBasis,
        plan_id: String,
    },
}

impl WorkerRequest {
    pub(super) const fn resets_refresh_timer(&self) -> bool {
        matches!(self, Self::Domain(_))
    }
}

pub(super) struct RefreshWorker {
    request: WorkerRequest,
    worker: CooperativeWorker<Result<RefreshResult, SourceError>>,
}

impl RefreshWorker {
    pub(super) fn same_domain(&self, tab: Tab) -> bool {
        matches!(&self.request, WorkerRequest::Domain(active) if active.same_domain(tab))
    }

    pub(super) fn spawn(source: Arc<dyn DashboardSource>, request: WorkerRequest) -> Result<Self> {
        let worker_request = request.clone();
        CooperativeWorker::spawn("jig-dashboard-refresh", move |cancelled| {
            let is_cancelled = || cancelled.is_cancelled();
            match worker_request {
                WorkerRequest::Domain(tab) if tab.is_status_domain() => source
                    .status(
                        StatusRequest {
                            timeline_limit: TimelineLimit::DEFAULT,
                        },
                        &|_| {},
                        &is_cancelled,
                    )
                    .map(RefreshResult::Status),
                WorkerRequest::Domain(_) => source
                    .recorder(
                        RecorderRequest {
                            mode: RecorderMode::Refresh,
                            timeline_limit: TimelineLimit::DEFAULT,
                        },
                        &is_cancelled,
                    )
                    .map(RefreshResult::Recorder),
                WorkerRequest::Plan { basis, plan_id } => source
                    .plan(basis, plan_id, &is_cancelled)
                    .map(RefreshResult::Plan),
            }
        })
        .map(|worker| Self { request, worker })
    }

    pub(super) fn try_finish(
        &mut self,
    ) -> Option<(WorkerRequest, Result<RefreshResult, SourceError>)> {
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
}

pub(super) fn apply_refresh_result(
    app: &mut App,
    request: WorkerRequest,
    result: Result<RefreshResult, SourceError>,
) {
    match request {
        WorkerRequest::Domain(tab) => {
            app.domain_mut(tab).set_refreshing(false);
            match result {
                Ok(RefreshResult::Status(refresh)) => app.accept_status_refresh(refresh),
                Ok(RefreshResult::Recorder(refresh)) => app.accept_recorder_refresh(refresh),
                Ok(RefreshResult::Plan(_)) => app.accept_error(
                    tab,
                    "dashboard worker returned plan data for a domain request".to_string(),
                ),
                Err(error) => app.accept_error(tab, error.to_string()),
            }
        }
        WorkerRequest::Plan { basis, plan_id } => match result {
            Ok(RefreshResult::Plan(result)) => app.accept_plan_result(basis, &plan_id, result),
            Ok(RefreshResult::Status(_) | RefreshResult::Recorder(_)) => app.accept_plan_error(
                &plan_id,
                "dashboard worker returned domain data for a plan request".to_string(),
            ),
            Err(error) => app.accept_plan_error(&plan_id, error.to_string()),
        },
    }
}
