//! Read-only terminal dashboard for Jig status aggregates.
//!
//! The matching `jig-sh` release owns repository loading and provider
//! execution. This crate consumes aggregate JSON through [`SnapshotSource`] so
//! terminal presentation remains independent from Jig runtime internals.

use std::time::Duration;

use serde_json::Value;

mod model;
mod render;
mod runtime;

#[cfg(test)]
mod tests;

/// Read-only source of versioned Jig status aggregate snapshots.
///
/// Implementations that launch child processes must poll `cancelled` and clean
/// up owned children before returning. The TUI joins the collection worker
/// before restoring the terminal and exiting.
pub trait SnapshotSource: Send + Sync {
    fn snapshot(&self, cancelled: &dyn Fn() -> bool) -> Result<Value, String>;
}

/// Runs the interactive terminal dashboard until the operator quits.
pub fn run(
    source: impl SnapshotSource + 'static,
    refresh_interval: Duration,
) -> anyhow::Result<()> {
    runtime::run(source, refresh_interval)
}
