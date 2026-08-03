//! `jig ui`: a read-only local dashboard over `.agent/state`.
//!
//! The flight recorder joins sessions, plans, receipts, decisions, work-gate
//! status, and loop workflow state into one snapshot, then serves it as a
//! server-rendered HTML page plus a JSON endpoint on a loopback socket. It is
//! runtime-owned like `jig mcp`: CLI-only, outside the generated contract, and
//! never exposed as an agent-callable tool because it starts a server process.

use std::io::Write;

use anyhow::Result;
use jig_ui::{DashboardSnapshot, PlanSnapshot, SnapshotProvider, UiQuery, UiServer};

use crate::cli::UiOpts;
use crate::context::RepoContext;

mod snapshot;

pub(crate) const DEFAULT_UI_PORT: u16 = jig_ui::DEFAULT_UI_PORT;

impl SnapshotProvider for RepoContext {
    fn dashboard_snapshot(&self, query: UiQuery) -> Result<DashboardSnapshot> {
        snapshot::snapshot_with_query(self, query)
    }

    fn plan_snapshot(&self, plan_id: &str) -> Result<Option<PlanSnapshot>> {
        snapshot::plan_snapshot(self, plan_id)
    }
}

pub(crate) fn serve(ctx: &RepoContext, opts: UiOpts, json_output: bool) -> Result<()> {
    let server = UiServer::bind(opts.port)?;
    let url = server.bootstrap_url();
    let origin = server.origin().to_string();
    let snapshot_path = server.snapshot_path();
    if json_output {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "command": "ui",
                "url": url,
                "origin": origin,
                "snapshot_path": snapshot_path,
            })
        );
    } else {
        println!("Jig UI serving at {url} (Ctrl-C to stop)");
        println!("Open this one-time URL to establish a browser session.");
        println!("Snapshot API after sign-in: {origin}{snapshot_path}");
    }
    std::io::stdout().flush()?;
    let result = server.serve(ctx);
    if json_output {
        return result.map_err(crate::cli::json_output_already_emitted);
    }
    result
}

#[cfg(test)]
mod tests;
