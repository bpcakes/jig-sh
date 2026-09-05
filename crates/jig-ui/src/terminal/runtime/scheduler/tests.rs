use super::*;

fn scheduler() -> (Instant, Scheduler) {
    let now = Instant::now();
    (
        now,
        Scheduler::new(Duration::from_secs(10), Duration::from_secs(30)),
    )
}

#[test]
fn coalescing_preserves_age_and_replaces_detail_target() {
    let (_, mut scheduler) = scheduler();
    scheduler.queue_status(true);
    scheduler.queue_detail(
        PlanBasis::RecorderEpoch(crate::dashboard::RecorderEpochId::FIRST),
        "old".into(),
    );
    scheduler.queue_detail(PlanBasis::Fresh, "new".into());
    scheduler.queue_recorder(RecorderMode::Refresh, true);

    let status = scheduler.start_next().unwrap();
    assert!(matches!(status.kind, WorkKind::Status(_)));
    scheduler.complete(status.generation, true, Instant::now());
    let detail = scheduler.start_next().unwrap();
    assert_eq!(detail.sequence, 2);
    assert!(matches!(
        detail.kind,
        WorkKind::Plan {
            plan_id,
            basis: PlanBasis::Fresh,
            stale_retries: 0,
        } if plan_id == "new"
    ));
}

#[test]
fn a_new_same_plan_intent_remains_pending_after_the_active_generation() {
    let (_, mut scheduler) = scheduler();
    scheduler.queue_detail(PlanBasis::Fresh, "plan_example".into());
    let first = scheduler.start_next().unwrap();
    scheduler.queue_detail(PlanBasis::Fresh, "plan_example".into());
    assert!(scheduler.detail_pending());

    scheduler.complete(first.generation, false, Instant::now());
    let second = scheduler.start_next().unwrap();
    assert_ne!(first.generation, second.generation);
    assert!(matches!(
        second.kind,
        WorkKind::Plan { plan_id, .. } if plan_id == "plan_example"
    ));
}

#[test]
fn provider_phase_can_be_preempted_and_requeued_at_original_age() {
    let (_, mut scheduler) = scheduler();
    scheduler.queue_status(true);
    let status = scheduler.start_next().unwrap();
    scheduler.accept_status_phase(status.generation, StatusPhase::Providers);
    scheduler.queue_recorder(RecorderMode::Refresh, true);
    assert_eq!(scheduler.preempt_status(), Some(status));
    let local = scheduler.start_next().unwrap();
    assert!(matches!(local.kind, WorkKind::Recorder(_)));
    scheduler.complete(local.generation, true, Instant::now());
    assert!(matches!(
        scheduler.start_next().unwrap().kind,
        WorkKind::Status(_)
    ));
}

#[test]
fn local_epoch_and_non_status_work_are_never_preempted() {
    let (_, mut scheduler) = scheduler();
    scheduler.queue_status(true);
    let status = scheduler.start_next().unwrap();
    scheduler.accept_status_phase(status.generation, StatusPhase::LocalEpoch);
    scheduler.queue_detail(PlanBasis::Fresh, "plan".into());
    assert!(!scheduler.should_preempt_status());
    assert!(scheduler.preempt_status().is_none());
}

#[test]
fn automatic_local_work_never_preempts_provider_collection() {
    let (now, mut scheduler) = scheduler();
    scheduler.queue_recorder(RecorderMode::Refresh, true);
    let local = scheduler.start_next().unwrap();
    scheduler.complete(local.generation, true, now);
    scheduler.queue_status(true);
    let status = scheduler.start_next().unwrap();
    scheduler.accept_status_phase(status.generation, StatusPhase::Providers);
    scheduler.enqueue_due(now + Duration::from_secs(10));
    assert!(scheduler.pending_recorder.is_none());
    assert!(!scheduler.should_preempt_status());
}

#[test]
fn the_scheduler_never_dispatches_a_second_active_request() {
    let (_, mut scheduler) = scheduler();
    scheduler.queue_status(true);
    scheduler.queue_recorder(RecorderMode::Refresh, true);
    let first = scheduler.start_next().unwrap();
    assert!(scheduler.start_next().is_none());
    scheduler.complete(first.generation, true, Instant::now());
    assert!(scheduler.start_next().is_some());
}

