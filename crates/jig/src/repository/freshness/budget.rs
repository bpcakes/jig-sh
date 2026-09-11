use std::time::{Duration, Instant};

use jig_contract::freshness::{
    FreshnessCollectionLimit, FreshnessCollectionStats, FreshnessReason, FreshnessReasonCode,
    MAX_FRESHNESS_DIAGNOSTIC_BYTES,
};

pub(crate) const RECORDING_TIMEOUT_MS: u64 = 30_000;
pub(crate) const INSPECTION_TIMEOUT_MS: u64 = 2_000;

#[derive(Clone, Copy, Debug)]
pub(crate) struct CollectionLimits {
    pub(crate) timeout: Duration,
    pub(crate) entries: u64,
    pub(crate) bytes: u64,
    pub(crate) git_output: usize,
    pub(crate) targets: u64,
    pub(crate) edges: u64,
    pub(crate) directory_depth: usize,
}

impl CollectionLimits {
    pub(crate) fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout: timeout.min(Duration::from_millis(RECORDING_TIMEOUT_MS)),
            entries: 250_000,
            bytes: 512 * 1024 * 1024,
            git_output: 16 * 1024 * 1024,
            targets: 10_000,
            edges: 100_000,
            directory_depth: 128,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CollectionFailure {
    pub(crate) limit: Option<FreshnessCollectionLimit>,
    pub(crate) reason: FreshnessReason,
    pub(crate) message: String,
}

impl CollectionFailure {
    pub(crate) fn new(code: FreshnessReasonCode, message: &str) -> Self {
        Self {
            limit: (code == FreshnessReasonCode::CollectionLimit)
                .then_some(FreshnessCollectionLimit::Resource),
            reason: FreshnessReason {
                code,
                target: None,
                path: None,
            },
            message: bounded_text(message, MAX_FRESHNESS_DIAGNOSTIC_BYTES.min(1_000)),
        }
    }

    fn deadline(timeout_ms: u64) -> Self {
        let mut failure = Self::new(
            FreshnessReasonCode::CollectionLimit,
            &format!("freshness collection exceeded {timeout_ms} ms"),
        );
        failure.limit = Some(FreshnessCollectionLimit::Deadline);
        failure
    }

    pub(crate) fn at(mut self, path: &str) -> Self {
        // Unsupported long paths are never partially used as source authority.
        // The preview is deliberately bounded and carries no file contents.
        self.reason.path = Some(bounded_text(path, 512));
        self
    }
}

impl std::fmt::Display for CollectionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

fn bounded_text(value: &str, limit: usize) -> String {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

impl std::error::Error for CollectionFailure {}

pub(crate) type CollectionResult<T> = Result<T, CollectionFailure>;

pub(crate) struct CollectionBudget<'a> {
    pub(crate) limits: CollectionLimits,
    pub(crate) stats: FreshnessCollectionStats,
    started: Instant,
    elapsed_before: Duration,
    deadline: Instant,
    cancelled: &'a dyn Fn() -> bool,
}

impl<'a> CollectionBudget<'a> {
    pub(crate) fn new(limits: CollectionLimits, cancelled: &'a dyn Fn() -> bool) -> Self {
        let started = Instant::now();
        Self {
            limits,
            stats: FreshnessCollectionStats {
                timeout_ms: limits.timeout.as_millis() as u64,
                ..FreshnessCollectionStats::default()
            },
            started,
            elapsed_before: Duration::ZERO,
            deadline: started + limits.timeout,
            cancelled,
        }
    }

    /// Resume cumulative observation work after a target executed. No source or
    /// proof counters reset, and time spent running targets is not observation.
    pub(crate) fn resume(
        limits: CollectionLimits,
        cancelled: &'a dyn Fn() -> bool,
        stats: FreshnessCollectionStats,
    ) -> Self {
        let mut budget = Self::new(limits, cancelled);
        budget.elapsed_before = Duration::from_micros(stats.elapsed_us);
        budget.deadline = budget.started + limits.timeout.saturating_sub(budget.elapsed_before);
        budget.stats = stats;
        budget.stats.timeout_ms = limits.timeout.as_millis() as u64;
        budget
    }

