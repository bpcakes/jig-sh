use std::time::{Duration, Instant};

use jig_dashboard::{RecorderMode, RecorderRequest, TimelineLimit};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ScheduledRequest {
    pub(super) generation: u64,
    pub(super) request: RecorderRequest,
}

#[derive(Debug)]
pub(super) struct Scheduler {
    pending: Option<RecorderRequest>,
    active: Option<ScheduledRequest>,
    next_generation: u64,
    refresh_interval: Duration,
    refresh_deadline: Option<Instant>,
    timeline_limit: TimelineLimit,
}

impl Scheduler {
    pub(super) fn new(refresh_interval: Duration, timeline_limit: TimelineLimit) -> Self {
        Self {
            pending: None,
            active: None,
            next_generation: 1,
            refresh_interval,
            refresh_deadline: None,
            timeline_limit,
        }
    }

    /// Queues one recorder collection. A newer request replaces a pending one.
    pub(super) fn queue_recorder(&mut self, mode: RecorderMode) {
        self.pending = Some(RecorderRequest {
            mode,
            timeline_limit: self.timeline_limit,
        });
    }

    pub(super) const fn timeline_limit(&self) -> TimelineLimit {
        self.timeline_limit
    }

    pub(super) fn set_timeline_limit(&mut self, timeline_limit: TimelineLimit) {
        self.timeline_limit = timeline_limit;
        if let Some(request) = &mut self.pending {
            request.timeline_limit = timeline_limit;
        }
    }

    pub(super) fn recorder_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(super) fn recorder_active(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn enqueue_due(&mut self, now: Instant) {
        if self
            .refresh_deadline
            .is_some_and(|deadline| now >= deadline)
            && self.pending.is_none()
            && self.active.is_none()
        {
            self.queue_recorder(RecorderMode::Refresh);
        }
    }

    pub(super) fn start_next(&mut self) -> Option<ScheduledRequest> {
        if self.active.is_some() {
            return None;
        }
        let request = ScheduledRequest {
            request: self.pending.take()?,
            generation: self.allocate_generation(),
        };
        self.active = Some(request.clone());
        Some(request)
    }

    pub(super) fn complete(&mut self, generation: u64, now: Instant) -> Option<ScheduledRequest> {
        if self.active.as_ref()?.generation != generation {
            return None;
        }
        self.refresh_deadline = Some(now + self.refresh_interval);
        self.active.take()
    }

    pub(super) fn is_active_generation(&self, generation: u64) -> bool {
        self.active
            .as_ref()
            .is_some_and(|request| request.generation == generation)
    }

    pub(super) fn clear(&mut self) {
        self.pending = None;
        self.active = None;
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
        assert_eq!(
            scheduler.start_next().unwrap().request.mode,
            RecorderMode::Refresh
        );
        assert!(scheduler.start_next().is_none());
    }
}
