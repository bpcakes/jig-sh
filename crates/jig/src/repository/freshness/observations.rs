//! Test-only physical-work counters. They never enter proof identity or output schemas.
use std::cell::Cell;
use std::time::Instant;

#[derive(Clone, Copy, Default, serde::Serialize)]
pub(crate) struct Metrics {
    pub(crate) original_index_scans: u64,
    pub(crate) original_index_us: u64,
    pub(crate) original_reads: u64,
    pub(crate) original_read_us: u64,
    pub(crate) source_collections: u64,
    pub(crate) source_collection_us: u64,
    pub(crate) identity_collections: u64,
    pub(crate) identity_collection_us: u64,
}

thread_local! {
    static METRICS: Cell<Metrics> = Cell::new(Metrics::default());
}

pub(crate) enum Phase {
    OriginalIndex,
    OriginalRead,
    Source,
    Identity,
}

pub(crate) struct Measurement {
    phase: Phase,
    started: Instant,
}

pub(crate) fn measure(phase: Phase) -> Measurement {
    Measurement {
        phase,
        started: Instant::now(),
    }
}

impl Drop for Measurement {
    fn drop(&mut self) {
        let elapsed = self.started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        let mut metrics = METRICS.get();
        let (count, time) = match self.phase {
            Phase::OriginalIndex => (
                &mut metrics.original_index_scans,
                &mut metrics.original_index_us,
            ),
            Phase::OriginalRead => (&mut metrics.original_reads, &mut metrics.original_read_us),
            Phase::Source => (
                &mut metrics.source_collections,
                &mut metrics.source_collection_us,
            ),
            Phase::Identity => (
                &mut metrics.identity_collections,
                &mut metrics.identity_collection_us,
            ),
        };
        *count += 1;
        *time = time.saturating_add(elapsed);
        METRICS.set(metrics);
    }
}
