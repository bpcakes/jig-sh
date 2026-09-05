use std::sync::Arc;

use super::{
    App, DashboardOptions, DashboardSource, EVENT_POLL_INTERVAL, Result, RuntimeAction,
    TerminalSession, event, handle_event, render,
};
use crate::dashboard::{PlanBasis, PlanSnapshotResult, RecorderMode, TimelineLimit};
use anyhow::Context;

use super::scheduler::{ScheduledRequest, Scheduler, WorkKind};
use super::worker::{RefreshResult, RefreshWorker, apply_refresh_result};

pub(super) fn run(
    terminal: &mut TerminalSession,
    source: impl DashboardSource + 'static,
    mut app: App,
    options: DashboardOptions,
    externally_cancelled: impl Fn() -> bool,
) -> Result<()> {
    let source: Arc<dyn DashboardSource> = Arc::new(source);
    let mut scheduler = Scheduler::new(options.refresh_interval, options.timeline_limit);
    scheduler.queue_recorder(RecorderMode::Refresh);
    let mut worker = None;
    let mut dirty = true;

    loop {
        if externally_cancelled() {
            shutdown(
                terminal,
                &mut app,
                &mut scheduler,
                &mut worker,
                "cancelling active collection before exit",
            )?;
            return Ok(());
        }
        dirty |= finish_worker(&mut app, &mut scheduler, &mut worker)?;
        drain_plan_intent(&mut app, &mut scheduler);
        scheduler.enqueue_due(std::time::Instant::now());
        dirty |= start_next(&source, &mut app, &mut scheduler, &mut worker)?;

        if dirty {
            terminal
                .draw(|frame| render::draw(frame, &app))
                .context("failed to draw the status TUI")?;
            dirty = false;
        }

        if event::poll(EVENT_POLL_INTERVAL).context("failed to poll terminal input")? {
            let action = handle_event(
                &mut app,
                event::read().context("failed to read terminal input")?,
            );
            if action == RuntimeAction::Quit {
                shutdown(
                    terminal,
                    &mut app,
                    &mut scheduler,
                    &mut worker,
                    "cancelling active collection before exit",
                )?;
                return Ok(());
            }
            dirty |= apply_action(&mut app, &mut scheduler, action);
        }
    }
}

fn finish_worker(
    app: &mut App,
    scheduler: &mut Scheduler,
    worker: &mut Option<RefreshWorker>,
) -> Result<bool> {
    let Some((request, result)) = worker.as_mut().and_then(RefreshWorker::try_finish) else {
        return Ok(false);
    };
    *worker = None;
    if !scheduler.is_active_generation(request.generation) {
        anyhow::bail!(
            "dashboard worker generation {} completed without matching active scheduler work",
            request.generation
        );
    }
    let detail_superseded =
        matches!(&request.kind, WorkKind::Plan { .. }) && scheduler.detail_pending();
    let projection_is_outdated = match &request.kind {
        WorkKind::Recorder(request) => request.timeline_limit != scheduler.timeline_limit(),
        WorkKind::Plan { .. } => false,
    };
    if projection_is_outdated {
        apply_outdated_projection(app, scheduler, &request, result);
    } else if detail_superseded {
        // The newer queued detail request owns the visible loading state.
    } else if let Some((basis, plan_id, stale_retries)) = stale_detail_retry(app, &request, &result)
    {
        app.detail.refresh_plan(plan_id.clone(), basis);
        scheduler.retry_stale_detail(basis, plan_id, stale_retries);
    } else {
        apply_refresh_result(app, &request, result);
    }
    scheduler.complete(request.generation, std::time::Instant::now());
    Ok(true)
}

fn apply_outdated_projection(
    app: &mut App,
    scheduler: &mut Scheduler,
    request: &ScheduledRequest,
    result: std::result::Result<RefreshResult, crate::dashboard::SourceError>,
) {
    match (&request.kind, result) {
        (WorkKind::Recorder(recorder_request), Ok(RefreshResult::Recorder(refresh)))
            if refresh.recorder.timeline_limit == recorder_request.timeline_limit.get() =>
        {
            app.recorder.refreshing = false;
        }
        (_, result) => {
            apply_refresh_result(app, request, result);
        }
    }
    if !scheduler.current_local_projection_pending() {
        scheduler.queue_recorder(RecorderMode::ReuseCurrent);
    }
}

