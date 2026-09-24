use super::*;

/// Capacity shared by ordinary workers and admitted resource wave members.
#[derive(Clone)]
pub(in crate::runtime::run_execution) struct ExecutionSlots {
    available: Arc<AtomicUsize>,
    resource_demand: Arc<AtomicBool>,
}

pub(super) struct ExecutionSlot(Arc<AtomicUsize>);

impl ExecutionSlots {
    pub(in crate::runtime::run_execution) fn new() -> Self {
        Self {
            available: Arc::new(AtomicUsize::new(MAX_PARALLEL_LAYER_TARGETS)),
            resource_demand: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(super) fn set_resource_demand(&self, demanded: bool) {
        self.resource_demand.store(demanded, Ordering::Release);
    }

    pub(super) fn try_acquire(&self) -> Option<ExecutionSlot> {
        self.acquire_with_minimum(0)
    }

    pub(super) fn try_acquire_ordinary(&self) -> Option<ExecutionSlot> {
        self.acquire_with_minimum(usize::from(self.resource_demand.load(Ordering::Acquire)))
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
