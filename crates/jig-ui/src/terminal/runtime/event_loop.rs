use std::sync::Arc;

use super::{
    App, DashboardOptions, DashboardSource, EVENT_POLL_INTERVAL, Result, RuntimeAction,
    TerminalSession, event, handle_event, render,
};
use crate::dashboard::{RecorderMode, TimelineLimit};
use anyhow::Context;

use super::scheduler::{ScheduledRequest, Scheduler};
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
    if request.request.timeline_limit == scheduler.timeline_limit() {
        apply_refresh_result(app, &request, result);
    } else {
        apply_outdated_projection(app, scheduler, &request, result);
    }
    scheduler.complete(request.generation, std::time::Instant::now());
    Ok(true)
}

fn apply_outdated_projection(
    app: &mut App,
    scheduler: &mut Scheduler,
    request: &ScheduledRequest,
    result: RefreshResult,
) {
    match result {
        Ok(refresh) if refresh.recorder.timeline_limit == request.request.timeline_limit.get() => {
            app.recorder.refreshing = false;
        }
        result => {
            apply_refresh_result(app, request, result);
        }
    }
    if !scheduler.recorder_pending() {
        scheduler.queue_recorder(RecorderMode::ReuseCurrent);
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
    app.recorder.refreshing = true;
    *worker = Some(RefreshWorker::spawn(Arc::clone(source), request)?);
    Ok(true)
}

fn apply_action(app: &mut App, scheduler: &mut Scheduler, action: RuntimeAction) -> bool {
    match action {
        RuntimeAction::Ignore => return false,
        RuntimeAction::Redraw | RuntimeAction::TabChanged => {}
        RuntimeAction::Refresh => scheduler.queue_recorder(RecorderMode::Refresh),
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
        if !scheduler.recorder_pending() {
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
            assert_eq!(
                scheduler.start_next().unwrap().request.mode,
                RecorderMode::Refresh
            );
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
            scheduled.request,
            RecorderRequest {
                mode: RecorderMode::Refresh,
                timeline_limit: TimelineLimit::new(250).unwrap(),
            }
        );

        let mut recorder = scenarios::recorder_snapshot();
        recorder.timeline_limit = 250;
        let status = scenarios::status_snapshot();
        assert!(app.accept_recorder_refresh(RecorderRefresh {
            status_local: StatusLocalSnapshot {
                epoch_id: recorder.epoch_id,
                observed_at_ms: status.observed_at_ms,
                repository: status.repository,
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
