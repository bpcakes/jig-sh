//! Keyboard-first interactive manager for one fixed Jig Vault scope.
//!
//! This crate owns terminal presentation and interaction. The matching
//! `jig-sh` release implements [`VaultBackend`] so repository scope, external
//! tools, and filesystem policy remain outside the renderer.

use std::{fmt, io::Write, path::PathBuf};

use jig_vault::{
    AuditVerification, FieldKind, SecretBytes, VaultItem, VaultReference, VaultRevision,
    VaultSnapshot, VerifiedVaultActivity,
};
pub use jig_vault::{VaultHomeState, VaultMutation};
use ulid::Ulid;

mod browse;
mod commands;
mod line_editor;
mod model;
mod peek;
mod quick_access;
mod render;
mod runtime;
mod secret_input;
mod tools;
mod viewport;

#[cfg(test)]
mod tests;

/// Public information available before a vault is unlocked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultDescriptor {
    /// Stable human-facing scope label (`repo`, `global`, or `explicit-home`).
    pub scope: String,
    /// Repo scope identifier when this is a repo-scoped vault.
    pub scope_id: Option<String>,
    /// Repo name when this is a repo-scoped vault.
    pub repo_name: Option<String>,
    /// Exact selected vault home.
    pub home: PathBuf,
    /// Exact filesystem state of the selected vault home.
    pub home_state: VaultHomeState,
}

/// Stable classification used by the UI to fail closed on authentication loss.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VaultUiErrorKind {
    Authentication,
    InvalidInput,
    NotFound,
    Audit,
    Conflict,
    Unsupported,
    Io,
    Other,
}

/// Sanitized backend failure. Messages must never contain protected values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultUiError {
    kind: VaultUiErrorKind,
    message: String,
}

impl VaultUiError {
    /// Creates a metadata-only error for the presentation layer.
    pub fn new(kind: VaultUiErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// Returns the stable failure class.
    pub const fn kind(&self) -> VaultUiErrorKind {
        self.kind
    }

    /// Returns a safe operator-facing explanation.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for VaultUiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for VaultUiError {}

/// One typed backend operation. Protected payloads remain zeroizing and use a
/// redacted `Debug` implementation through [`SecretBytes`].
#[derive(Debug)]
pub enum VaultAction {
    Refresh,
    MigrateToV2,
    Mutate {
        revision: VaultRevision,
        mutation: VaultMutation,
    },
    Activity {
        limit: usize,
    },
    VerifyAudit,
    PreviewOnePasswordImport {
        env_file: PathBuf,
        item: VaultItem,
        out_env: PathBuf,
        replace: bool,
        overwrite: bool,
        dry_run: bool,
    },
    CommitOnePasswordImport {
        plan: ImportPlanToken,
        replace: bool,
        overwrite: bool,
    },
    DiscardOnePasswordImport {
        plan: ImportPlanToken,
    },
    CreateBackup {
        output: PathBuf,
        overwrite: bool,
    },
    RestoreBackup {
        input: PathBuf,
        passphrase: SecretBytes,
    },
    ChangePassphrase {
        new_passphrase: SecretBytes,
    },
    ExportField {
        reference: VaultReference,
        output: PathBuf,
        overwrite: bool,
    },
}

/// One planned field transition in a 1Password import preview.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportFieldChange {
    /// The destination field was absent when the revision-bound plan was made.
    Create { kind: FieldKind },
    /// The destination field existed with an authenticated prior kind.
    Replace {
        previous_kind: FieldKind,
        kind: FieldKind,
    },
}

impl ImportFieldChange {
    /// Builds the only valid transition for an observed prior kind.
    pub const fn from_previous_kind(previous_kind: Option<FieldKind>, kind: FieldKind) -> Self {
        match previous_kind {
            Some(previous_kind) => Self::Replace {
                previous_kind,
                kind,
            },
            None => Self::Create { kind },
        }
    }

    /// Returns the kind that commit will store.
    pub const fn kind(self) -> FieldKind {
        match self {
            Self::Create { kind } | Self::Replace { kind, .. } => kind,
        }
    }

    /// Returns whether this transition replaces an existing field.
    pub const fn replaces_existing(self) -> bool {
        matches!(self, Self::Replace { .. })
    }

    /// Returns whether the transition removes output-redaction treatment.
    pub const fn is_redaction_downgrade(self) -> bool {
        matches!(
            self,
            Self::Replace {
                previous_kind: FieldKind::Concealed,
                kind: FieldKind::Text,
            }
        )
    }
}

/// Safe preview row for one 1Password import assignment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportPreviewRow {
    pub variable: String,
    pub reference: VaultReference,
    pub change: ImportFieldChange,
}

/// Opaque one-shot capability for committing one exact import preview.
#[derive(Clone, Eq, PartialEq)]
pub struct ImportPlanToken(String);

impl ImportPlanToken {
    /// Creates a process-local unguessable plan identity.
    pub fn generate() -> Self {
        Self(Ulid::new().to_string())
    }
}

impl fmt::Debug for ImportPlanToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ImportPlanToken([OPAQUE])")
    }
}

/// Whether a safe import preview can be committed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportPreviewAuthorization {
    /// A dry run intentionally has no commit authority.
    DryRun,
    /// The backend retained a protected one-shot plan for this preview.
    Commit(ImportPlanToken),
}

/// Metadata-only preview of one proposed 1Password dotenv import.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportPreview {
    pub env_file: PathBuf,
    pub item: VaultItem,
    pub out_env: PathBuf,
    pub replace: bool,
    pub overwrite: bool,
    pub authorization: ImportPreviewAuthorization,
    pub rows: Vec<ImportPreviewRow>,
    pub destination_exists: bool,
}