    pub(crate) fn ensure_active(&self) -> CollectionResult<()> {
        if (self.cancelled)() {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::CollectionFailed,
                "freshness collection was cancelled",
            ));
        }
        if Instant::now() >= self.deadline {
            return Err(CollectionFailure::deadline(self.stats.timeout_ms));
        }
        if self.stats.discovered_entries > self.limits.entries
            || self.stats.content_bytes_read > self.limits.bytes
            || self.stats.targets > self.limits.targets
            || self.stats.dependency_edges > self.limits.edges
        {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::CollectionLimit,
                "freshness collection exhausted a shared entry, byte, target, or dependency limit",
            ));
        }
        Ok(())
    }

    pub(crate) fn stopped(&self) -> bool {
        (self.cancelled)() || Instant::now() >= self.deadline
    }

    pub(crate) fn remaining(&self) -> CollectionResult<Duration> {
        self.ensure_active()?;
        Ok(self.deadline.saturating_duration_since(Instant::now()))
    }

    pub(crate) fn entries(&mut self, count: u64) -> CollectionResult<()> {
        self.ensure_active()?;
        self.stats.discovered_entries = self.stats.discovered_entries.saturating_add(count);
        if self.stats.discovered_entries > self.limits.entries {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::CollectionLimit,
                "freshness collection exceeded the discovered-entry limit",
            ));
        }
        Ok(())
    }

    pub(crate) fn bytes(&mut self, count: u64) -> CollectionResult<()> {
        // Callers charge bytes already returned by a read, including a read
        // which crossed the deadline or observed cancellation.
        self.stats.content_bytes_read = self.stats.content_bytes_read.saturating_add(count);
        self.ensure_active()?;
        if self.stats.content_bytes_read > self.limits.bytes {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::CollectionLimit,
                "freshness collection exceeded the content-read limit",
            ));
        }
        Ok(())
    }

    pub(crate) fn target(&mut self) -> CollectionResult<()> {
        self.ensure_active()?;
        self.stats.targets = self.stats.targets.saturating_add(1);
        if self.stats.targets > self.limits.targets {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::CollectionLimit,
                "freshness collection exceeded the target/proof-node limit",
            ));
        }
        Ok(())
    }

    pub(crate) fn edges(&mut self, count: u64) -> CollectionResult<()> {
        self.ensure_active()?;
        self.stats.dependency_edges = self.stats.dependency_edges.saturating_add(count);
        if self.stats.dependency_edges > self.limits.edges {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::CollectionLimit,
                "freshness collection exceeded the dependency-edge limit",
            ));
        }
        Ok(())
    }

    pub(crate) fn finish_stats(&self) -> FreshnessCollectionStats {
        let mut stats = self.stats.clone();
        stats.elapsed_us = self
            .elapsed_before
            .saturating_add(self.started.elapsed())
            .as_micros()
            .min(u128::from(u64::MAX)) as u64;
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_limit_classification_records_the_actual_failure() {
        let deadline =
            CollectionBudget::new(CollectionLimits::with_timeout(Duration::ZERO), &|| false);
        assert_eq!(
            deadline.ensure_active().unwrap_err().limit,
            Some(FreshnessCollectionLimit::Deadline)
        );
        let mut limits = CollectionLimits::with_timeout(Duration::from_secs(30));
        limits.entries = 0;
        let mut resource = CollectionBudget::new(limits, &|| false);
        assert_eq!(
            resource.entries(1).unwrap_err().limit,
            Some(FreshnessCollectionLimit::Resource)
        );
    }

    #[test]
    fn resumed_recording_cannot_reset_an_exhausted_observation_allowance() {
        let stats = FreshnessCollectionStats {
            elapsed_us: RECORDING_TIMEOUT_MS * 1_000,
            content_bytes_read: 73,
            discovered_entries: 19,
            ..FreshnessCollectionStats::default()
        };
        let budget = CollectionBudget::resume(
            CollectionLimits::with_timeout(Duration::from_millis(RECORDING_TIMEOUT_MS)),
            &|| false,
            stats,
        );
        assert_eq!(
            budget.ensure_active().unwrap_err().reason.code,
            FreshnessReasonCode::CollectionLimit
        );
        let observed = budget.finish_stats();
        assert!(observed.elapsed_us >= RECORDING_TIMEOUT_MS * 1_000);
        assert_eq!(observed.content_bytes_read, 73);
        assert_eq!(observed.discovered_entries, 19);
    }

    #[test]
    fn already_read_bytes_are_counted_even_when_the_read_crosses_cancellation() {
        let cancelled = std::cell::Cell::new(false);
        let is_cancelled = || cancelled.get();
        let mut budget = CollectionBudget::new(
            CollectionLimits::with_timeout(Duration::from_secs(30)),
            &is_cancelled,
        );
        budget.ensure_active().unwrap();
        cancelled.set(true);
        assert!(budget.bytes(64).is_err());
        assert_eq!(budget.finish_stats().content_bytes_read, 64);
    }
}