fn stale_detail_retry(
    app: &App,
    request: &ScheduledRequest,
    result: &std::result::Result<RefreshResult, crate::dashboard::SourceError>,
) -> Option<(PlanBasis, String, u8)> {
    let WorkKind::Plan {
        basis: PlanBasis::RecorderEpoch(request_epoch),
        plan_id,
        stale_retries,
    } = &request.kind
    else {
        return None;
    };
    if *stale_retries >= 1
        || !matches!(
            result,
            Ok(RefreshResult::Plan(PlanSnapshotResult::StaleRecorderEpoch))
        )
        || app.detail.loading_plan.as_deref() != Some(plan_id)
    {
        return None;
    }
    let current_epoch = app.recorder.data.as_ref()?.epoch_id;
    (current_epoch != *request_epoch).then(|| {
        (
            PlanBasis::RecorderEpoch(current_epoch),
            plan_id.clone(),
            stale_retries + 1,
        )
    })
}

fn drain_plan_intent(app: &mut App, scheduler: &mut Scheduler) {
    if let Some((basis, plan_id)) = app.take_plan_request() {
        scheduler.queue_detail(basis, plan_id);
    }
}

fn start_next(
    source: &Arc<dyn DashboardSource>,
    app: &mut App,
    scheduler: &mut Scheduler,
    worker: &mut Option<RefreshWorker>,
) -> Result<bool> {
    if worker.is_some() {
        return Ok(false);
    }
    let Some(request) = scheduler.start_next() else {
        return Ok(false);
    };
    if matches!(request.kind, WorkKind::Recorder(_)) {
        app.recorder.refreshing = true;
    }
    *worker = Some(RefreshWorker::spawn(Arc::clone(source), request)?);
    Ok(true)
}

fn apply_action(app: &mut App, scheduler: &mut Scheduler, action: RuntimeAction) -> bool {
    match action {
        RuntimeAction::Ignore => return false,
        RuntimeAction::Redraw | RuntimeAction::TabChanged => {}
        RuntimeAction::Refresh => scheduler.queue_recorder(RecorderMode::Refresh),
        RuntimeAction::DetailRequested => drain_plan_intent(app, scheduler),
        RuntimeAction::RefreshDetail => queue_detail_refresh(app, scheduler),
        RuntimeAction::GrowTimeline => change_timeline_limit(app, scheduler, true),
        RuntimeAction::ShrinkTimeline => change_timeline_limit(app, scheduler, false),
        RuntimeAction::Quit => unreachable!("quit is handled before action application"),
    }
    true
}

const TIMELINE_LIMIT_STEPS: [usize; 8] = [1, 10, 25, 50, 120, 250, 500, 1_000];

fn change_timeline_limit(app: &mut App, scheduler: &mut Scheduler, grow: bool) {
    let current = scheduler.timeline_limit().get();
    let next = if grow {
        TIMELINE_LIMIT_STEPS
            .into_iter()
            .find(|candidate| *candidate > current)
    } else {
        TIMELINE_LIMIT_STEPS
            .into_iter()
            .rev()
            .find(|candidate| *candidate < current)
    };
    let Some(next) = next.and_then(|rows| TimelineLimit::new(rows).ok()) else {
        return;
    };
    if !grow && app.recorder.data.is_none() {
        return;
    }

    scheduler.set_timeline_limit(next);
    if grow {
        if !scheduler.current_local_projection_pending() {
            let mode = if app.recorder.data.is_some() || scheduler.recorder_active() {
                RecorderMode::ReuseCurrent
            } else {
                RecorderMode::Refresh
            };
            scheduler.queue_recorder(mode);
        }
    } else {
        app.shrink_timeline_limit(next.get());
    }
}

