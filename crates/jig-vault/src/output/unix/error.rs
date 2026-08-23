use std::fmt;
use std::path::PathBuf;

use anyhow::Error;

use crate::VaultErrorKind;

#[derive(Debug)]
pub(super) enum PrivateOutputConflict {
    ExistingWithoutOverwrite(PathBuf),
    ExistingWithoutReplacement(PathBuf),
    ChangedSincePreview(PathBuf),
}

impl fmt::Display for PrivateOutputConflict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExistingWithoutOverwrite(path) => write!(
                formatter,
                "private vault output already exists at {}; pass --overwrite to replace it",
                path.display()
            ),
            Self::ExistingWithoutReplacement(path) => write!(
                formatter,
                "private vault output already exists at {}; enable replacement to replace it",
                path.display()
            ),
            Self::ChangedSincePreview(path) => write!(
                formatter,
                "private vault output destination changed since preview at {}; preview again",
                path.display()
            ),
        }
    }
}

impl std::error::Error for PrivateOutputConflict {}

pub(super) fn preflight_error_kind(error: &Error) -> VaultErrorKind {
    if is_private_output_conflict(error) {
        VaultErrorKind::AlreadyExists
    } else if error.chain().any(|source| source.is::<std::io::Error>()) {
        VaultErrorKind::Io
    } else {
        VaultErrorKind::InvalidInput
    }
}

pub(super) fn install_error_kind(error: &Error) -> VaultErrorKind {
    if is_private_output_conflict(error)
        || error
            .chain()
            .filter_map(|source| source.downcast_ref::<std::io::Error>())
            .any(|error| error.kind() == std::io::ErrorKind::AlreadyExists)
    {
        VaultErrorKind::AlreadyExists
    } else {
        VaultErrorKind::Io
    }
}

fn is_private_output_conflict(error: &Error) -> bool {
    error
        .chain()
        .any(|source| source.is::<PrivateOutputConflict>())
}
