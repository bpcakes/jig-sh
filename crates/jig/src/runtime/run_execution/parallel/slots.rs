use super::*;

/// Capacity shared by ordinary workers and admitted resource wave members.
#[derive(Clone)]
pub(in crate::runtime::run_execution) struct ExecutionSlots(Arc<AtomicUsize>);

pub(super) struct ExecutionSlot(Arc<AtomicUsize>);

impl ExecutionSlots {
    pub(in crate::runtime::run_execution) fn new() -> Self {
        Self(Arc::new(AtomicUsize::new(MAX_PARALLEL_LAYER_TARGETS)))
    }

    pub(super) fn is_full(&self) -> bool {
        self.0.load(Ordering::Acquire) == 0
    }

    pub(super) fn try_acquire(&self) -> Option<ExecutionSlot> {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |available| {
                available.checked_sub(1)
            })
            .ok()
            .map(|_| ExecutionSlot(Arc::clone(&self.0)))
    }
}

impl Drop for ExecutionSlot {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Release);
    }
}
