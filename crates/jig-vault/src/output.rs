use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::Error;

use crate::store::VaultStore;
use crate::{Result, SecretBytes, VaultError, VaultErrorKind};

#[cfg(unix)]
mod unix;

/// A fully written and synced private sibling file awaiting atomic install.
///
/// Preparation does not change the destination. Dropping an uninstalled value
/// removes only the generated same-directory temporary file.
pub struct PreparedPrivateFile {
    inner: PreparedPrivateOutput,
    destination: PathBuf,
    overwrite: bool,
    byte_len: usize,
    vault_output: Option<VaultOutputPolicy>,
}

/// Opaque observation of one private output destination.
///
/// This value binds a later preparation to the exact destination state seen
/// during preview. It is intentionally non-cloneable so callers consume the
/// approval once instead of silently reusing stale filesystem authority.
pub struct PrivateFilePrecondition {
    inner: PrivateDestinationPrecondition,
    destination: PathBuf,
    destination_exists: bool,
    vault_output: Option<VaultOutputPolicy>,
}

struct VaultOutputPolicy {
    store: VaultStore,
    operation_label: &'static str,
}

impl VaultOutputPolicy {
    fn validate(&self, destination: &Path) -> Result<()> {
        self.store
            .validate_external_output(destination, self.operation_label)
            .map_err(|error| VaultError::from_anyhow(VaultErrorKind::InvalidInput, error))
    }
}

impl PrivateFilePrecondition {
    /// Reports whether the exact previewed destination existed.
    pub const fn destination_exists(&self) -> bool {
        self.destination_exists
    }
}

impl fmt::Debug for PrivateFilePrecondition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PrivateFilePrecondition")
            .field("destination", &self.destination)
            .field("destination_exists", &self.destination_exists)
            .finish_non_exhaustive()
    }
}

impl PreparedPrivateFile {
    /// Captures the current hardened destination state without creating a file.
    ///
    /// The returned precondition can be consumed by
    /// [`Self::prepare_if_unchanged`] after an operator approves a preview.
    /// Existing compatibility APIs continue to express ordinary create/upsert
    /// policy without retaining a preview observation.
    ///
    /// # Errors
    ///
    /// Returns an error when the destination cannot be hardened or inspected.
    pub fn preview(destination: &Path) -> Result<PrivateFilePrecondition> {
        let inner =
            preview_private_destination(destination).map_err(output_failure_to_vault_error)?;
        Ok(PrivateFilePrecondition {
            destination: destination.to_path_buf(),
            destination_exists: private_destination_exists(&inner),
            inner,
            vault_output: None,
        })
    }

    pub(crate) fn preview_for_vault(
        store: VaultStore,
        destination: &Path,
        operation_label: &'static str,
    ) -> Result<PrivateFilePrecondition> {
        let vault_output = VaultOutputPolicy {
            store,
            operation_label,
        };
        vault_output.validate(destination)?;
        let mut precondition = Self::preview(destination)?;
        precondition.vault_output = Some(vault_output);
        Ok(precondition)
    }

    /// Validates a destination and overwrite policy without creating a file.
    ///
    /// This is suitable for dry-run reporting. It checks the same supported
    /// platform, parent, ancestor, and current leaf conditions as preparation,
    /// but installation still rechecks them to close ordinary races.
    ///
    /// # Errors
    ///
    /// Returns an error when the destination cannot be hardened or the current
    /// leaf conflicts with `overwrite`.
    pub fn preflight(destination: &Path, overwrite: bool) -> Result<()> {
        preflight_private_destination(destination, overwrite).map_err(output_failure_to_vault_error)
    }

    pub(crate) fn preflight_for_vault(
        store: &VaultStore,
        destination: &Path,
        operation_label: &'static str,
        overwrite: bool,
    ) -> Result<()> {
        store
            .validate_external_output(destination, operation_label)
            .map_err(|error| VaultError::from_anyhow(VaultErrorKind::InvalidInput, error))?;
        Self::preflight(destination, overwrite)
    }

    /// Writes and syncs an owner-only temporary file beside `destination`.
    ///
    /// # Errors
    ///
    /// Returns an error when path hardening, overwrite preflight, private
    /// creation, writing, or syncing fails. Platforms without the required
    /// guarantees reject preparation.
    pub fn prepare(destination: &Path, contents: SecretBytes, overwrite: bool) -> Result<Self> {
        let byte_len = contents.len();
        let inner = prepare_private_bytes(destination, contents.as_slice(), overwrite)
            .map_err(output_failure_to_vault_error)?;
        Ok(Self {
            inner,
            destination: destination.to_path_buf(),
            overwrite,
            byte_len,
            vault_output: None,
        })
    }

    /// Writes and syncs an owner-only temporary file only when the destination
    /// still matches a previously captured preview.
    ///
    /// When the preview observed an existing regular file, `allow_replace`
    /// must be true and that same file identity must remain installed. When it
    /// observed absence, installation remains atomic no-clobber even if
    /// `allow_replace` is true. This prevents a broad overwrite permission from
    /// authorizing a different destination than the operator reviewed.
    ///
    /// # Errors
    ///
    /// Returns an error when the destination changed since preview, replacement
    /// was not authorized, or private preparation fails.
    pub fn prepare_if_unchanged(
        precondition: PrivateFilePrecondition,
        contents: SecretBytes,
        allow_replace: bool,
    ) -> Result<Self> {
        let PrivateFilePrecondition {
            inner,
            destination,
            destination_exists,
            vault_output,
        } = precondition;
        if let Some(policy) = &vault_output {
            policy.validate(&destination)?;
        }
        let byte_len = contents.len();
        let prepared =
            prepare_private_bytes_if_unchanged(inner, contents.as_slice(), allow_replace)
                .map_err(output_failure_to_vault_error)?;
        Ok(Self {
            inner: prepared,
            destination,
            overwrite: destination_exists,
            byte_len,
            vault_output,
        })
    }

