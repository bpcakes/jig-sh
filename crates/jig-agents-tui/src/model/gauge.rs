//! Meter data for each usage window: how much is used, how far into the window
//! the sample was taken, and where the sampled pace ends up at reset.

use super::{
    Details, MIN_PROJECTION_ELAPSED_FRACTION, RateLimitBucket, RateLimitWindow,
    UsageSnapshotAssessment, WindowRole,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Gauge {
    /// Used quota in percent; it may exceed 100.
    pub(crate) used: Option<f64>,
    /// The fraction of the window that had elapsed when the sample was taken.
    pub(crate) elapsed: Option<f64>,
    /// Used quota at reset if the sampled pace holds, once past the warmup.
    pub(crate) projected: Option<f64>,
}

/// One usage window as the list and details present it.
pub(crate) struct WindowView<'a> {
    pub(crate) role: WindowRole,
    pub(crate) window: &'a RateLimitWindow,
    pub(crate) gauge: Gauge,
    pub(crate) assessment: UsageSnapshotAssessment,
}

impl RateLimitWindow {
    pub(crate) fn gauge_at(&self, observed_at: u64) -> Gauge {
        let used = self.valid_used_percent();
        let elapsed = self.timing_at(observed_at).map(|(_, elapsed)| elapsed);
        let projected = match (used, elapsed) {
            (Some(used), _) if used == 0.0 || used >= 100.0 => Some(used),
            (Some(used), Some(elapsed)) if elapsed >= MIN_PROJECTION_ELAPSED_FRACTION => {
                Some(used / elapsed)
            }
            _ => None,
        };
        Gauge {
            used,
            elapsed,
            projected,
        }
    }
}

impl Details {
    /// The primary bucket's windows, as the list shows them.
    pub(crate) fn primary_windows_at(&self, now: u64) -> Vec<WindowView<'_>> {
        self.primary_bucket()
            .map_or_else(Vec::new, |bucket| self.windows_at(bucket, now))
    }

    pub(crate) fn windows_at<'a>(
        &'a self,
        bucket: &'a RateLimitBucket,
        now: u64,
    ) -> Vec<WindowView<'a>> {
        bucket
            .windows
            .iter()
            .enumerate()
            .map(|(index, window)| WindowView {
                role: bucket.window_role(index),
                window,
                gauge: window.gauge_at(self.observed_at),
                assessment: self.window_usage_snapshot_assessment_at(bucket, index, now),
            })
            .collect()
    }
}

/// A bucket's label with each window's role and used quota.
#[cfg(test)]
pub(crate) type BucketUsage = (String, Vec<(WindowRole, Option<f64>)>);

#[cfg(test)]
impl super::HomeRow {
    /// The primary bucket's usage, as the list shows it.
    pub(crate) fn primary_usage(&self) -> Option<BucketUsage> {
        let super::Inspection::Ready(details) = self.inspection() else {
            return None;
        };
        let bucket = details.primary_bucket()?;
        let windows = details.windows_at(bucket, 0);
        Some((
            bucket.label().to_owned(),
            windows
                .iter()
                .map(|window| (window.role, window.gauge.used))
                .collect(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(used: Option<f64>, elapsed_fraction: f64) -> RateLimitWindow {
        const NOW: u64 = 2_000_000_000;
        let duration = 10_080 * 60;
        let remaining = (duration as f64 * (1.0 - elapsed_fraction)).round() as u64;
        RateLimitWindow {
            used_percent: used,
            duration_minutes: Some(10_080),
            resets_at: Some(i64::try_from(NOW + remaining).unwrap()),
        }
    }

    #[test]
    fn gauge_places_the_pace_and_projects_the_sampled_rate() {
        const NOW: u64 = 2_000_000_000;
        let gauge = window(Some(25.0), 0.5).gauge_at(NOW);
        assert_eq!(gauge.used, Some(25.0));
        assert!((gauge.elapsed.unwrap() - 0.5).abs() < 1e-6);
        assert!((gauge.projected.unwrap() - 50.0).abs() < 1e-3);

        // During the warmup the pace is known but the projection is not.
        let warmup = window(Some(5.0), 0.05).gauge_at(NOW);
        assert!(warmup.elapsed.is_some());
        assert_eq!(warmup.projected, None);

        assert_eq!(window(Some(0.0), 0.05).gauge_at(NOW).projected, Some(0.0));
        assert_eq!(
            window(Some(120.0), 0.5).gauge_at(NOW).projected,
            Some(120.0)
        );
        assert_eq!(
            window(None, 0.5).gauge_at(NOW),
            Gauge {
                used: None,
                elapsed: Some(0.5),
                projected: None
            }
        );
        let unknown_reset = RateLimitWindow {
            used_percent: Some(25.0),
            duration_minutes: Some(300),
            resets_at: None,
        };
        assert_eq!(
            unknown_reset.gauge_at(NOW),
            Gauge {
                used: Some(25.0),
                elapsed: None,
                projected: None
            }
        );
    }
}
