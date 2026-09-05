//! Unified read-only terminal dashboard.
//!
//! The terminal application is additive while the legacy HTTP dashboard and
//! status TUI remain routed. Its source boundary is owned by the matching
//! `jig-sh` release; this crate owns only application state and presentation.

use std::time::Duration;

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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DashboardOptions {
    pub initial_tab: InitialTab,
    pub refresh_interval: Duration,
}

impl DashboardOptions {
    #[must_use]
    pub const fn new(initial_tab: InitialTab, refresh_interval: Duration) -> Self {
        Self {
            initial_tab,
            refresh_interval,
        }
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
