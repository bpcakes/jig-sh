use std::sync::Arc;

use super::{
    App, DashboardOptions, DashboardSource, EVENT_POLL_INTERVAL, InitialTab, Result, RuntimeAction,
    Tab, TerminalSession, event, handle_event, render,
};
use crate::dashboard::{PlanBasis, PlanSnapshotResult, RecorderMode};
use anyhow::Context;

use super::scheduler::{Scheduler, WorkKind};
use super::worker::{RefreshResult, RefreshWorker, apply_refresh_result};

pub(super) fn run(
    terminal: &mut TerminalSession,
    source: impl DashboardSource + 'static,
    mut app: App,
    options: DashboardOptions,
    externally_cancelled: impl Fn() -> bool,
) -> Result<()> {
    let source: Arc<dyn DashboardSource> = Arc::new(source);
    let mut scheduler = Scheduler::new(
        options.local_refresh_interval,
        options.status_refresh_interval,
    );
    queue_initial(&mut scheduler, options.initial_tab);
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
        drain_phases(&mut scheduler, worker.as_ref());
        maybe_preempt(terminal, &mut app, &mut scheduler, &mut worker)?;
        dirty |= finish_worker(&mut app, &mut scheduler, &mut worker)?;
        drain_plan_intent(&mut app, &mut scheduler);
        scheduler.enqueue_due(std::time::Instant::now());
        sync_pending(&mut app, &scheduler);
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
            sync_pending(&mut app, &scheduler);
            maybe_preempt(terminal, &mut app, &mut scheduler, &mut worker)?;
        }
    }
}

fn queue_initial(scheduler: &mut Scheduler, initial_tab: InitialTab) {
    match initial_tab {
        InitialTab::Status => scheduler.queue_status(true),
        InitialTab::Work => scheduler.queue_recorder(RecorderMode::Refresh, true),
    }
}

