//! Unified read-only terminal dashboard for `jig ui` and `jig status --tui`.
//!
//! The matching `jig-sh` release supplies typed snapshots through the
//! [`dashboard::DashboardSource`] boundary, keeping repository state and
//! runtime policy in the CLI crate that owns it.

pub mod dashboard;
pub mod terminal;
