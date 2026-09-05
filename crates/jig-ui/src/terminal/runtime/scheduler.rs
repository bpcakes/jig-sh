use std::time::{Duration, Instant};

use crate::dashboard::{
    PlanBasis, RecorderMode, RecorderRequest, StatusPhase, StatusRequest, TimelineLimit,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum WorkKind {
    Recorder(RecorderRequest),
    Status(StatusRequest),
    Plan {
        basis: PlanBasis,
        plan_id: String,
        stale_retries: u8,
    },
}

impl WorkKind {
    const fn slot(&self) -> Slot {
        match self {
            Self::Recorder(_) => Slot::Recorder,
            Self::Status(_) => Slot::Status,
            Self::Plan { .. } => Slot::Detail,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ScheduledRequest {
    pub(super) generation: u64,
    pub(super) sequence: u64,
    pub(super) kind: WorkKind,
}

impl ScheduledRequest {
    pub(super) const fn resets_status_timer(&self) -> bool {
        matches!(self.kind, WorkKind::Status(_))
    }

    pub(super) const fn resets_local_timer(&self, published_local: bool) -> bool {
        matches!(self.kind, WorkKind::Recorder(_))
            || (published_local && matches!(self.kind, WorkKind::Status(_)))
    }
}

#[derive(Clone, Debug)]
struct Pending {
    sequence: u64,
    explicit: bool,
    kind: WorkKind,
}

#[derive(Clone, Debug)]
struct Active {
    request: ScheduledRequest,
    explicit: bool,
    status_phase: Option<StatusPhase>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Slot {
    Recorder,
    Status,
    Detail,
}

#[derive(Debug)]
pub(super) struct Scheduler {
    pending_recorder: Option<Pending>,
    pending_status: Option<Pending>,
    pending_detail: Option<Pending>,
    dispatch_override: Option<Slot>,
    active: Option<Active>,
    next_sequence: u64,
    next_generation: u64,
    local_interval: Duration,
    status_interval: Duration,
    local_deadline: Option<Instant>,
    status_deadline: Option<Instant>,
    timeline_limit: TimelineLimit,
}

impl Scheduler {
    pub(super) fn new(
        local_interval: Duration,
        status_interval: Duration,
        timeline_limit: TimelineLimit,
    ) -> Self {
        Self {
            pending_recorder: None,
            pending_status: None,
            pending_detail: None,
            dispatch_override: None,
            active: None,
            next_sequence: 1,
            next_generation: 1,
            local_interval,
            status_interval,
            local_deadline: None,
            status_deadline: None,
            timeline_limit,
        }
    }

    pub(super) fn queue_recorder(&mut self, mode: RecorderMode, explicit: bool) {
        self.queue(
            WorkKind::Recorder(RecorderRequest {
                mode,
                timeline_limit: self.timeline_limit,
            }),
            explicit,
        );
    }

    pub(super) fn queue_status(&mut self, explicit: bool) {
        self.queue(
            WorkKind::Status(StatusRequest {
                timeline_limit: self.timeline_limit,
            }),
            explicit,
        );
    }

    pub(super) const fn timeline_limit(&self) -> TimelineLimit {
        self.timeline_limit
    }

    pub(super) fn set_timeline_limit(&mut self, timeline_limit: TimelineLimit) {
        self.timeline_limit = timeline_limit;
        for pending in [self.pending_recorder.as_mut(), self.pending_status.as_mut()]
            .into_iter()
            .flatten()
        {
            match &mut pending.kind {
                WorkKind::Recorder(request) => request.timeline_limit = timeline_limit,
                WorkKind::Status(request) => request.timeline_limit = timeline_limit,
                WorkKind::Plan { .. } => unreachable!("primary slot contained detail work"),
            }
        }
    }

    pub(super) fn current_local_projection_pending(&self) -> bool {
        self.pending_recorder.is_some() || self.pending_status.is_some()
    }

    pub(super) fn primary_active(&self) -> bool {
        self.active.as_ref().is_some_and(|active| {
            matches!(
                active.request.kind,
                WorkKind::Recorder(_) | WorkKind::Status(_)
            )
        })
    }

    pub(super) fn queue_detail(&mut self, basis: PlanBasis, plan_id: String) {
        self.queue(
            WorkKind::Plan {
                basis,
                plan_id,
                stale_retries: 0,
            },
            true,
        );
    }

    pub(super) fn retry_stale_detail(
        &mut self,
        basis: PlanBasis,
        plan_id: String,
        stale_retries: u8,
    ) {
        self.queue(
            WorkKind::Plan {
                basis,
                plan_id,
                stale_retries,
            },
            true,
        );
    }

    fn queue(&mut self, kind: WorkKind, explicit: bool) {
        let slot = kind.slot();
        let sequence = self.allocate_sequence();
        let pending = self.pending_mut(slot);
        if let Some(pending) = pending {
            pending.kind = kind;
            pending.explicit |= explicit;
        } else {
            *pending = Some(Pending {
                sequence,
                explicit,
                kind,
            });
        }
    }

    pub(super) fn enqueue_due(&mut self, now: Instant) {
        let status_due = self.status_deadline.is_some_and(|deadline| now >= deadline);
        if status_due && self.pending_status.is_none() && !self.active_is(Slot::Status) {
            self.queue_status(false);
        }
        let status_will_publish_local =
            self.pending_status.is_some() || self.active_is(Slot::Status);
        if self.local_deadline.is_some_and(|deadline| now >= deadline)
            && self.pending_recorder.is_none()
            && !self.active_is(Slot::Recorder)
            && !status_will_publish_local
        {
            self.queue_recorder(RecorderMode::Refresh, false);
        }
    }

    pub(super) fn start_next(&mut self) -> Option<ScheduledRequest> {
        if self.active.is_some() {
            return None;
        }
        let slot = self.dispatch_override.take().or_else(|| {
            [Slot::Recorder, Slot::Status, Slot::Detail]
                .into_iter()
                .filter_map(|slot| self.pending(slot).map(|pending| (pending.sequence, slot)))
                .min_by_key(|(sequence, _)| *sequence)
                .map(|(_, slot)| slot)
        })?;
        let pending = self
            .pending_mut(slot)
            .take()
            .expect("selected pending slot");
        let request = ScheduledRequest {
            generation: self.allocate_generation(),
            sequence: pending.sequence,
            kind: pending.kind,
        };
        self.active = Some(Active {
            request: request.clone(),
            explicit: pending.explicit,
            status_phase: None,
        });
        Some(request)
    }

    pub(super) fn accept_status_phase(&mut self, generation: u64, phase: StatusPhase) -> bool {
        let Some(active) = &mut self.active else {
            return false;
        };
        if active.request.generation != generation
            || !matches!(active.request.kind, WorkKind::Status(_))
        {
            return false;
        }
        match (active.status_phase, phase) {
            (Some(StatusPhase::LocalEpoch), StatusPhase::Providers)
            | (Some(StatusPhase::Providers), StatusPhase::Providers)
            | (Some(StatusPhase::LocalEpoch), StatusPhase::LocalEpoch) => false,
            (None, _) | (Some(StatusPhase::Providers), StatusPhase::LocalEpoch) => {
                active.status_phase = Some(phase);
                true
            }
        }
    }

    pub(super) fn should_preempt_status(&self) -> bool {
        let Some(active) = &self.active else {
            return false;
        };
        matches!(active.request.kind, WorkKind::Status(_))
            && active.status_phase == Some(StatusPhase::Providers)
            && [self.pending_recorder.as_ref(), self.pending_detail.as_ref()]
                .into_iter()
                .flatten()
                .any(|pending| pending.explicit)
    }

    pub(super) fn preempt_status(&mut self) -> Option<ScheduledRequest> {
        if !self.should_preempt_status() {
            return None;
        }
        let active = self.active.take().expect("preemptible active status");
        self.dispatch_override = [Slot::Recorder, Slot::Detail]
            .into_iter()
            .filter_map(|slot| {
                self.pending(slot)
                    .filter(|pending| pending.explicit)
                    .map(|pending| (pending.sequence, slot))
            })
            .min_by_key(|(sequence, _)| *sequence)
            .map(|(_, slot)| slot);
        self.queue_with_sequence(
            active.request.kind.clone(),
            active.explicit,
            active.request.sequence,
        );
        Some(active.request)
    }

    pub(super) fn complete(
        &mut self,
        generation: u64,
        published_local: bool,
        now: Instant,
    ) -> Option<ScheduledRequest> {
        let active = self.active.as_ref()?;
        if active.request.generation != generation {
            return None;
        }
        let active = self.active.take().expect("checked active request");
        if active.request.resets_status_timer() {
            self.status_deadline = Some(now + self.status_interval);
        }
        if active.request.resets_local_timer(published_local) {
            self.local_deadline = Some(now + self.local_interval);
        }
        Some(active.request)
    }

    pub(super) fn is_active_generation(&self, generation: u64) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.request.generation == generation)
    }

    pub(super) fn status_published_local(&mut self) {
        if self
            .pending_recorder
            .as_ref()
            .is_some_and(|pending| !pending.explicit)
        {
            self.pending_recorder = None;
        }
    }

    pub(super) fn recorder_pending(&self) -> bool {
        self.pending_recorder.is_some()
    }

    pub(super) fn status_pending(&self) -> bool {
        self.pending_status.is_some()
    }

    pub(super) fn detail_pending(&self) -> bool {
        self.pending_detail.is_some()
    }

    pub(super) fn clear(&mut self) {
        self.pending_recorder = None;
        self.pending_status = None;
        self.pending_detail = None;
        self.dispatch_override = None;
        self.active = None;
    }

    #[cfg(test)]
    pub(super) const fn has_active(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn domain_active(&self, status_domain: bool) -> bool {
        self.active.as_ref().is_some_and(|active| {
            if status_domain {
                matches!(active.request.kind, WorkKind::Status(_))
            } else {
                matches!(active.request.kind, WorkKind::Recorder(_))
            }
        })
    }

    fn active_is(&self, slot: Slot) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.request.kind.slot() == slot)
    }

    fn pending(&self, slot: Slot) -> Option<&Pending> {
        match slot {
            Slot::Recorder => self.pending_recorder.as_ref(),
            Slot::Status => self.pending_status.as_ref(),
            Slot::Detail => self.pending_detail.as_ref(),
        }
    }

    fn pending_mut(&mut self, slot: Slot) -> &mut Option<Pending> {
        match slot {
            Slot::Recorder => &mut self.pending_recorder,
            Slot::Status => &mut self.pending_status,
            Slot::Detail => &mut self.pending_detail,
        }
    }

    fn queue_with_sequence(&mut self, mut kind: WorkKind, explicit: bool, sequence: u64) {
        match &mut kind {
            WorkKind::Recorder(request) => request.timeline_limit = self.timeline_limit,
            WorkKind::Status(request) => request.timeline_limit = self.timeline_limit,
            WorkKind::Plan { .. } => {}
        }
        let pending = self.pending_mut(kind.slot());
        match pending {
            Some(existing) => {
                existing.sequence = existing.sequence.min(sequence);
                existing.explicit |= explicit;
            }
            None => {
                *pending = Some(Pending {
                    sequence,
                    explicit,
                    kind,
                });
            }
        }
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
mod tests;
