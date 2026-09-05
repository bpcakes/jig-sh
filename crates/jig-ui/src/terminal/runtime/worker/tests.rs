use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use super::*;
use crate::dashboard::{
    PlanBasis, PlanSnapshotResult, RecorderMode, RecorderRequest, StatusPhase, StatusRequest,
};
use crate::terminal::runtime::scheduler::Scheduler;

struct ControlledSource {
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl DashboardSource for ControlledSource {
    fn recorder(
        &self,
        _request: RecorderRequest,
        _cancelled: &dyn Fn() -> bool,
    ) -> Result<RecorderRefresh, SourceError> {
        self.events.lock().unwrap().push("recorder-start");
        let status = crate::dashboard::scenarios::status_snapshot();
        Ok(RecorderRefresh {
            recorder: crate::dashboard::scenarios::recorder_snapshot(),
            status_local: crate::dashboard::StatusLocalSnapshot {
                epoch_id: crate::dashboard::RecorderEpochId::FIRST,
                observed_at_ms: status.observed_at_ms,
                repository: status.repository,
                work: status.work,
                loops: status.loops,
                errors: status.errors,
            },
        })
    }

    fn status(
        &self,
        _request: StatusRequest,
        phase_changed: &dyn Fn(StatusPhase),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<StatusRefresh, SourceError> {
        self.events.lock().unwrap().push("status-start");
        phase_changed(StatusPhase::Providers);
        while !cancelled() {
            thread::sleep(Duration::from_millis(2));
        }
        self.events.lock().unwrap().push("status-cancelled");
        Err(SourceError::Cancelled)
    }

    fn plan(
        &self,
        _basis: PlanBasis,
        _plan_id: String,
        _cancelled: &dyn Fn() -> bool,
    ) -> Result<PlanSnapshotResult, SourceError> {
        Ok(PlanSnapshotResult::NotFound)
    }
}

fn wait_for_phase(worker: &RefreshWorker) -> (u64, StatusPhase) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(phase) = worker.try_phase() {
            return phase;
        }
        assert!(
            Instant::now() < deadline,
            "worker did not announce its phase"
        );
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn phase_events_are_generation_tagged_and_preemption_joins_before_local_start() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let source: Arc<dyn DashboardSource> = Arc::new(ControlledSource {
        events: Arc::clone(&events),
    });
    let mut scheduler = Scheduler::new(Duration::from_secs(10), Duration::from_secs(30));
    scheduler.queue_status(true);
    let status = scheduler.start_next().unwrap();
    let mut worker = RefreshWorker::spawn(Arc::clone(&source), status.clone()).unwrap();
    let (generation, phase) = wait_for_phase(&worker);
    assert_eq!(generation, status.generation);
    assert!(scheduler.accept_status_phase(generation, phase));

    scheduler.queue_recorder(RecorderMode::Refresh, true);
    assert!(worker.claim_provider_cancellation());
    assert_eq!(scheduler.preempt_status(), Some(status));
    worker.cancel_and_join();
    let local = scheduler.start_next().unwrap();
    let mut local_worker = RefreshWorker::spawn(source, local).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while local_worker.try_finish().is_none() {
        assert!(Instant::now() < deadline, "recorder worker did not finish");
        thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(
        *events.lock().unwrap(),
        ["status-start", "status-cancelled", "recorder-start"]
    );
}

struct TransitionSource {
    enter_local: Mutex<mpsc::Receiver<()>>,
    local_announced: mpsc::Sender<()>,
    local_started: Arc<AtomicBool>,
}

impl DashboardSource for TransitionSource {
    fn recorder(
        &self,
        _request: RecorderRequest,
        _cancelled: &dyn Fn() -> bool,
    ) -> Result<RecorderRefresh, SourceError> {
        unreachable!()
    }

    fn status(
        &self,
        _request: StatusRequest,
        phase_changed: &dyn Fn(StatusPhase),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<StatusRefresh, SourceError> {
        phase_changed(StatusPhase::Providers);
        self.enter_local.lock().unwrap().recv().unwrap();
        phase_changed(StatusPhase::LocalEpoch);
        if !cancelled() {
            self.local_started.store(true, Ordering::SeqCst);
        }
        self.local_announced.send(()).unwrap();
        while !cancelled() {
            thread::sleep(Duration::from_millis(2));
        }
        Err(SourceError::Cancelled)
    }

    fn plan(
        &self,
        _basis: PlanBasis,
        _plan_id: String,
        _cancelled: &dyn Fn() -> bool,
    ) -> Result<PlanSnapshotResult, SourceError> {
        unreachable!()
    }
}

#[test]
fn provider_cancellation_cannot_be_claimed_after_local_epoch_transition_wins() {
    let (enter_local, enter_local_rx) = mpsc::channel();
    let (local_announced, local_announced_rx) = mpsc::channel();
    let local_started = Arc::new(AtomicBool::new(false));
    let source = Arc::new(TransitionSource {
        enter_local: Mutex::new(enter_local_rx),
        local_announced,
        local_started: Arc::clone(&local_started),
    });
    let request = ScheduledRequest {
        generation: 1,
        sequence: 1,
        kind: WorkKind::Status(StatusRequest {
            timeline_limit: crate::dashboard::TimelineLimit::DEFAULT,
        }),
    };
    let mut worker = RefreshWorker::spawn(source, request).unwrap();
    assert_eq!(wait_for_phase(&worker).1, StatusPhase::Providers);
    enter_local.send(()).unwrap();
    local_announced_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();

    assert!(!worker.claim_provider_cancellation());
    assert!(local_started.load(Ordering::SeqCst));
    worker.cancel_and_join();
}

#[test]
fn provider_cancellation_claim_is_visible_before_local_work_can_start() {
    let (enter_local, enter_local_rx) = mpsc::channel();
    let (local_announced, local_announced_rx) = mpsc::channel();
    let local_started = Arc::new(AtomicBool::new(false));
    let source = Arc::new(TransitionSource {
        enter_local: Mutex::new(enter_local_rx),
        local_announced,
        local_started: Arc::clone(&local_started),
    });
    let request = ScheduledRequest {
        generation: 1,
        sequence: 1,
        kind: WorkKind::Status(StatusRequest {
            timeline_limit: crate::dashboard::TimelineLimit::DEFAULT,
        }),
    };
    let mut worker = RefreshWorker::spawn(source, request).unwrap();
    assert_eq!(wait_for_phase(&worker).1, StatusPhase::Providers);
    assert!(worker.claim_provider_cancellation());
    enter_local.send(()).unwrap();
    local_announced_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();

    assert!(!local_started.load(Ordering::SeqCst));
    worker.cancel_and_join();
}

struct PanicSource;

impl DashboardSource for PanicSource {
    fn recorder(
        &self,
        _request: RecorderRequest,
        _cancelled: &dyn Fn() -> bool,
    ) -> Result<RecorderRefresh, SourceError> {
        panic!("controlled recorder panic")
    }

    fn status(
        &self,
        _request: StatusRequest,
        _phase_changed: &dyn Fn(StatusPhase),
        _cancelled: &dyn Fn() -> bool,
    ) -> Result<StatusRefresh, SourceError> {
        unreachable!()
    }

    fn plan(
        &self,
        _basis: PlanBasis,
        _plan_id: String,
        _cancelled: &dyn Fn() -> bool,
    ) -> Result<PlanSnapshotResult, SourceError> {
        unreachable!()
    }
}

#[test]
fn source_panics_are_joined_and_reported_as_typed_internal_errors() {
    let request = ScheduledRequest {
        generation: 9,
        sequence: 1,
        kind: WorkKind::Recorder(RecorderRequest {
            mode: RecorderMode::Refresh,
            timeline_limit: crate::dashboard::TimelineLimit::DEFAULT,
        }),
    };
    let mut worker = RefreshWorker::spawn(Arc::new(PanicSource), request).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let (_, result) = loop {
        if let Some(result) = worker.try_finish() {
            break result;
        }
        assert!(Instant::now() < deadline, "panic result was not joined");
        thread::sleep(Duration::from_millis(2));
    };
    assert!(matches!(
        result,
        Err(SourceError::InternalContract { message }) if message.contains("worker panicked")
    ));
}

#[test]
fn mismatched_status_payload_is_a_status_error_and_cannot_publish_local_data() {
    let mut app = App::default();
    app.status.refreshing = true;
    let request = ScheduledRequest {
        generation: 1,
        sequence: 1,
        kind: WorkKind::Status(StatusRequest {
            timeline_limit: crate::dashboard::TimelineLimit::DEFAULT,
        }),
    };
    let status = crate::dashboard::scenarios::status_snapshot();
    apply_refresh_result(
        &mut app,
        &request,
        Ok(RefreshResult::Recorder(RecorderRefresh {
            recorder: crate::dashboard::scenarios::recorder_snapshot(),
            status_local: crate::dashboard::StatusLocalSnapshot {
                epoch_id: crate::dashboard::RecorderEpochId::FIRST,
                observed_at_ms: status.observed_at_ms,
                repository: status.repository,
                work: status.work,
                loops: status.loops,
                errors: status.errors,
            },
        })),
    );

    assert!(app.recorder.data.is_none());
    assert_eq!(
        app.status.error.as_deref(),
        Some("dashboard worker returned mismatched data for a status request")
    );
    assert!(!app.status.refreshing);
}

#[test]
fn invalid_status_recorder_projection_is_not_reported_as_local_publication() {
    let mut app = App::default();
    let request = ScheduledRequest {
        generation: 1,
        sequence: 1,
        kind: WorkKind::Status(StatusRequest {
            timeline_limit: crate::dashboard::TimelineLimit::DEFAULT,
        }),
    };
    let mut recorder = crate::dashboard::scenarios::recorder_snapshot();
    recorder.schema_version += 1;

    let published_local = apply_refresh_result(
        &mut app,
        &request,
        Ok(RefreshResult::Status(StatusRefresh {
            status: crate::dashboard::scenarios::status_snapshot(),
            recorder,
        })),
    );

    assert!(!published_local);
    assert!(app.recorder.data.is_none());
    assert!(app.status.data.is_some());
}
