//! Unified read-only terminal dashboard.
//!
//! Its source boundary is owned by the matching `jig-sh` release; this crate
//! owns only application state and presentation.

use std::time::Duration;

use crate::dashboard::{SourceError, TimelineLimit};

mod model;
mod render;
mod runtime;

#[cfg(test)]
mod tests;

/// Initial top-level view selected by a CLI entrypoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitialTab {
    /// Provider-oriented status overview used by `jig status --tui`.
    Status,
    /// Local work overview used by canonical `jig ui`.
    Work,
}

/// Additive terminal runtime configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DashboardOptions {
    pub initial_tab: InitialTab,
    pub local_refresh_interval: Duration,
    pub status_refresh_interval: Duration,
    pub timeline_limit: TimelineLimit,
    pub initial_plan: Option<String>,
}

impl DashboardOptions {
    #[must_use]
    pub const fn new(initial_tab: InitialTab, refresh_interval: Duration) -> Self {
        Self {
            initial_tab,
            local_refresh_interval: refresh_interval,
            status_refresh_interval: refresh_interval,
            timeline_limit: TimelineLimit::DEFAULT,
            initial_plan: None,
        }
    }

    #[must_use]
    pub const fn with_refresh_intervals(
        initial_tab: InitialTab,
        local_refresh_interval: Duration,
        status_refresh_interval: Duration,
    ) -> Self {
        Self {
            initial_tab,
            local_refresh_interval,
            status_refresh_interval,
            timeline_limit: TimelineLimit::DEFAULT,
            initial_plan: None,
        }
    }

    /// Select the initially visible timeline window.
    ///
    /// # Errors
    ///
    /// Returns an error when `rows` is outside the dashboard's supported
    /// one-through-one-thousand row range.
    pub fn with_timeline_limit(mut self, rows: usize) -> Result<Self, SourceError> {
        self.timeline_limit = TimelineLimit::new(rows)?;
        Ok(self)
    }

    #[must_use]
    pub fn with_initial_plan(mut self, plan_id: Option<String>) -> Self {
        self.initial_plan = plan_id;
        self
    }
}

/// Run the additive unified terminal application.
///
/// # Errors
///
/// Returns an error when terminal setup, collection, input, rendering, or
/// restoration fails.
pub fn run(
    source: impl crate::dashboard::DashboardSource + 'static,
    options: DashboardOptions,
) -> anyhow::Result<()> {
    runtime::run(source, options)
}

/// Runs the dashboard with an adapter-owned cooperative cancellation signal.
///
/// This is used by CLI signal supervision so terminal restoration remains in
/// the normal Rust drop path.
#[doc(hidden)]
pub fn run_with_cancellation(
    source: impl crate::dashboard::DashboardSource + 'static,
    options: DashboardOptions,
    externally_cancelled: impl Fn() -> bool,
) -> anyhow::Result<()> {
    runtime::run_with_cancellation(source, options, externally_cancelled)
}
