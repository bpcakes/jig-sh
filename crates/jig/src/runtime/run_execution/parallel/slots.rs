use super::*;

/// Capacity shared by ordinary workers and admitted resource wave members.
#[derive(Clone)]
pub(in crate::runtime::run_execution) struct ExecutionSlots {
    available: Arc<AtomicUsize>,
    preference: Arc<Mutex<ResourcePreference>>,
}

pub(super) struct ExecutionSlot(Arc<AtomicUsize>);

pub(super) struct AdmissionSnapshot {
    generation: u64,
    dispatching: bool,
}

#[derive(Default)]
struct ResourcePreference {
    demand: bool,
    wave_active: bool,
    dispatching: bool,
    generation: u64,
}

impl ExecutionSlots {
    pub(in crate::runtime::run_execution) fn new() -> Self {
        Self {
            available: Arc::new(AtomicUsize::new(MAX_PARALLEL_LAYER_TARGETS)),
            preference: Arc::new(Mutex::new(ResourcePreference::default())),
        }
    }

    pub(super) fn request_resource_admission(&self) {
        let mut preference = self.preference.lock().expect("resource preference lock");
        preference.generation = preference.generation.wrapping_add(1);
        preference.dispatching = true;
        if !preference.wave_active {
            preference.demand = true;
        }
    }

    pub(super) fn finish_resource_dispatch(&self) {
        self.preference
            .lock()
            .expect("resource preference lock")
            .dispatching = false;
    }

    pub(super) fn admission_snapshot(&self) -> AdmissionSnapshot {
        let preference = self.preference.lock().expect("resource preference lock");
        AdmissionSnapshot {
            generation: preference.generation,
            dispatching: preference.dispatching,
        }
    }

    pub(super) fn finish_admission(&self, snapshot: AdmissionSnapshot, waiting_for_slot: bool) {
        let mut preference = self.preference.lock().expect("resource preference lock");
        if waiting_for_slot {
            preference.demand = true;
        } else if !snapshot.dispatching
            && !preference.dispatching
            && preference.generation == snapshot.generation
        {
            // A dispatch already in flight at scan start may not have reached
            // the arrivals channel before this worker drained it.
            preference.demand = false;
        }
    }

    pub(super) fn begin_wave(&self) {
        let mut preference = self.preference.lock().expect("resource preference lock");
        preference.wave_active = true;
        preference.demand = false;
    }

    pub(super) fn end_wave(&self) {
        let mut preference = self.preference.lock().expect("resource preference lock");
        preference.wave_active = false;
        preference.demand = true;
    }

    pub(super) fn set_resource_demand(&self, demanded: bool) {
        self.preference
            .lock()
            .expect("resource preference lock")
            .demand = demanded;
    }

    pub(super) fn try_acquire(&self) -> Option<ExecutionSlot> {
        self.acquire_with_minimum(0)
    }

    pub(super) fn try_acquire_ordinary(&self) -> Option<ExecutionSlot> {
        let preference = self.preference.lock().expect("resource preference lock");
        self.acquire_with_minimum(usize::from(preference.demand))
    }

    fn acquire_with_minimum(&self, minimum: usize) -> Option<ExecutionSlot> {
        self.available
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |available| {
                (available > minimum).then(|| available - 1)
            })
            .ok()
            .map(|_| ExecutionSlot(Arc::clone(&self.available)))
    }
}

impl Drop for ExecutionSlot {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_reservation_survives_a_scan_that_missed_a_dispatch() {
        for dispatch_started_before_scan in [false, true] {
            let slots = ExecutionSlots::new();
            if dispatch_started_before_scan {
                slots.request_resource_admission();
            }
            let stale_scan = slots.admission_snapshot();
            if !dispatch_started_before_scan {
                slots.request_resource_admission();
            }
            slots.finish_resource_dispatch();
            slots.finish_admission(stale_scan, false);

            let held = (0..MAX_PARALLEL_LAYER_TARGETS - 1)
                .map(|_| slots.try_acquire_ordinary().expect("ordinary slot"))
                .collect::<Vec<_>>();
            assert!(slots.try_acquire_ordinary().is_none());

            slots.finish_admission(slots.admission_snapshot(), false);
            assert!(slots.try_acquire_ordinary().is_some());
            drop(held);
        }
    }
}
