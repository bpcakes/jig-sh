use std::time::{Duration, Instant};

use crate::dashboard::{PlanBasis, RecorderMode, RecorderRequest, TimelineLimit};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum WorkKind {
    Recorder(RecorderRequest),
    Plan {
        basis: PlanBasis,
        plan_id: String,
        stale_retries: u8,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ScheduledRequest {
    pub(super) generation: u64,
    pub(super) sequence: u64,
    pub(super) kind: WorkKind,
}

#[derive(Clone, Debug)]
struct Pending {
    sequence: u64,
    kind: WorkKind,
}

#[derive(Debug)]
pub(super) struct Scheduler {
    pending_recorder: Option<Pending>,
    pending_detail: Option<Pending>,
    active: Option<ScheduledRequest>,
    next_sequence: u64,
    next_generation: u64,
    refresh_interval: Duration,
    refresh_deadline: Option<Instant>,
    timeline_limit: TimelineLimit,
}

impl Scheduler {
    pub(super) fn new(refresh_interval: Duration, timeline_limit: TimelineLimit) -> Self {
        Self {
            pending_recorder: None,
            pending_detail: None,
            active: None,
            next_sequence: 1,
            next_generation: 1,
            refresh_interval,
            refresh_deadline: None,
            timeline_limit,
        }
    }

    pub(super) fn queue_recorder(&mut self, mode: RecorderMode) {
        let kind = WorkKind::Recorder(RecorderRequest {
            mode,
            timeline_limit: self.timeline_limit,
        });
        let sequence = self.allocate_sequence();
        if let Some(pending) = &mut self.pending_recorder {
            pending.kind = kind;
        } else {
            self.pending_recorder = Some(Pending { sequence, kind });
        }
    }

    pub(super) fn queue_detail(&mut self, basis: PlanBasis, plan_id: String) {
        self.queue_detail_with_retry(basis, plan_id, 0);
    }

    pub(super) fn retry_stale_detail(
        &mut self,
        basis: PlanBasis,
        plan_id: String,
        stale_retries: u8,
    ) {
        self.queue_detail_with_retry(basis, plan_id, stale_retries);
    }

    fn queue_detail_with_retry(&mut self, basis: PlanBasis, plan_id: String, stale_retries: u8) {
        let kind = WorkKind::Plan {
            basis,
            plan_id,
            stale_retries,
        };
        let sequence = self.allocate_sequence();
        if let Some(pending) = &mut self.pending_detail {
            pending.kind = kind;
        } else {
            self.pending_detail = Some(Pending { sequence, kind });
        }
    }

    pub(super) const fn timeline_limit(&self) -> TimelineLimit {
        self.timeline_limit
    }

    pub(super) fn set_timeline_limit(&mut self, timeline_limit: TimelineLimit) {
        self.timeline_limit = timeline_limit;
        if let Some(Pending {
            kind: WorkKind::Recorder(request),
            ..
        }) = &mut self.pending_recorder
        {
            request.timeline_limit = timeline_limit;
        }
    }

    pub(super) fn current_local_projection_pending(&self) -> bool {
        self.pending_recorder.is_some()
    }

    pub(super) fn recorder_active(&self) -> bool {
        matches!(
            self.active.as_ref().map(|request| &request.kind),
            Some(WorkKind::Recorder(_))
        )
    }

    pub(super) fn enqueue_due(&mut self, now: Instant) {
        if self
            .refresh_deadline
            .is_some_and(|deadline| now >= deadline)
            && self.pending_recorder.is_none()
            && !self.recorder_active()
        {
            self.queue_recorder(RecorderMode::Refresh);
        }
    }

    pub(super) fn start_next(&mut self) -> Option<ScheduledRequest> {
        if self.active.is_some() {
            return None;
        }
        let pending = match (&self.pending_recorder, &self.pending_detail) {
            (Some(recorder), Some(detail)) if recorder.sequence <= detail.sequence => {
                self.pending_recorder.take()
            }
            (Some(_), Some(_)) => self.pending_detail.take(),
            (Some(_), None) => self.pending_recorder.take(),
            (None, Some(_)) => self.pending_detail.take(),
            (None, None) => None,
        }?;
        let request = ScheduledRequest {
            generation: self.allocate_generation(),
            sequence: pending.sequence,
            kind: pending.kind,
        };
        self.active = Some(request.clone());
        Some(request)
    }

    pub(super) fn complete(&mut self, generation: u64, now: Instant) -> Option<ScheduledRequest> {
        if self.active.as_ref()?.generation != generation {
            return None;
        }
        let active = self.active.take();
        if active
            .as_ref()
            .is_some_and(|request| matches!(request.kind, WorkKind::Recorder(_)))
        {
            self.refresh_deadline = Some(now + self.refresh_interval);
        }
        active
    }

    pub(super) fn is_active_generation(&self, generation: u64) -> bool {
        self.active
            .as_ref()
            .is_some_and(|request| request.generation == generation)
    }

    #[cfg(test)]
    pub(super) fn recorder_pending(&self) -> bool {
        self.pending_recorder.is_some()
    }

    pub(super) fn detail_pending(&self) -> bool {
        self.pending_detail.is_some()
    }

    pub(super) fn clear(&mut self) {
        self.pending_recorder = None;
        self.pending_detail = None;
        self.active = None;
    }

    fn allocate_sequence(&mut self) -> u64 {
        let value = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        value
    }

    fn allocate_generation(&mut self) -> u64 {
        let value = self.next_generation;
        self.next_generation = self.next_generation.saturating_add(1);
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_refresh_is_completion_relative_and_single_domain() {
        let start = Instant::now();
        let mut scheduler = Scheduler::new(Duration::from_secs(10), TimelineLimit::DEFAULT);
        scheduler.queue_recorder(RecorderMode::Refresh);
        let request = scheduler.start_next().unwrap();
        scheduler.complete(request.generation, start);
        scheduler.enqueue_due(start + Duration::from_secs(9));
        assert!(!scheduler.recorder_pending());
        scheduler.enqueue_due(start + Duration::from_secs(10));
        assert!(scheduler.recorder_pending());
    }

    #[test]
    fn pending_recorder_refresh_coalesces() {
        let mut scheduler = Scheduler::new(Duration::from_secs(10), TimelineLimit::DEFAULT);
        scheduler.queue_recorder(RecorderMode::ReuseCurrent);
        scheduler.queue_recorder(RecorderMode::Refresh);
        assert!(matches!(
            scheduler.start_next().unwrap().kind,
            WorkKind::Recorder(RecorderRequest {
                mode: RecorderMode::Refresh,
                ..
            })
        ));
    }
}
