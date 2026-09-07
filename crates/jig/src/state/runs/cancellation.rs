use anyhow::Result;

use crate::context::RepoContext;
use crate::state::jsonl::{append_jsonl_with_end_offset, try_scan_jsonl_raw_from};
use crate::state::records::RunEventRecord;

use super::{EVENT_CANCEL_REQUESTED, RUNS_FILE, parse_run_event_identity};

#[derive(Clone, Copy, Debug)]
pub(crate) struct RunEventCursor(u64);

pub(crate) fn run_cancel_requested_since(
    ctx: &RepoContext,
    run_id: &str,
    cursor: &mut RunEventCursor,
    cancelled: &dyn Fn() -> bool,
) -> Result<bool> {
    let path = ctx.state_file(RUNS_FILE);
    let mut requested = false;
    let scanned = try_scan_jsonl_raw_from(&path, cursor.0, cancelled, |raw| {
        let event = parse_run_event_identity(raw, &path)?;
        if event.run_id == run_id && event.event == EVENT_CANCEL_REQUESTED {
            requested = true;
        }
        Ok(())
    })?;
    if let Some((offset, _)) = scanned {
        cursor.0 = offset;
    }
    Ok(requested)
}

pub(super) fn append_event_with_cursor(
    ctx: &RepoContext,
    event: RunEventRecord,
) -> Result<RunEventCursor> {
    append_jsonl_with_end_offset(&ctx.state_file(RUNS_FILE), &event).map(RunEventCursor)
}
