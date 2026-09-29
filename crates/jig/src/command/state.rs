//! Runtime state command DTOs.

use std::path::PathBuf;

#[derive(Debug)]
pub(crate) enum StateCommand {
    Summary,
    Diagnose,
    Restore(StateRestoreRequest),
    Archive(StateArchiveRequest),
}

#[derive(Debug)]
pub(crate) struct StateRestoreRequest {
    pub(crate) backup: PathBuf,
}

#[derive(Debug)]
pub(crate) struct StateArchiveRequest {
    pub(crate) before: String,
    pub(crate) dry_run: bool,
}