#[test]
fn stale_phases_and_completions_cannot_change_active_work() {
    let (_, mut scheduler) = scheduler();
    scheduler.queue_status(true);
    let status = scheduler.start_next().unwrap();
    assert!(!scheduler.accept_status_phase(status.generation + 1, StatusPhase::Providers));
    assert!(
        scheduler
            .complete(status.generation + 1, true, Instant::now())
            .is_none()
    );
    assert!(scheduler.has_active());
}

#[test]
fn status_phase_cannot_regress_to_a_preemptible_phase() {
    let (_, mut scheduler) = scheduler();
    scheduler.queue_status(true);
    let status = scheduler.start_next().unwrap();
    assert!(scheduler.accept_status_phase(status.generation, StatusPhase::LocalEpoch));
    assert!(!scheduler.accept_status_phase(status.generation, StatusPhase::Providers));
    scheduler.queue_recorder(RecorderMode::Refresh, true);
    assert!(!scheduler.should_preempt_status());
}

#[test]
fn automatic_work_is_completion_relative_and_does_not_duplicate_pending() {
    let (now, mut scheduler) = scheduler();
    scheduler.queue_recorder(RecorderMode::Refresh, true);
    let initial_local = scheduler.start_next().unwrap();
    scheduler.complete(initial_local.generation, true, now);
    scheduler.enqueue_due(now + Duration::from_secs(31));
    assert!(scheduler.pending_recorder.is_some());
    scheduler.enqueue_due(now + Duration::from_secs(60));
    let local = scheduler.start_next().unwrap();
    scheduler.complete(local.generation, true, now + Duration::from_secs(60));
    scheduler.enqueue_due(now + Duration::from_secs(69));
    assert!(scheduler.pending_recorder.is_none());
    scheduler.enqueue_due(now + Duration::from_secs(70));
    assert!(scheduler.pending_recorder.is_some());
}

#[test]
fn timers_stay_disarmed_until_the_domain_first_completes() {
    let (now, mut scheduler) = scheduler();
    scheduler.enqueue_due(now + Duration::from_secs(3_600));
    assert!(scheduler.pending_recorder.is_none());
    assert!(scheduler.pending_status.is_none());

    scheduler.queue_recorder(RecorderMode::Refresh, true);
    let local = scheduler.start_next().unwrap();
    scheduler.complete(local.generation, true, now);
    scheduler.enqueue_due(now + Duration::from_secs(10));
    assert!(scheduler.pending_recorder.is_some());
    assert!(scheduler.pending_status.is_none());
}

#[test]
fn due_status_suppresses_redundant_automatic_local_until_publication_is_known() {
    let (now, mut scheduler) = scheduler();
    scheduler.queue_status(true);
    let status = scheduler.start_next().unwrap();
    scheduler.complete(status.generation, true, now);
    scheduler.enqueue_due(now + Duration::from_secs(30));

    assert!(scheduler.pending_status.is_some());
    assert!(scheduler.pending_recorder.is_none());
    let status = scheduler.start_next().unwrap();
    scheduler.complete(status.generation, false, now + Duration::from_secs(30));
    scheduler.enqueue_due(now + Duration::from_secs(30));
    assert!(scheduler.pending_recorder.is_some());
}

#[test]
fn quit_clear_discards_every_pending_and_active_request() {
    let (_, mut scheduler) = scheduler();
    scheduler.queue_status(true);
    scheduler.start_next();
    scheduler.queue_recorder(RecorderMode::Refresh, true);
    scheduler.queue_detail(PlanBasis::Fresh, "plan".into());
    scheduler.clear();
    assert!(!scheduler.has_active());
    assert!(scheduler.pending_recorder.is_none());
    assert!(scheduler.pending_status.is_none());
    assert!(scheduler.pending_detail.is_none());
}

#[test]
fn status_publication_only_discards_automatic_local_work() {
    let (_, mut scheduler) = scheduler();
    scheduler.queue_recorder(RecorderMode::Refresh, false);
    scheduler.status_published_local();
    assert!(scheduler.pending_recorder.is_none());

    scheduler.queue_recorder(RecorderMode::Refresh, true);
    scheduler.status_published_local();
    assert!(scheduler.pending_recorder.is_some());
}
