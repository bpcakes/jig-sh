use std::time::Duration;

use jig_status_tui::SnapshotSource;
use serde_json::Value;

use crate::context::RepoContext;

const _: fn(&RepoContext, &dyn Fn() -> bool) -> anyhow::Result<Value> =
    super::snapshot_with_cancellation;

#[allow(dead_code)]
struct RepoStatusSource {
    ctx: RepoContext,
}

impl SnapshotSource for RepoStatusSource {
    fn snapshot(&self, cancelled: &dyn Fn() -> bool) -> Result<Value, String> {
        super::snapshot_with_cancellation(&self.ctx, cancelled)
            .map_err(|error| format!("{error:#}"))
    }
}

#[allow(dead_code)]
pub(crate) fn run(ctx: RepoContext, refresh_interval: Duration) -> anyhow::Result<()> {
    jig_status_tui::run(RepoStatusSource { ctx }, refresh_interval)
}