    /// Atomically installs the prepared file according to its overwrite policy.
    ///
    /// # Errors
    ///
    /// Returns an error when close-to-install path checks or the atomic
    /// namespace operation fail.
    pub fn install(self) -> Result<()> {
        let Self {
            inner,
            destination,
            vault_output,
            ..
        } = self;
        if let Some(policy) = vault_output {
            policy.validate(&destination)?;
        }
        install_prepared_private(inner).map_err(output_failure_to_vault_error)
    }
}

impl fmt::Debug for PreparedPrivateFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedPrivateFile")
            .field("destination", &self.destination)
            .field("overwrite", &self.overwrite)
            .field("byte_len", &self.byte_len)
            .field("contents", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputFailureStage {
    Preflight,
    #[cfg(unix)]
    Sink,
}

impl OutputFailureStage {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Preflight => "sink_preflight",
            #[cfg(unix)]
            Self::Sink => "sink",
        }
    }
}

#[derive(Debug)]
pub(crate) struct OutputInstallFailure {
    pub(crate) stage: OutputFailureStage,
    pub(crate) kind: VaultErrorKind,
    pub(crate) error: Error,
}

fn output_failure_to_vault_error(failure: OutputInstallFailure) -> VaultError {
    VaultError::from_anyhow(failure.kind, failure.error)
}

#[cfg(unix)]
pub(crate) fn install_private_bytes(
    path: &Path,
    bytes: &[u8],
    overwrite: bool,
) -> std::result::Result<(), OutputInstallFailure> {
    let prepared = prepare_private_bytes(path, bytes, overwrite)?;
    install_prepared_private(prepared)
}

#[cfg(not(unix))]
pub(crate) fn install_private_bytes(
    path: &Path,
    _bytes: &[u8],
    _overwrite: bool,
) -> std::result::Result<(), OutputInstallFailure> {
    Err(unsupported_private_output(path))
}

#[cfg(unix)]
type PreparedPrivateOutput = unix::PreparedPrivateOutput;

#[cfg(unix)]
type PrivateDestinationPrecondition = unix::PrivateDestinationPrecondition;

#[cfg(unix)]
fn prepare_private_bytes(
    path: &Path,
    bytes: &[u8],
    overwrite: bool,
) -> std::result::Result<PreparedPrivateOutput, OutputInstallFailure> {
    unix::prepare_private_bytes(path, bytes, overwrite)
}

#[cfg(unix)]
fn preview_private_destination(
    path: &Path,
) -> std::result::Result<PrivateDestinationPrecondition, OutputInstallFailure> {
    unix::preview_private_destination(path)
}

#[cfg(unix)]
fn private_destination_exists(precondition: &PrivateDestinationPrecondition) -> bool {
    precondition.destination_exists()
}

#[cfg(unix)]
fn prepare_private_bytes_if_unchanged(
    precondition: PrivateDestinationPrecondition,
    bytes: &[u8],
    allow_replace: bool,
) -> std::result::Result<PreparedPrivateOutput, OutputInstallFailure> {
    unix::prepare_private_bytes_if_unchanged(precondition, bytes, allow_replace)
}

#[cfg(unix)]
fn install_prepared_private(
    prepared: PreparedPrivateOutput,
) -> std::result::Result<(), OutputInstallFailure> {
    prepared.install()
}

#[cfg(unix)]
fn preflight_private_destination(
    path: &Path,
    overwrite: bool,
) -> std::result::Result<(), OutputInstallFailure> {
    unix::preflight_private_destination(path, overwrite)
}

#[cfg(not(unix))]
struct PreparedPrivateOutput;

#[cfg(not(unix))]
struct PrivateDestinationPrecondition;

#[cfg(not(unix))]
fn prepare_private_bytes(
    path: &Path,
    _bytes: &[u8],
    _overwrite: bool,
) -> std::result::Result<PreparedPrivateOutput, OutputInstallFailure> {
    Err(unsupported_private_output(path))
}

#[cfg(not(unix))]
fn preview_private_destination(
    path: &Path,
) -> std::result::Result<PrivateDestinationPrecondition, OutputInstallFailure> {
    Err(unsupported_private_output(path))
}

#[cfg(not(unix))]
fn private_destination_exists(_: &PrivateDestinationPrecondition) -> bool {
    unreachable!("unsupported platforms cannot construct private destination preconditions")
}

#[cfg(not(unix))]
fn prepare_private_bytes_if_unchanged(
    _precondition: PrivateDestinationPrecondition,
    _bytes: &[u8],
    _allow_replace: bool,
) -> std::result::Result<PreparedPrivateOutput, OutputInstallFailure> {
    unreachable!("unsupported platforms cannot construct private destination preconditions")
}

#[cfg(not(unix))]
fn install_prepared_private(
    _prepared: PreparedPrivateOutput,
) -> std::result::Result<(), OutputInstallFailure> {
    unreachable!("unsupported platforms cannot construct prepared private output")
}

#[cfg(not(unix))]
fn preflight_private_destination(
    path: &Path,
    _overwrite: bool,
) -> std::result::Result<(), OutputInstallFailure> {
    Err(unsupported_private_output(path))
}

#[cfg(not(unix))]
fn unsupported_private_output(path: &Path) -> OutputInstallFailure {
    OutputInstallFailure {
        stage: OutputFailureStage::Preflight,
        kind: VaultErrorKind::InvalidInput,
        error: anyhow::anyhow!(
            "private vault output to {} is unsupported on this platform because Jig cannot guarantee owner-only ACLs, reparse-point refusal, and atomic no-clobber installation",
            path.display()
        ),
    }
}