fn queue_detail_refresh(app: &mut App, scheduler: &mut Scheduler) {
    let Some((plan_id, is_open)) = app.detail.plan().map_or_else(
        || {
            app.detail
                .target_plan_id
                .clone()
                .map(|plan_id| (plan_id, false))
        },
        |plan| Some((plan.raw_plan_id.clone(), plan.is_open)),
    ) else {
        return;
    };
    if is_open {
        scheduler.queue_recorder(RecorderMode::Refresh);
        if app.refresh_plan_detail() {
            drain_plan_intent(app, scheduler);
        }
    } else {
        app.detail.refresh_plan(plan_id.clone(), PlanBasis::Fresh);
        scheduler.queue_detail(PlanBasis::Fresh, plan_id);
    }
}

fn shutdown(
    terminal: &mut TerminalSession,
    app: &mut App,
    scheduler: &mut Scheduler,
    worker: &mut Option<RefreshWorker>,
    notice: &str,
) -> Result<()> {
    scheduler.clear();
    if worker.is_some() {
        app.runtime_notice = Some(notice.to_string());
        terminal
            .draw(|frame| render::draw(frame, app))
            .context("failed to draw collection cancellation state")?;
    }
    if let Some(mut active) = worker.take() {
        active.cancel_and_join();
    }
    app.runtime_notice = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::dashboard::{RecorderRefresh, RecorderRequest, StatusLocalSnapshot, scenarios};
    use crate::terminal::model::Tab;

    #[test]
    fn every_view_refreshes_the_single_recorder_domain() {
        for tab in Tab::ALL {
            let mut app = App::new(tab);
            let mut scheduler =
                Scheduler::new(std::time::Duration::from_secs(10), TimelineLimit::DEFAULT);
            assert!(apply_action(
                &mut app,
                &mut scheduler,
                RuntimeAction::Refresh
            ));
            assert!(scheduler.recorder_pending());
            assert!(matches!(
                scheduler.start_next().unwrap().kind,
                WorkKind::Recorder(_)
            ));
        }
    }

    #[test]
    fn timeline_limit_endpoints_and_plus_minus_controls_are_enforced() {
        assert_eq!(TIMELINE_LIMIT_STEPS.first(), Some(&1));
        assert_eq!(TIMELINE_LIMIT_STEPS.last(), Some(&1_000));
        assert!(TimelineLimit::new(0).is_err());
        assert!(TimelineLimit::new(1_001).is_err());

        let mut app = App::new(Tab::Timeline);
        let mut scheduler =
            Scheduler::new(std::time::Duration::from_secs(10), TimelineLimit::DEFAULT);
        assert!(apply_action(
            &mut app,
            &mut scheduler,
            RuntimeAction::GrowTimeline
        ));
        assert_eq!(scheduler.timeline_limit().get(), 250);
        let scheduled = scheduler.start_next().unwrap();
        assert_eq!(
            scheduled.kind,
            WorkKind::Recorder(RecorderRequest {
                mode: RecorderMode::Refresh,
                timeline_limit: TimelineLimit::new(250).unwrap(),
            })
        );

        let mut recorder = scenarios::recorder_snapshot();
        recorder.timeline_limit = 250;
        let status = scenarios::status_snapshot();
        assert!(app.accept_recorder_refresh(RecorderRefresh {
            status_local: StatusLocalSnapshot {
                epoch_id: recorder.epoch_id,
                observed_at_ms: status.observed_at_ms,
                repository: status.repository,
                work: status.work,
                loops: status.loops,
                errors: status.errors,
            },
            recorder,
        }));
        scheduler.complete(scheduled.generation, Instant::now());
        assert!(apply_action(
            &mut app,
            &mut scheduler,
            RuntimeAction::ShrinkTimeline
        ));
        assert_eq!(scheduler.timeline_limit().get(), 120);
        assert_eq!(app.recorder.data.as_ref().unwrap().timeline_limit, 120);
    }
}
