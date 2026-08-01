//! Runtime state command DTOs.

use std::path::PathBuf;

#[derive(Debug)]
pub(crate) enum StateCommand {
    Summary,
    Diagnose(StateDiagnoseRequest),
    CompactSessions(StateCompactSessionsRequest),
    Restore(StateRestoreRequest),
    ExportReceipts(StateExportReceiptsRequest),
    Archive(StateArchiveRequest),
}

#[derive(Debug)]
pub(crate) struct StateDiagnoseRequest {
    pub(crate) deep: bool,
}

#[derive(Debug)]
pub(crate) struct StateCompactSessionsRequest {
    pub(crate) dry_run: bool,
}

#[derive(Debug)]
pub(crate) struct StateRestoreRequest {
    pub(crate) backup: PathBuf,
}

#[derive(Debug)]
pub(crate) struct StateExportReceiptsRequest {
    pub(crate) before: String,
    pub(crate) output: PathBuf,
}

#[derive(Debug)]
pub(crate) struct StateArchiveRequest {
    pub(crate) before: String,
    pub(crate) dry_run: bool,
}
