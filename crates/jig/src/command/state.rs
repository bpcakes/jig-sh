//! Runtime state command DTOs.

pub(crate) use jig_state::{StateArchiveRequest, StateRestoreRequest};

#[derive(Debug)]
pub(crate) enum StateCommand {
    Summary,
    Diagnose,
    Restore(StateRestoreRequest),
    Archive(StateArchiveRequest),
}
