//! Loopback HTTP server and server-rendered dashboard for `jig ui`.
//!
//! This crate owns UI transport, routing, query parsing, and presentation. The
//! matching `jig-sh` release supplies snapshots through [`SnapshotProvider`],
//! keeping repository state and runtime policy in the CLI crate that owns it.

use anyhow::Result;
pub mod dashboard;
mod html;
mod model;
mod server;
pub mod terminal;

pub use model::*;
pub use server::UiServer;

pub const DEFAULT_UI_PORT: u16 = 5440;
const DEFAULT_TIMELINE_LIMIT: usize = dashboard::DEFAULT_TIMELINE_ROWS;
const MAX_TIMELINE_LIMIT: usize = dashboard::MAX_TIMELINE_ROWS;

/// Read-only data source for the dashboard and plan detail routes.
pub trait SnapshotProvider: Sync {
    /// Collects the dashboard snapshot for a parsed timeline query.
    ///
    /// # Errors
    ///
    /// Returns an error when repository state or supporting status data cannot
    /// be read or assembled safely.
    fn dashboard_snapshot(&self, query: UiQuery) -> Result<DashboardSnapshot>;

    /// Collects one plan snapshot when the requested plan exists.
    ///
    /// # Errors
    ///
    /// Returns an error when plan state, related gates, or timeline data cannot
    /// be read or assembled safely.
    fn plan_snapshot(&self, plan_id: &str) -> Result<Option<PlanSnapshot>>;
}

/// Timeline query parsed from `?show=...&limit=...`. Unknown values fall back
/// to defaults so hand-typed URLs degrade instead of erroring.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiQuery {
    pub show: TimelineShow,
    pub limit: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimelineShow {
    All,
    Receipts,
    Failures,
    Plans,
    Sessions,
    Decisions,
}

impl TimelineShow {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Receipts => "receipts",
            Self::Failures => "failures",
            Self::Plans => "plans",
            Self::Sessions => "sessions",
            Self::Decisions => "decisions",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "receipts" => Self::Receipts,
            "failures" => Self::Failures,
            "plans" => Self::Plans,
            "sessions" => Self::Sessions,
            "decisions" => Self::Decisions,
            _ => Self::All,
        }
    }
}

impl Default for UiQuery {
    fn default() -> Self {
        Self {
            show: TimelineShow::All,
            limit: DEFAULT_TIMELINE_LIMIT,
        }
    }
}

impl UiQuery {
    pub fn from_query(query: Option<&str>) -> Self {
        let mut parsed = Self::default();
        for pair in query.unwrap_or("").split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            match key {
                "show" => parsed.show = TimelineShow::from_str(value),
                "limit" => {
                    if let Ok(limit) = value.parse::<usize>() {
                        parsed.limit = limit.clamp(1, MAX_TIMELINE_LIMIT);
                    }
                }
                _ => {}
            }
        }
        parsed
    }
}

#[cfg(test)]
mod tests {
    use super::{TimelineShow, UiQuery};

    #[test]
    fn ui_query_parses_show_and_clamped_limit() {
        let query = UiQuery::from_query(Some("show=failures&limit=999999"));
        assert_eq!(query.show, TimelineShow::Failures);
        assert_eq!(query.limit, 1000);

        let default = UiQuery::from_query(None);
        assert_eq!(default.show, TimelineShow::All);

        let unknown = UiQuery::from_query(Some("show=bogus&limit=zero"));
        assert_eq!(unknown.show, TimelineShow::All);
        assert_eq!(unknown.limit, default.limit);
    }
}
