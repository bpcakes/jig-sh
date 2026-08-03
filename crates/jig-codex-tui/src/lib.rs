//! Responsive terminal picker for exact Codex home paths.

use std::path::PathBuf;

use serde_json::Value;

mod model;
mod render;
mod runtime;

#[cfg(test)]
mod tests;

/// An inexpensive discovered home shown before account inspection completes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Home {
    /// Exact path used for selection; it is never reconstructed from display text.
    pub path: PathBuf,
    /// Human-facing basename.
    pub name: String,
    /// Whether this path matches the current `CODEX_HOME`.
    pub current: bool,
}

/// One completed inspection, keyed by the stable discovery index.
#[derive(Clone, Debug)]
pub struct HomeUpdate {
    /// Index of the matching entry in the original `homes` vector.
    pub index: usize,
    /// Same-release normalized account and usage object.
    pub details: Value,
}

/// Supplies account and usage updates without coupling this crate to Jig runtime code.
pub trait InspectionSource: Send + Sync {
    /// Nonfatal discovery warnings known before background inspection starts.
    fn discovery_warnings(&self) -> Vec<String> {
        Vec::new()
    }

    /// Inspects homes and emits each result as it becomes available.
    ///
    /// Implementations must poll `cancelled` and clean up owned children before
    /// returning. The picker joins the inspection worker before restoring the
    /// terminal and exiting.
    fn inspect(
        &self,
        emit: &mut dyn FnMut(HomeUpdate) -> Result<(), String>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<(), String>;
}

/// Opens the full-screen picker and returns the selected exact home path.
///
/// # Errors
///
/// Returns an error when terminal setup, input, rendering, or worker ownership fails.
pub fn select(
    homes: Vec<Home>,
    source: impl InspectionSource + 'static,
) -> anyhow::Result<Option<PathBuf>> {
    select_with_cancellation(homes, source, || false)
}

/// Opens the picker while also observing process-level cancellation.
///
/// # Errors
///
/// Returns an error when terminal setup, input, rendering, or worker ownership fails.
pub fn select_with_cancellation(
    homes: Vec<Home>,
    source: impl InspectionSource + 'static,
    cancelled: impl Fn() -> bool + Send + Sync + 'static,
) -> anyhow::Result<Option<PathBuf>> {
    runtime::run(homes, source, cancelled)
}
