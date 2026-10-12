use std::sync::Arc;

use jig_dashboard::{DashboardSource, RecorderRefresh, SourceError};

use super::scheduler::ScheduledRequest;
use crate::terminal::model::App;
use anyhow::Result;
use jig_tui::CooperativeWorker;

pub(super) type RefreshResult = Result<Box<RecorderRefresh>, SourceError>;

pub(super) struct RefreshWorker {
    request: ScheduledRequest,
    worker: CooperativeWorker<RefreshResult>,
}

impl RefreshWorker {
    pub(super) fn spawn(
        source: Arc<dyn DashboardSource>,
        request: ScheduledRequest,
    ) -> Result<Self> {
        let recorder_request = request.request;
        let worker = CooperativeWorker::spawn("jig-dashboard-refresh", move |cancelled| {
            source
                .recorder(recorder_request, &|| cancelled.is_cancelled())
                .map(Box::new)
        })?;
        Ok(Self { request, worker })
    }

    pub(super) fn try_finish(&mut self) -> Option<(ScheduledRequest, RefreshResult)> {
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
    result: RefreshResult,
) -> bool {
    app.set_local_refreshing(false);
    match result {
        Ok(refresh) if refresh.recorder.timeline_limit == request.request.timeline_limit.get() => {
            app.accept_recorder_refresh(*refresh)
        }
        Ok(_) => {
            app.accept_error("recorder refresh returned a different timeline limit".to_string());
            false
        }
        Err(error) => {
            app.accept_error(error.to_string());
            false
        }
    }
}