fn drain_phases(scheduler: &mut Scheduler, worker: Option<&RefreshWorker>) {
    let Some(worker) = worker else {
        return;
    };
    while let Some((generation, phase)) = worker.try_phase() {
        scheduler.accept_status_phase(generation, phase);
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
    let status_request = matches!(&request.kind, WorkKind::Status(_));
    let detail_superseded =
        matches!(&request.kind, WorkKind::Plan { .. }) && scheduler.detail_pending();
    let published_local = if detail_superseded {
        false
    } else if let Some((basis, plan_id, stale_retries)) = stale_detail_retry(app, &request, &result)
    {
        app.detail.refresh_plan(plan_id.clone(), basis);
        scheduler.retry_stale_detail(basis, plan_id, stale_retries);
        false
    } else {
        apply_refresh_result(app, &request, result)
    };
    scheduler.complete(
        request.generation,
        published_local,
        std::time::Instant::now(),
    );
    if status_request && published_local {
        scheduler.status_published_local();
    }
    Ok(true)
}

fn stale_detail_retry(
    app: &App,
    request: &super::scheduler::ScheduledRequest,
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
    match request.kind {
        WorkKind::Status(_) => app.status.refreshing = true,
        WorkKind::Recorder(_) => app.recorder.refreshing = true,
        WorkKind::Plan { .. } => {}
    }
    *worker = Some(RefreshWorker::spawn(Arc::clone(source), request)?);
    Ok(true)
}

fn apply_action(app: &mut App, scheduler: &mut Scheduler, action: RuntimeAction) -> bool {
    match action {
        RuntimeAction::Ignore => return false,
        RuntimeAction::Redraw => {}
        RuntimeAction::TabChanged => {
            if !app.domain_has_data(app.tab) && !scheduler.domain_active(app.tab.is_status_domain())
            {
                queue_domain(scheduler, app.tab);
            }
        }
        RuntimeAction::Refresh => queue_domain(scheduler, app.tab),
        RuntimeAction::RefreshAll => {
            scheduler.queue_recorder(RecorderMode::Refresh, true);
            scheduler.queue_status(true);
        }
        RuntimeAction::DetailRequested => drain_plan_intent(app, scheduler),
        RuntimeAction::RefreshDetail => queue_detail_refresh(app, scheduler),
        RuntimeAction::Quit => unreachable!("quit is handled before action application"),
    }
    true
}

fn queue_domain(scheduler: &mut Scheduler, tab: Tab) {
    if tab.is_status_domain() {
        scheduler.queue_status(true);
    } else {
        scheduler.queue_recorder(RecorderMode::Refresh, true);
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
        scheduler.queue_recorder(RecorderMode::Refresh, true);
        if app.refresh_plan_detail() {
            drain_plan_intent(app, scheduler);
        }
    } else {
        app.detail.refresh_plan(plan_id.clone(), PlanBasis::Fresh);
        scheduler.queue_detail(PlanBasis::Fresh, plan_id);
    }
}

fn sync_pending(app: &mut App, scheduler: &Scheduler) {
    app.recorder.refresh_queued = scheduler.recorder_pending();
    app.status.refresh_queued = scheduler.status_pending();
}

fn maybe_preempt(
    terminal: &mut TerminalSession,
    app: &mut App,
    scheduler: &mut Scheduler,
    worker: &mut Option<RefreshWorker>,
) -> Result<()> {
    drain_phases(scheduler, worker.as_ref());
    if !scheduler.should_preempt_status() {
        return Ok(());
    }
    let Some(active) = worker.as_ref() else {
        return Ok(());
    };
    if !active.claim_provider_cancellation() {
        drain_phases(scheduler, worker.as_ref());
        return Ok(());
    }
    app.runtime_notice = Some("cancelling provider collection for local foreground work".into());
    terminal
        .draw(|frame| render::draw(frame, app))
        .context("failed to draw provider cancellation state")?;
    scheduler
        .preempt_status()
        .context("provider cancellation was claimed without preemptible scheduler work")?;
    if let Some(mut active) = worker.take() {
        active.cancel_and_join();
    }
    app.status.refreshing = false;
    app.runtime_notice = None;
    sync_pending(app, scheduler);
    Ok(())
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
    use super::super::scheduler::ScheduledRequest;
    use super::*;
    use crate::dashboard::{RecorderEpochId, RecorderRefresh, StatusLocalSnapshot};

    fn refresh_at_epoch(epoch: RecorderEpochId) -> RecorderRefresh {
        let mut recorder = crate::dashboard::scenarios::recorder_snapshot();
        recorder.epoch_id = epoch;
        let status = crate::dashboard::scenarios::status_snapshot();
        RecorderRefresh {
            recorder,
            status_local: StatusLocalSnapshot {
                epoch_id: epoch,
                observed_at_ms: status.observed_at_ms,
                repository: status.repository,
                work: status.work,
                loops: status.loops,
                errors: status.errors,
            },
        }
    }

    fn app_at_epoch(epoch: RecorderEpochId) -> App {
        let mut app = App::default();
        app.accept_recorder_refresh(refresh_at_epoch(epoch));
        app
    }

    fn detail_request(epoch: RecorderEpochId, stale_retries: u8) -> ScheduledRequest {
        ScheduledRequest {
            generation: 7,
            sequence: 3,
            kind: WorkKind::Plan {
                basis: PlanBasis::RecorderEpoch(epoch),
                plan_id: "plan_example".to_string(),
                stale_retries,
            },
        }
    }

    #[test]
    fn stale_detail_retries_once_against_the_newest_accepted_epoch() {
        let current = RecorderEpochId::new(2).unwrap();
        let mut app = app_at_epoch(current);
        app.detail.refresh_plan(
            "plan_example".to_string(),
            PlanBasis::RecorderEpoch(current),
        );
        let stale = Ok(RefreshResult::Plan(PlanSnapshotResult::StaleRecorderEpoch));

        assert_eq!(
            stale_detail_retry(&app, &detail_request(RecorderEpochId::FIRST, 0), &stale),
            Some((
                PlanBasis::RecorderEpoch(current),
                "plan_example".to_string(),
                1,
            ))
        );
        assert!(stale_detail_retry(&app, &detail_request(current, 1), &stale).is_none());
    }

    #[test]
    fn stale_detail_does_not_retry_after_the_operator_targets_another_plan() {
        let current = RecorderEpochId::new(2).unwrap();
        let mut app = app_at_epoch(current);
        app.detail.refresh_plan(
            "another_plan".to_string(),
            PlanBasis::RecorderEpoch(current),
        );
        let stale = Ok(RefreshResult::Plan(PlanSnapshotResult::StaleRecorderEpoch));

        assert!(
            stale_detail_retry(&app, &detail_request(RecorderEpochId::FIRST, 0), &stale).is_none()
        );
    }

    #[test]
    fn queued_detail_rebases_without_losing_its_queue_age() {
        let mut app = app_at_epoch(RecorderEpochId::FIRST);
        app.detail.refresh_plan(
            "plan_example".to_string(),
            PlanBasis::RecorderEpoch(RecorderEpochId::FIRST),
        );
        let mut scheduler = Scheduler::new(
            std::time::Duration::from_secs(10),
            std::time::Duration::from_secs(30),
        );
        scheduler.queue_detail(
            PlanBasis::RecorderEpoch(RecorderEpochId::FIRST),
            "plan_example".to_string(),
        );

        let current = RecorderEpochId::new(2).unwrap();
        app.accept_recorder_refresh(refresh_at_epoch(current));
        drain_plan_intent(&mut app, &mut scheduler);

        let detail = scheduler.start_next().unwrap();
        assert_eq!(detail.sequence, 1);
        assert!(matches!(
            detail.kind,
            WorkKind::Plan {
                basis: PlanBasis::RecorderEpoch(epoch),
                plan_id,
                stale_retries: 0,
            } if epoch == current && plan_id == "plan_example"
        ));
    }

    #[test]
    fn queued_fresh_detail_is_not_rebased_by_an_older_domain_completion() {
        let mut app = app_at_epoch(RecorderEpochId::FIRST);
        app.detail
            .refresh_plan("plan_example".to_string(), PlanBasis::Fresh);
        let mut scheduler = Scheduler::new(
            std::time::Duration::from_secs(10),
            std::time::Duration::from_secs(30),
        );
        scheduler.queue_detail(PlanBasis::Fresh, "plan_example".to_string());

        app.accept_recorder_refresh(refresh_at_epoch(RecorderEpochId::new(2).unwrap()));
        drain_plan_intent(&mut app, &mut scheduler);

        assert!(matches!(
            scheduler.start_next().unwrap().kind,
            WorkKind::Plan {
                basis: PlanBasis::Fresh,
                ..
            }
        ));
    }
}
