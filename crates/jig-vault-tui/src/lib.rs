//! Keyboard-first interactive manager for one fixed Jig Vault scope.
//!
//! This crate owns terminal presentation and interaction. The matching
//! `jig-sh` release implements [`VaultBackend`] so repository scope, external
//! tools, and filesystem policy remain outside the renderer.

use std::{fmt, io::Write, path::PathBuf};

pub use jig_vault::VaultMutation;
use jig_vault::{
    AuditVerification, FieldKind, SecretBytes, VaultItem, VaultReference, VaultRevision,
    VaultSnapshot, VerifiedVaultActivity,
};
use ulid::Ulid;

mod model;
mod peek;
mod render;
mod runtime;
mod secret_input;
mod tools;

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
    /// Whether the complete vault currently exists.
    pub exists: bool,
}

/// Authoritative presence of initialized vault state at the fixed scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VaultPresence {
    Missing,
    Present,
}

impl VaultPresence {
    /// Converts a protected-state existence result into the typed UI state.
    pub const fn from_exists(exists: bool) -> Self {
        if exists { Self::Present } else { Self::Missing }
    }

    /// Returns whether initialized vault state is present.
    pub const fn is_present(self) -> bool {
        matches!(self, Self::Present)
    }
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
#[non_exhaustive]
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
}

/// Fixed-scope bridge from terminal interaction to vault/runtime policy.
pub trait VaultBackend: Send + Sync + 'static {
    /// Returns scope and existence metadata without unlocking or creating a home.
    fn descriptor(&self) -> VaultDescriptor;

    /// Rechecks initialized state without unlocking or creating a home.
    fn presence(&self) -> Result<VaultPresence, VaultUiError>;

    /// Authenticates and returns a complete metadata snapshot.
    fn unlock(&self, passphrase: SecretBytes) -> Result<VaultSnapshot, VaultUiError>;

    /// Creates a new vault and enters an unlocked session.
    fn initialize(&self, passphrase: SecretBytes) -> Result<VaultSnapshot, VaultUiError>;

    /// Drops all backend credential state.
    fn lock(&self);

    /// Refreshes metadata using the current process-local credential.
    fn refresh(&self) -> Result<VaultSnapshot, VaultUiError>;

    /// Performs one typed operation. Results may contain metadata only.
    fn execute(&self, action: VaultAction) -> Result<VaultActionResult, VaultUiError>;

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
