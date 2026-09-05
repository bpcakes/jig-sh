use std::sync::Arc;

use super::scheduler::{ScheduledRequest, WorkKind};
use crate::{
    dashboard::{DashboardSource, RecorderRefresh, SourceError},
    terminal::model::App,
};
use anyhow::Result;
use jig_tui::CooperativeWorker;

pub(super) enum RefreshResult {
    Recorder(Box<RecorderRefresh>),
    Plan(crate::dashboard::PlanSnapshotResult),
}

pub(super) struct RefreshWorker {
    request: ScheduledRequest,
    worker: CooperativeWorker<Result<RefreshResult, SourceError>>,
}

impl RefreshWorker {
    pub(super) fn spawn(
        source: Arc<dyn DashboardSource>,
        request: ScheduledRequest,
    ) -> Result<Self> {
        let worker_request = request.clone();
        let worker =
            CooperativeWorker::spawn(
                "jig-dashboard-refresh",
                move |cancelled| match worker_request.kind {
                    WorkKind::Recorder(request) => source
                        .recorder(request, &|| cancelled.is_cancelled())
                        .map(Box::new)
                        .map(RefreshResult::Recorder),
                    WorkKind::Plan { basis, plan_id, .. } => source
                        .plan(basis, plan_id, &|| cancelled.is_cancelled())
                        .map(RefreshResult::Plan),
                },
            )?;
        Ok(Self { request, worker })
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
}

pub(super) fn apply_refresh_result(
    app: &mut App,
    request: &ScheduledRequest,
    result: Result<RefreshResult, SourceError>,
) -> bool {
    match &request.kind {
        WorkKind::Recorder(recorder_request) => {
            app.set_local_refreshing(false);
            match result {
                Ok(RefreshResult::Recorder(refresh))
                    if refresh.recorder.timeline_limit == recorder_request.timeline_limit.get() =>
                {
                    app.accept_recorder_refresh(*refresh)
                }
                Ok(RefreshResult::Recorder(_)) => {
                    app.accept_error(
                        "recorder refresh returned a different timeline limit".to_string(),
                    );
                    false
                }
                Ok(RefreshResult::Plan(_)) => {
                    app.accept_error(
                        "dashboard worker returned plan data for a recorder request".to_string(),
                    );
                    false
                }
                Err(error) => {
                    app.accept_error(error.to_string());
                    false
                }
            }
        }
        WorkKind::Plan { basis, plan_id, .. } => match result {
            Ok(RefreshResult::Plan(result)) => {
                app.accept_plan_result(*basis, plan_id, result);
                false
            }
            Ok(RefreshResult::Recorder(_)) => {
                app.accept_plan_error(
                    plan_id,
                    "dashboard worker returned recorder data for a plan request".to_string(),
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
