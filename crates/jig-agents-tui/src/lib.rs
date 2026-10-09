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

/// Opens a provider's picker using explicit subscription semantics.
///
/// `subscription_bucket` identifies the primary subscription bucket in normalized
/// inspection reports. `None` disables subscription recommendations; unknown
/// buckets can still display generic usage. Selection preserves original indices.
///
/// # Errors
/// Returns an error when terminal setup, rendering, input, or cleanup fails.
pub fn select_provider_with_cancellation(
    title: &str,
    command: &str,
    homes: Vec<ConfigurationHome>,
    warnings: Vec<String>,
    source: Option<impl InspectionSource + 'static>,
    subscription_bucket: Option<&str>,
    cancelled: impl Fn() -> bool + Send + Sync + 'static,
) -> anyhow::Result<Option<usize>> {
    let app = model::App::provider(
        title,
        homes,
        warnings,
        source.is_some(),
        subscription_bucket,
    );
    runtime::run(
        app,
        source.map(|source| Box::new(source) as Box<dyn InspectionSource>),
        command,
        cancelled,
    )
}