impl ImportPreview {
    /// Returns true when this result is metadata-only and cannot be committed.
    pub const fn is_dry_run(&self) -> bool {
        matches!(self.authorization, ImportPreviewAuthorization::DryRun)
    }

    /// Returns whether any replacement removes output-redaction treatment.
    pub fn has_redaction_downgrade(&self) -> bool {
        self.rows
            .iter()
            .any(|row| row.change.is_redaction_downgrade())
    }
}

/// Metadata proving which durable action completed before snapshot refresh.
///
/// This closed value never contains protected field bytes. It lets a backend
/// distinguish a committed primary operation from a later presentation refresh
/// failure so the UI cannot suggest that retrying the primary action is harmless.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum VaultCommittedAction {
    Initialized,
    Migrated,
    Mutated,
    Imported,
    PassphraseChanged,
    BackupCreated {
        output: PathBuf,
        bytes_written: usize,
        backup_version: u32,
    },
    Exported {
        output: PathBuf,
        bytes_written: usize,
    },
}

impl VaultCommittedAction {
    /// Attaches a verified current snapshot to this completed action.
    pub fn with_snapshot(self, snapshot: VaultSnapshot) -> VaultActionResult {
        match self {
            Self::Initialized
            | Self::Migrated
            | Self::Mutated
            | Self::Imported
            | Self::PassphraseChanged => VaultActionResult::Snapshot(snapshot),
            Self::BackupCreated {
                output,
                bytes_written,
                backup_version,
            } => VaultActionResult::BackupCreated {
                output,
                bytes_written,
                backup_version,
                snapshot,
            },
            Self::Exported {
                output,
                bytes_written,
            } => VaultActionResult::Exported {
                output,
                bytes_written,
                snapshot,
            },
        }
    }
}

/// Metadata-only completion returned by [`VaultBackend::execute`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum VaultActionResult {
    Snapshot(VaultSnapshot),
    Activity(VerifiedVaultActivity),
    Audit(AuditVerification),
    ImportPreview(ImportPreview),
    ImportDiscarded,
    BackupCreated {
        output: PathBuf,
        bytes_written: usize,
        backup_version: u32,
        snapshot: VaultSnapshot,
    },
    Restored {
        root: PathBuf,
        vault_id: String,
        format_version: u32,
    },
    Exported {
        output: PathBuf,
        bytes_written: usize,
        snapshot: VaultSnapshot,
    },
    /// The primary action committed, but its trailing snapshot refresh failed.
    Committed {
        action: VaultCommittedAction,
        refresh_error: VaultUiError,
    },
}

/// Fixed-scope bridge from terminal interaction to vault/runtime policy.
pub trait VaultBackend: Send + Sync + 'static {
    /// Returns scope and existence metadata without unlocking or creating a home.
    fn descriptor(&self) -> VaultDescriptor;

    /// Rechecks vault-home state without unlocking or creating a home.
    fn home_state(&self) -> Result<VaultHomeState, VaultUiError>;

    /// Authenticates and returns a complete metadata snapshot.
    fn unlock(&self, passphrase: SecretBytes) -> Result<VaultSnapshot, VaultUiError>;

    /// Creates a new vault and enters an unlocked session.
    fn initialize(&self, passphrase: SecretBytes) -> Result<VaultSnapshot, VaultUiError>;

    /// Creates a new vault while preserving split commit/refresh outcomes.
    ///
    /// The default preserves compatibility for backends whose initialization is
    /// one indivisible operation. A backend that persists the vault before a
    /// fallible snapshot refresh should override this method and return
    /// [`VaultActionResult::Committed`] after persistence succeeds.
    fn initialize_with_completion(
        &self,
        passphrase: SecretBytes,
    ) -> Result<VaultActionResult, VaultUiError> {
        self.initialize(passphrase).map(VaultActionResult::Snapshot)
    }

    /// Drops all backend credential state.
    fn lock(&self);

    /// Refreshes metadata using the current process-local credential.
    fn refresh(&self) -> Result<VaultSnapshot, VaultUiError>;

    /// Performs one typed operation. Results may contain metadata only.
    fn execute(&self, action: VaultAction) -> Result<VaultActionResult, VaultUiError>;

    /// Performs one typed operation with cooperative cancellation available to
    /// preparation that is still safe to abandon.
    ///
    /// Backends must ignore cancellation once an audited mutation or other
    /// durable commit has started. The default keeps existing backend
    /// implementations non-cancellable and lets the runtime join them before
    /// dropping credentials or restoring the terminal.
    fn execute_with_cancellation(
        &self,
        action: VaultAction,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<VaultActionResult, VaultUiError> {
        let _ = cancelled;
        self.execute(action)
    }

    /// Reveals one canonical field directly into the supplied immediate sink.
    /// No protected bytes may be returned in the result or error.
    fn peek(
        &self,
        reference: &VaultReference,
        output: &mut dyn Write,
    ) -> Result<usize, VaultUiError>;
}

/// Opens the full-screen manager for one fixed vault scope.
///
/// `initial_passphrase` is an optional credential captured and removed from the
/// process environment before any worker starts. It never enters model state.
///
/// # Errors
///
/// Returns an error when terminal setup, rendering, input, or worker ownership
/// fails.
pub fn run(
    backend: impl VaultBackend,
    initial_passphrase: Option<SecretBytes>,
    cancelled: impl Fn() -> bool + Send + Sync + 'static,
) -> anyhow::Result<()> {
    runtime::run(backend, initial_passphrase, cancelled)
}
