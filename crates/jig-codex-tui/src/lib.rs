//! Shared terminal home picker with background subscription inspection and configuration views.

use std::path::PathBuf;

use serde_json::Value;

mod model;
mod render;
mod runtime;
pub mod usage;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "tests/configuration.rs"]
mod configuration_tests;

/// An inexpensive discovered home shown before account inspection completes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Home {
    /// Exact path used for selection; it is never reconstructed from display text.
    pub path: PathBuf,
    /// Human-facing basename.
    pub name: String,
    /// Whether this entry represents the current configuration.
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

/// A configuration choice with already available details, without account inspection.
#[derive(Clone, Debug)]
pub struct ConfigurationHome {
    /// Exact home identity and display name.
    pub home: Home,
    /// Label/value pairs shown in the details pane. Both are sanitized before display.
    pub details: Vec<(String, String)>,
}

/// Uses the same layout and controls as the Codex picker for static configurations.
///
/// Returns the original entry index, preserving distinct modes with identical paths.
///
/// # Errors
///
/// Returns an error when terminal setup, input, or rendering fails.
pub fn select_configuration_with_cancellation(
    title: &str,
    command: &str,
    homes: Vec<ConfigurationHome>,
    warnings: Vec<String>,
    cancelled: impl Fn() -> bool + Send + Sync + 'static,
) -> anyhow::Result<Option<usize>> {
    runtime::run(
        model::App::configuration(title, homes, warnings),
        None,
        command,
        cancelled,
    )
}

/// Opens a configuration picker with background account and usage inspection.
/// Returns the original index, including when multiple modes share a path.
///
/// # Errors
/// Returns an error when terminal setup, input, rendering, or cleanup fails.
pub fn select_inspected_configuration_with_cancellation(
    title: &str,
    command: &str,
    homes: Vec<ConfigurationHome>,
    source: impl InspectionSource + 'static,
    cancelled: impl Fn() -> bool + Send + Sync + 'static,
) -> anyhow::Result<Option<usize>> {
    let app = model::App::inspected_configuration(title, homes, source.discovery_warnings());
    runtime::run(app, Some(Box::new(source)), command, cancelled)
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
    let paths = homes
        .iter()
        .map(|home| home.path.clone())
        .collect::<Vec<_>>();
    let app = model::App::new(homes, source.discovery_warnings());
    runtime::run(app, Some(Box::new(source)), "jig codex launch", cancelled)
        .map(|selected| selected.map(|index| paths[index].clone()))
}
