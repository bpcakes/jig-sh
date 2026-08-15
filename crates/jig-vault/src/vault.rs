use std::{
    collections::BTreeSet,
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::Result as AnyResult;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use secrecy::{ExposeSecret, SecretString};
use time::OffsetDateTime;
use zeroize::Zeroizing;

use crate::audit::{
    AuditAction, AuditEvent, AuditVerification, MAX_VAULT_ACTIVITY_RECORDS, VerifiedVaultActivity,
    verified_activity_unlocked, verify_chain_unlocked,
};
use crate::broker::BrokeredRun;
use crate::crypto::KEY_LEN;
#[cfg(test)]
use crate::crypto::{NONCE_LEN, SALT_LEN, derive_wrap_key, open};
use crate::error::{
    ClassifiedVaultError, classified, classified_kind, classify_source, vault_error_from_anyhow,
};
use crate::exec::{
    ExecEnvValue, MAX_EXEC_ENV_TOTAL_BYTES, MAX_EXEC_ENV_VALUE_LEN, VaultExec,
    redactor_from_concealed_values,
};
use crate::exec_output::StreamingRedactor;
use crate::exec_process::{
    ResolvedExecEnv as ProcessExecEnv, ResolvedExecProcess, run_exec_process,
};
#[cfg(test)]
use crate::format::{AeadRole, decode_b64_array, payload_aad};
use crate::format::{FORMAT_VERSION, SecretEntry, V1_FORMAT_VERSION, VaultFile, VaultState};
use crate::output::{
    OutputInstallFailure, PreparedPrivateFile, PrivateFilePrecondition, install_private_bytes,
};
use crate::redact::MIN_REDACTABLE_LEN;
use crate::run::RunOutput;
use crate::store::VaultStore;
use crate::template::InjectionTemplate;
use crate::types::{EnvVarName, FieldKind, SecretName, VaultItem, VaultReference};
use crate::{Result, SecretBytes, VaultError, VaultErrorKind};

mod envelope;
mod lifecycle;
use envelope::{
    MigratedVaultEnvelope, NewVaultEnvelope, ParsedVaultEnvelope, ResealedVaultEnvelope,
    UnlockedVaultEnvelope,
};

pub const MAX_SECRET_VALUE_LEN: usize = 1024 * 1024;
pub const MIN_MASTER_PASSPHRASE_LEN: usize = 12;
const MAX_IMPORT_FIELDS: usize = 1_024;
const MAX_IMPORT_VALUE_BYTES: usize = 16 * 1024 * 1024;

mod brokered;

#[derive(Clone, Debug)]
pub struct Vault {
    store: VaultStore,
}

/// Filesystem state of one resolved vault home.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VaultHomeState {
    /// The vault home itself does not exist.
    Absent,
    /// The vault home exists, but initialized vault state does not.
    Uninitialized,
    /// Initialized vault state exists inside the vault home.
    Initialized,
}

impl VaultHomeState {
    /// Returns whether the vault home itself is absent.
    pub const fn is_absent(self) -> bool {
        matches!(self, Self::Absent)
    }

    /// Returns whether initialized vault state exists.
    pub const fn is_initialized(self) -> bool {
        matches!(self, Self::Initialized)
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultStatus {
    pub root: PathBuf,
    /// Exact filesystem state of the selected vault home.
    pub home_state: VaultHomeState,
    /// Compatibility projection of [`VaultStatus::home_state`].
    pub exists: bool,
}

impl Vault {
    /// Resolves a vault handle without opening encrypted state.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault home is invalid, unsafe, or cannot be
    /// resolved and hardened.
    pub fn resolve(explicit_home: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            store: VaultStore::resolve(explicit_home)?,
        })
    }

    /// Resolves a vault handle that creates new test fixtures with the minimum
    /// accepted Argon2 cost. Existing vaults continue to use the parameters in
    /// their authenticated headers.
    ///
    /// This constructor is available only to this crate's tests and consumers
    /// that explicitly enable the `test-utils` feature. Production code must
    /// use [`Vault::resolve`].
    #[cfg(any(test, feature = "test-utils"))]
    #[doc(hidden)]
    pub fn resolve_for_test(explicit_home: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            store: VaultStore::resolve_for_test(explicit_home)?,
        })
    }

    pub fn root(&self) -> &Path {
        self.store.root()
    }

    /// Reports whether the vault state file exists.
    ///
    /// # Errors
    ///
    /// Returns an error when the protected state path cannot be inspected
    /// safely.
    pub fn exists(&self) -> Result<bool> {
        self.store.exists()
    }

    /// Resolves a vault home and reports whether initialized state exists.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault home or state path is invalid, unsafe,
    /// or cannot be inspected.
    pub fn status(explicit_home: Option<PathBuf>) -> Result<VaultStatus> {
        let (root, home_state) = VaultStore::inspect(explicit_home)?;
        Ok(VaultStatus {
            root,
            home_state,
            exists: home_state.is_initialized(),
        })
    }

    /// Initializes a new encrypted vault and audit chain.
    ///
    /// # Errors
    ///
    /// Returns an error when the passphrase is invalid, vault state already
    /// exists, stale audit state is present, cryptography fails, or protected
    /// files cannot be created safely.
    pub fn init(&self, passphrase: &SecretString) -> Result<()> {
        self.store.init(passphrase)
    }

    /// Creates or updates one encrypted secret.
    ///
    /// # Errors
    ///
    /// Returns an error when the name or value is invalid, unlocking or audit
    /// verification fails, or the audit and encrypted state cannot be written
    /// safely.
    pub fn set_secret(
        &self,
        passphrase: &SecretString,
        name: &str,
        value: SecretBytes,
    ) -> Result<()> {
        self.write_secret(passphrase, name, value, VaultWriteMode::Upsert)
    }

    /// Creates or replaces one encrypted legacy secret under an atomic
    /// existence precondition.
    pub fn write_secret(
        &self,
        passphrase: &SecretString,
        name: &str,
        value: SecretBytes,
        mode: VaultWriteMode,
    ) -> Result<()> {
        self.store.write_secret(passphrase, name, value, mode)
    }

    /// Removes one encrypted secret when it exists.
    ///
    /// # Errors
    ///
    /// Returns an error when the name is invalid, unlocking or audit
    /// verification fails, or the audit and encrypted state cannot be written
    /// safely.
    pub fn remove_secret(&self, passphrase: &SecretString, name: &str) -> Result<bool> {
        self.store.remove_secret(passphrase, name)
    }

    /// Removes one legacy secret and fails if it no longer exists.
    pub fn remove_secret_required(&self, passphrase: &SecretString, name: &str) -> Result<()> {
        self.store.remove_secret_required(passphrase, name)
    }

    /// Lists secret metadata without returning plaintext values.
    ///
    /// # Errors
    ///
    /// Returns an error when vault state cannot be read, authenticated,
    /// decrypted, decoded, or validated safely.
    pub fn list(&self, passphrase: &SecretString) -> Result<Vec<SecretRecord>> {
        self.store.list(passphrase)
    }

    /// Explicitly upgrades a version 1 vault envelope to the current format.
    ///
    /// Version 1 vaults remain readable for compatibility, but field-oriented
    /// mutations require this deliberate one-way upgrade so older binaries do
    /// not silently discard field kinds.
    ///
    /// # Errors
    ///
    /// Returns an error when the target is unsupported, the vault cannot be
    /// opened and audit-verified, or the audit/state transition cannot be
    /// written atomically.
    pub fn migrate(
        &self,
        passphrase: &SecretString,
        target_version: u32,
    ) -> Result<VaultMigration> {
        self.store.migrate(passphrase, target_version)
    }

    /// Lists canonical field metadata without returning encrypted values.
    ///
    /// Legacy secret names that cannot be represented as `jig://ITEM/FIELD`
    /// remain available through [`Vault::list`] and are intentionally omitted
    /// from this field-oriented view.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault cannot be opened safely.
    pub fn list_fields(&self, passphrase: &SecretString) -> Result<Vec<FieldRecord>> {
        self.store.list_fields(passphrase)
    }

    /// Opens the vault once and returns the complete metadata needed by an
    /// interactive browser.
    ///
    /// Canonical fields and unrepresentable legacy names are disjoint. The
    /// audit chain is verified under the same vault lock before any metadata
    /// is returned. No decrypted field value is exposed.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault cannot be opened or its audit chain
    /// cannot be verified.
    pub fn snapshot(&self, passphrase: &SecretString) -> Result<VaultSnapshot> {
        self.store.snapshot(passphrase)
    }

    /// Applies one interactive mutation only while the vault still matches the
    /// authenticated metadata snapshot that authorized it.
    ///
    /// Existing operation-specific methods retain their non-interactive
    /// semantics. This command boundary is for human-approved workflows where
    /// an intervening state save must force a refresh and new approval.
    ///
    /// # Errors
    ///
    /// Returns a conflict before editing or appending audit state when the
    /// revision is stale, or the ordinary validation and persistence error for
    /// the selected mutation.
    pub fn mutate_if_unchanged(
        &self,
        passphrase: &SecretString,
        revision: VaultRevision,
        mutation: VaultMutation,
    ) -> Result<()> {
        self.store
            .mutate_if_unchanged(passphrase, revision, mutation)
    }

    /// Returns verified audit metadata and the newest activity records first.
    ///
    /// Only action-specific metadata is projected. Raw audit details, command
    /// arguments, errors, and secret values are never returned. Chain summary
    /// metadata includes the latest MAC, event count, and any recoverable torn
    /// tail instead of silently discarding verification state.
    ///
    /// # Errors
    ///
    /// Returns an error when `limit` is zero or exceeds
    /// [`MAX_VAULT_ACTIVITY_RECORDS`], the vault cannot be opened, or the audit
    /// chain cannot be verified.
    pub fn activity(
        &self,
        passphrase: &SecretString,
        limit: usize,
    ) -> Result<VerifiedVaultActivity> {
        self.store.activity(passphrase, limit)
    }

    /// Creates or updates one canonical encrypted field.
    ///
    /// Fields can be set only after an explicit v1-to-v2 migration. Text
    /// fields remain encrypted and may be empty; concealed fields retain the
    /// existing minimum length required for reliable redaction.
    ///
    /// # Errors
    ///
    /// Returns an error before mutation when the field is invalid, the vault
    /// needs migration, audit verification fails, or persistence fails.
    pub fn set_field(
        &self,
        passphrase: &SecretString,
        reference: VaultReference,
        kind: FieldKind,
        value: SecretBytes,
    ) -> Result<FieldBatchResult> {
        self.write_field(passphrase, reference, kind, value, VaultWriteMode::Upsert)
    }

    /// Creates or replaces one canonical field under an atomic existence
    /// precondition.
    pub fn write_field(
        &self,
        passphrase: &SecretString,
        reference: VaultReference,
        kind: FieldKind,
        value: SecretBytes,
        mode: VaultWriteMode,
    ) -> Result<FieldBatchResult> {
        self.store
            .write_field(passphrase, reference, kind, value, mode)
    }

    /// Removes one canonical field when it exists.
    ///
    /// # Errors
    ///
    /// Returns an error before mutation when the vault needs migration or
    /// cannot be opened, audit-verified, and saved safely.
    pub fn remove_field(
        &self,
        passphrase: &SecretString,
        reference: VaultReference,
    ) -> Result<FieldBatchResult> {
        self.store
            .apply_field_batch(passphrase, vec![FieldMutation::remove(reference)])
    }

    /// Removes one canonical field and fails if it no longer exists.
    pub fn remove_field_required(
        &self,
        passphrase: &SecretString,
        reference: VaultReference,
    ) -> Result<FieldBatchResult> {
        self.store.remove_field_required(passphrase, reference)
    }

    /// Changes one field's encrypted handling kind without exposing or
    /// rewriting its value through a caller-visible plaintext buffer. Asking
    /// for the current kind returns `changed: false` without rewriting state
    /// or appending an audit event.
    ///
    /// # Errors
    ///
    /// Returns an error when the field is missing, the current vault format
    /// is not writable v2, the new kind is incompatible with the stored value
    /// length, or the audited atomic save fails.
    pub fn change_field_kind(
        &self,
        passphrase: &SecretString,
        reference: VaultReference,
        kind: FieldKind,
    ) -> Result<FieldKindChangeResult> {
        self.store.change_field_kind(passphrase, reference, kind)
    }

    /// Atomically renames or moves one canonical field.
    ///
    /// # Errors
    ///
    /// Returns an error when source and destination match, the source is
    /// missing, the destination exists, the vault is not writable v2, or the
    /// audited atomic save fails.
    pub fn rename_field(
        &self,
        passphrase: &SecretString,
        from: VaultReference,
        to: VaultReference,
    ) -> Result<FieldBatchResult> {
        self.store.rename_field(passphrase, from, to)
    }

    /// Atomically moves every canonical field from one item to another.
    ///
    /// # Errors
    ///
    /// Returns an error when source and destination match, the source item is
    /// empty, any destination field exists, the vault is not writable v2, or
    /// the audited atomic save fails.
    pub fn rename_item(
        &self,
        passphrase: &SecretString,
        from: VaultItem,
        to: VaultItem,
    ) -> Result<FieldBatchResult> {
        self.store.rename_item(passphrase, from, to)
    }

    /// Atomically removes every canonical field in one item.
    ///
    /// # Errors
    ///
    /// Returns an error when the item is empty, the vault is not writable v2,
    /// or the audited atomic save fails.
    pub fn remove_item(
        &self,
        passphrase: &SecretString,
        item: VaultItem,
    ) -> Result<FieldBatchResult> {
        self.store.remove_item(passphrase, item)
    }

    /// Atomically converts one unrepresentable legacy secret into a canonical
    /// field while preserving its encrypted bytes and creation timestamp.
    ///
    /// # Errors
    ///
    /// Returns an error when the source is already canonical or missing, the
    /// destination exists, the requested kind is incompatible with the stored
    /// value length, the vault is not writable v2, or the audited atomic save
    /// fails.
    pub fn convert_legacy_secret(
        &self,
        passphrase: &SecretString,
        secret_name: &str,
        reference: VaultReference,
        kind: FieldKind,
    ) -> Result<LegacyConversionResult> {
        self.store
            .convert_legacy_secret(passphrase, secret_name, reference, kind)
    }

    /// Applies validated field changes as one audited vault-state transition.
    ///
    /// Set mutations create or replace fields. Duplicate references within a
    /// batch are rejected before any audit append or state save.
    ///
    /// # Errors
    ///
    /// Returns an error before mutation when validation, migration, unlock,
    /// or audit verification fails. A state-save failure can leave the audit
    /// intent ahead of state, matching the vault's existing mutation invariant.
    pub fn apply_field_batch(
        &self,
        passphrase: &SecretString,
        mutations: Vec<FieldMutation>,
    ) -> Result<FieldBatchResult> {
        self.store.apply_field_batch(passphrase, mutations)
    }

    /// Reports which proposed import references already exist in a writable
    /// version-two vault, without appending audit state or mutating fields.
    ///
    /// Returned booleans preserve the input order. The vault and its audit
    /// chain are verified together under one lock so dry-run collision reports
    /// are based on one consistent snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty, duplicate, or oversized reference set,
    /// a version-one vault, failed unlock, or invalid audit chain.
    pub fn preview_import_fields(
        &self,
        passphrase: &SecretString,
        references: &[VaultReference],
    ) -> Result<Vec<bool>> {
        self.store.preview_import_fields(passphrase, references)
    }

    /// Captures an opaque precondition for one proposed import field set.
    ///
    /// The precondition records the exact encrypted vault-state generation and
    /// ordered prior field kinds observed under the vault lock. It can later be
    /// consumed by [`Self::import_fields_if_unchanged`] so an interactive
    /// approval cannot silently widen to a different state.
    ///
    /// # Errors
    ///
    /// Returns the same validation, migration, unlock, and audit errors as
    /// [`Self::preview_import_fields`].
    pub fn plan_import_fields(
        &self,
        passphrase: &SecretString,
        references: &[VaultReference],
    ) -> Result<VaultImportPrecondition> {
        self.store.plan_import_fields(passphrase, references)
    }

    /// Atomically imports one batch of canonical encrypted fields.
    ///
    /// Imports accept only set mutations. With `replace == false`, any field
    /// collision aborts the whole batch. Collision checks, audit verification,
    /// the single `onepassword_import` intent, and state preparation all occur
    /// under one vault lock; the exact serialized envelope is bounded before
    /// that intent is appended and atomically saved.
    ///
    /// # Errors
    ///
    /// Returns an error without mutation for invalid input, a version-one
    /// vault, an existing field without replacement permission, audit failure,
    /// or an oversized final state. A save failure may leave the import intent
    /// ahead of state, matching the vault's mutation invariant.
    pub fn import_fields(
        &self,
        passphrase: &SecretString,
        mutations: Vec<FieldMutation>,
        replace: bool,
    ) -> Result<FieldBatchResult> {
        self.store.import_fields(passphrase, mutations, replace)
    }

    /// Atomically imports fields only when the vault still matches a prior
    /// [`Self::plan_import_fields`] observation.
    ///
    /// The precondition is consumed once. Any intervening vault-state save, a
    /// different vault identity, or a mismatched field set rejects the import
    /// before audit append or state mutation and requires a new preview.
    ///
    /// # Errors
    ///
    /// Returns an error without mutation when the plan is stale or mismatched,
    /// replacement was not authorized for a previewed collision, or ordinary
    /// import validation and persistence fails.
    pub fn import_fields_if_unchanged(
        &self,
        passphrase: &SecretString,
        mutations: Vec<FieldMutation>,
        precondition: VaultImportPrecondition,
        replace: bool,
    ) -> Result<FieldBatchResult> {
        self.store
            .import_fields_if_unchanged(passphrase, mutations, precondition, replace)
    }

    /// Writes one canonical field to a caller-selected stream as exact bytes.
    ///
    /// Start and resolution happen under the vault lock. The lock is released
    /// before writing and flushing, then a matching finish or failure event is
    /// recorded before this method returns. No newline is appended. Version 1
    /// canonical `ITEM/FIELD` entries remain readable as concealed fields.
    ///
    /// # Errors
    ///
    /// Returns an error without revealing bytes when preparation fails. A
    /// writer error can occur after a partial external write; its text is
    /// sanitized and a terminal failure event is recorded when possible.
    pub fn read_field_to<W: Write>(
        &self,
        passphrase: &SecretString,
        reference: VaultReference,
        writer: &mut W,
    ) -> Result<RevealResult> {
        self.store
            .prepare_field_read(passphrase, reference)?
            .write_to(writer)
    }

    /// Validates a private file destination outside this vault's owned home.
    ///
    /// The generic private-output checks are combined with the selected vault's
    /// namespace ownership policy. Callers that separate preview from writing
    /// should use [`Self::preview_private_output`] so the policy is retained and
    /// rechecked by the consumed precondition.
    ///
    /// # Errors
    ///
    /// Returns an error when the destination is inside the vault home, aliases
    /// a vault-owned source file, cannot be hardened, or conflicts with the
    /// requested overwrite policy.
    pub fn preflight_private_output(&self, path: &Path, overwrite: bool) -> Result<()> {
        PreparedPrivateFile::preflight_for_vault(&self.store, path, "private", overwrite)
    }

    /// Captures a hardened private destination outside this vault's owned home.
    ///
    /// The opaque result retains the vault namespace policy. Passing it to
    /// [`PreparedPrivateFile::prepare_if_unchanged`] rechecks that policy both
    /// before staging bytes and immediately before installation.
    ///
    /// # Errors
    ///
    /// Returns an error when the destination is inside the vault home, aliases
    /// a vault-owned source file, or cannot be hardened and inspected.
    pub fn preview_private_output(&self, path: &Path) -> Result<PrivateFilePrecondition> {
        PreparedPrivateFile::preview_for_vault(self.store.clone(), path, "private")
    }

    /// Atomically writes one canonical field to a hardened private file.
    ///
    /// Preparation happens under the vault lock, file I/O happens after lock
    /// release, and a matching finish or failure event is recorded before this
    /// method returns. Unix provides owner-only, fsynced, symlink-refusing,
    /// atomic no-clobber or regular-file replacement semantics. Other
    /// platforms reject this sink until equivalent guarantees exist.
    ///
    /// # Errors
    ///
    /// Returns a preparation, preflight, I/O, already-exists, or audit error
    /// without including field bytes in its message.
    pub fn read_field_to_file(
        &self,
        passphrase: &SecretString,
        reference: VaultReference,
        path: &Path,
        overwrite: bool,
    ) -> Result<RevealResult> {
        self.store
            .prepare_field_read(passphrase, reference)?
            .write_to_file(path, overwrite)
    }

    /// Resolves, renders, and writes a validated template as exact bytes.
    ///
    /// Call [`InjectionTemplate::parse`] before passphrase capture when CLI
    /// ordering matters. Start, complete reference resolution, and bounded
    /// rendering happen under the vault lock. The lock is released before
    /// writing and flushing, then a terminal event is recorded before return.
    /// No newline is appended.
    ///
    /// # Errors
    ///
    /// Returns an error without output when preparation fails. A writer error
    /// can occur after a partial external write; its text is sanitized and a
    /// terminal failure event is recorded when possible.
    pub fn inject_template_to<W: Write>(
        &self,
        passphrase: &SecretString,
        template: InjectionTemplate,
        writer: &mut W,
    ) -> Result<RevealResult> {
        self.store
            .prepare_template_injection(passphrase, template)?
            .write_to(writer)
    }

    /// Resolves, renders, and atomically writes a validated template to a
    /// hardened private file.
    ///
    /// Preparation and rendering happen under the vault lock, file I/O happens
    /// after lock release, and a terminal event is recorded before return.
    /// File hardening matches [`Vault::read_field_to_file`].
    ///
    /// # Errors
    ///
    /// Returns a preparation, preflight, I/O, already-exists, or audit error
    /// without including rendered bytes in its message.
    pub fn inject_template_to_file(
        &self,
        passphrase: &SecretString,
        template: InjectionTemplate,
        path: &Path,
        overwrite: bool,
    ) -> Result<RevealResult> {
        self.store
            .prepare_template_injection(passphrase, template)?
            .write_to_file(path, overwrite)
    }

    /// Verifies the vault's tamper-evident audit chain.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault cannot be unlocked or the audit log
    /// cannot be read and verified.
    pub fn verify_audit(&self, passphrase: &SecretString) -> Result<AuditVerification> {
        self.store.verify_audit(passphrase)
    }

    /// Runs a command with brokered secret environment and file mappings.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault cannot be unlocked, audit or secret
    /// resolution fails, process-tree supervision is unavailable, the command
    /// fails to start, output exceeds safety bounds, or cleanup is incomplete.
    pub fn run_brokered(
        &self,
        passphrase: &SecretString,
        request: BrokeredRun,
    ) -> Result<RunOutput> {
        self.store.run_brokered(passphrase, request)
    }

    /// Runs an ordinary command with vault-aware environment assignments and
    /// streaming concealed-value redaction.
    ///
    /// This transparent execution inherits stdin and the ordinary environment,
    /// has no Jig timeout or output cap, and does not own a separate process
    /// tree. Nonzero and signal exits are returned as successful outcomes.
    ///
    /// # Errors
    ///
    /// Returns an error when preparation, child supervision, output streaming,
    /// or lifecycle audit recording fails.
    pub fn exec(
        &self,
        passphrase: &SecretString,
        request: VaultExec,
    ) -> Result<crate::ExecOutcome> {
        self.store.exec(passphrase, request)
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecretRecord {
    pub name: String,
    pub created_at_ms: i128,
    pub updated_at_ms: i128,
    pub value_len: usize,
}

/// Metadata for one canonical vault field, without its encrypted value.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldRecord {
    pub reference: VaultReference,
    pub kind: FieldKind,
    pub created_at_ms: i128,
    pub updated_at_ms: i128,
    pub value_len: usize,
}

/// Complete authenticated vault metadata for one interactive refresh.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultSnapshot {
    pub vault_id: String,
    /// Opaque identity of the exact encrypted state represented here.
    pub revision: VaultRevision,
    pub created_at_ms: i128,
    pub format_version: u32,
    pub fields: Vec<FieldRecord>,
    pub legacy_secrets: Vec<SecretRecord>,
    pub audit: AuditVerification,
}

/// Opaque identity of one authenticated encrypted vault-state generation.
///
/// Revisions are value-like capabilities created only by authenticated vault
/// reads. Their representation remains private so callers cannot construct or
/// weaken an interactive state precondition.
#[derive(Clone, Eq, PartialEq)]
pub struct VaultRevision {
    vault_id: String,
    state_nonce_b64: String,
}

impl std::fmt::Debug for VaultRevision {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VaultRevision")
            .field("state_generation", &"[OPAQUE]")
            .finish_non_exhaustive()
    }
}

/// Metadata-only result of changing one field kind.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldKindChangeResult {
    pub reference: VaultReference,
    pub previous_kind: FieldKind,
    pub kind: FieldKind,
    pub changed: bool,
}

/// Metadata-only result of converting an unrepresentable legacy entry.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyConversionResult {
    pub secret_name: String,
    pub reference: VaultReference,
    pub kind: FieldKind,
}

/// Metadata-only result of a completed controlled reveal operation.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevealResult {
    pub bytes_written: usize,
}

/// Atomic existence policy for protected value writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VaultWriteMode {
    /// Fail if the destination already exists.
    Create,
    /// Fail if the destination no longer exists.
    Replace,
    /// Create or replace for backwards-compatible CLI behavior.
    Upsert,
}

/// One atomic field, item, or legacy-entry mutation.
#[derive(Debug)]
#[non_exhaustive]
pub enum VaultMutation {
    SetField {
        reference: VaultReference,
        kind: FieldKind,
        value: SecretBytes,
        mode: VaultWriteMode,
    },
    ChangeFieldKind {
        reference: VaultReference,
        kind: FieldKind,
    },
    RenameField {
        source: VaultReference,
        destination: VaultReference,
    },
    RenameItem {
        source: VaultItem,
        destination: VaultItem,
    },
    RemoveField {
        reference: VaultReference,
    },
    RemoveItem {
        item: VaultItem,
    },
    SetLegacy {
        name: String,
        value: SecretBytes,
        mode: VaultWriteMode,
    },
    RemoveLegacy {
        name: String,
    },
    ConvertLegacy {
        name: String,
        reference: VaultReference,
        kind: FieldKind,
    },
}

/// Opaque, one-shot optimistic-concurrency precondition for a field import.
///
/// The encrypted-state generation is intentionally hidden so callers cannot
/// construct or weaken a plan. Prior field kinds are metadata and remain
/// available for rendering a safe preview.
pub struct VaultImportPrecondition {
    revision: VaultRevision,
    fields: Vec<ImportFieldObservation>,
}

impl VaultImportPrecondition {
    /// Iterates over the planned references and whether each one existed.
    pub fn fields(&self) -> impl ExactSizeIterator<Item = (&VaultReference, bool)> {
        self.fields
            .iter()
            .map(|field| (&field.reference, field.previous_kind.is_some()))
    }

    /// Iterates over the planned references and their previously stored kinds.
    ///
    /// `None` identifies a field that did not exist when the plan was created.
    /// The observation is bound to the same opaque revision as the commit
    /// precondition, so an interactive preview can describe kind transitions
    /// without separately reopening the vault.
    pub fn fields_with_previous_kinds(
        &self,
    ) -> impl ExactSizeIterator<Item = (&VaultReference, Option<FieldKind>)> {
        self.fields
            .iter()
            .map(|field| (&field.reference, field.previous_kind))
    }
}

impl std::fmt::Debug for VaultImportPrecondition {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VaultImportPrecondition")
            .field("field_count", &self.fields.len())
            .field("state_generation", &"[OPAQUE]")
            .finish_non_exhaustive()
    }
}

struct ImportFieldObservation {
    reference: VaultReference,
    previous_kind: Option<FieldKind>,
}

#[derive(Clone, Copy)]
enum VaultEditPrecondition<'a> {
    Unconditional,
    Revision(&'a VaultRevision),
}

impl VaultEditPrecondition<'_> {
    fn enforce(self, vault: &OpenVault, stale_message: &'static str) -> AnyResult<()> {
        match self {
            Self::Unconditional => Ok(()),
            Self::Revision(revision) if vault.matches_revision(revision) => Ok(()),
            Self::Revision(_) => Err(classified(VaultErrorKind::AlreadyExists, stale_message)),
        }
    }
}

impl VaultWriteMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Replace => "replace",
            Self::Upsert => "upsert",
        }
    }
}

/// One atomic field change.
///
/// `SecretBytes` owns and zeroizes the supplied value when this mutation is
/// consumed or dropped. Its `Debug` implementation intentionally hides bytes.
#[derive(Debug)]
pub enum FieldMutation {
    Set {
        reference: VaultReference,
        kind: FieldKind,
        value: SecretBytes,
    },
    Remove {
        reference: VaultReference,
    },
}

impl FieldMutation {
    /// Creates a field set mutation.
    pub fn set(reference: VaultReference, kind: FieldKind, value: SecretBytes) -> Self {
        Self::Set {
            reference,
            kind,
            value,
        }
    }

    /// Creates a field removal mutation.
    pub fn remove(reference: VaultReference) -> Self {
        Self::Remove { reference }
    }

    fn reference(&self) -> &VaultReference {
        match self {
            Self::Set { reference, .. } | Self::Remove { reference } => reference,
        }
    }
}

/// Metadata-only outcome of an atomic field change.
#[non_exhaustive]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FieldBatchResult {
    /// References created or updated by set mutations.
    pub changed: Vec<VaultReference>,
    /// References that existed and were removed by remove mutations.
    pub removed: Vec<VaultReference>,
}

/// Outcome of an explicit vault envelope migration.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultMigration {
    pub from_version: u32,
    pub to_version: u32,
    pub changed: bool,
}

pub(crate) struct OpenVault {
    file: VaultFile,
    state: VaultState,
    dek: Zeroizing<[u8; KEY_LEN]>,
    audit_key: Zeroizing<[u8; KEY_LEN]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RevealOperation {
    FieldRead,
    TemplateInject,
}

impl RevealOperation {
    const fn start_action(self) -> AuditAction {
        match self {
            Self::FieldRead => AuditAction::FieldReadStart,
            Self::TemplateInject => AuditAction::TemplateInjectStart,
        }
    }

    const fn finish_action(self) -> AuditAction {
        match self {
            Self::FieldRead => AuditAction::FieldReadFinish,
            Self::TemplateInject => AuditAction::TemplateInjectFinish,
        }
    }

    const fn failed_action(self) -> AuditAction {
        match self {
            Self::FieldRead => AuditAction::FieldReadFailed,
            Self::TemplateInject => AuditAction::TemplateInjectFailed,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::FieldRead => "field read",
            Self::TemplateInject => "template injection",
        }
    }
}

struct RevealLifecycle {
    store: VaultStore,
    audit_key: Zeroizing<[u8; KEY_LEN]>,
    operation_id: String,
    operation: RevealOperation,
}

impl RevealLifecycle {
    fn record_finish(&self, sink: &str, bytes_written: usize) -> AnyResult<()> {
        AuditEvent::append(
            &self.store,
            self.audit_key.as_ref(),
            self.operation.finish_action(),
            serde_json::json!({
                "operation_id": self.operation_id,
                "sink": sink,
                "bytes_written": bytes_written,
            }),
        )?;
        Ok(())
    }

    fn record_failure(&self, stage: &str) -> AnyResult<()> {
        AuditEvent::append(
            &self.store,
            self.audit_key.as_ref(),
            self.operation.failed_action(),
            reveal_failure_details(&self.operation_id, stage),
        )?;
        Ok(())
    }

    fn output_error(&self, stage: &str, kind: VaultErrorKind, error: anyhow::Error) -> VaultError {
        match self.record_failure(stage) {
            Ok(()) => VaultError::from_anyhow(kind, error),
            Err(audit_error) => VaultError::from_anyhow(
                kind,
                error.context(format!(
                    "{} output failed; additionally failed to append terminal audit event: {audit_error}",
                    self.operation.label()
                )),
            ),
        }
    }

    fn finish_error(&self, error: anyhow::Error) -> VaultError {
        match self.record_failure("audit_finish") {
            Ok(()) => VaultError::from_anyhow(
                VaultErrorKind::AuditTampered,
                error.context(format!(
                    "{} output completed, but its finish audit event failed",
                    self.operation.label()
                )),
            ),
            Err(failure_error) => VaultError::from_anyhow(
                VaultErrorKind::AuditTampered,
                error.context(format!(
                    "{} output completed, but both finish and failure audit events failed: {failure_error}",
                    self.operation.label()
                )),
            ),
        }
    }
}

impl std::fmt::Debug for RevealLifecycle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RevealLifecycle")
            .field("operation_id", &self.operation_id)
            .field("operation", &self.operation)
            .field("audit_key", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

struct PreparedReveal {
    lifecycle: RevealLifecycle,
    value: SecretBytes,
}

impl PreparedReveal {
    fn write_to<W: Write>(self, writer: &mut W) -> Result<RevealResult> {
        let Self { lifecycle, value } = self;
        let bytes_written = value.len();
        if let Err(error) = writer
            .write_all(value.as_slice())
            .and_then(|()| writer.flush())
        {
            let error_kind = error.kind();
            return Err(lifecycle.output_error(
                "sink",
                VaultErrorKind::Io,
                anyhow::anyhow!(
                    "failed to write {} bytes to the selected output stream ({error_kind:?})",
                    lifecycle.operation.label()
                ),
            ));
        }
        lifecycle
            .record_finish("stream", bytes_written)
            .map_err(|error| lifecycle.finish_error(error))?;
        Ok(RevealResult { bytes_written })
    }

    fn write_to_file(self, path: &Path, overwrite: bool) -> Result<RevealResult> {
        let Self { lifecycle, value } = self;
        let bytes_written = value.len();
        if let Err(error) = lifecycle
            .store
            .validate_external_output(path, lifecycle.operation.label())
        {
            return Err(lifecycle.output_error(
                "sink_preflight",
                VaultErrorKind::InvalidInput,
                error,
            ));
        }
        if let Err(OutputInstallFailure { stage, kind, error }) =
            install_private_bytes(path, value.as_slice(), overwrite)
        {
            return Err(lifecycle.output_error(stage.as_str(), kind, error));
        }
        lifecycle
            .record_finish("file", bytes_written)
            .map_err(|error| lifecycle.finish_error(error))?;
        Ok(RevealResult { bytes_written })
    }
}

impl std::fmt::Debug for PreparedReveal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedReveal")
            .field("lifecycle", &self.lifecycle)
            .field("value_len", &self.value.len())
            .field("value", &"[REDACTED]")
            .finish()
    }
}

struct PreparedExec {
    store: VaultStore,
    audit_key: Zeroizing<[u8; KEY_LEN]>,
    operation_id: String,
    command: Vec<OsString>,
    env: Vec<ResolvedExecEnv>,
    redactor: StreamingRedactor,
}

struct ResolvedExecEnv {
    var: EnvVarName,
    value: Zeroizing<String>,
    field_kind: Option<FieldKind>,
}

impl PreparedExec {
    #[cfg(test)]
    fn record_finish(&self, exit_status: i32, exit_signal: Option<i32>) -> AnyResult<()> {
        AuditEvent::append(
            &self.store,
            self.audit_key.as_ref(),
            AuditAction::ExecFinish,
            serde_json::json!({
                "operation_id": self.operation_id,
                "exit_status": exit_status,
                "exit_signal": exit_signal,
            }),
        )?;
        Ok(())
    }

    fn execute(self) -> Result<crate::ExecOutcome> {
        let Self {
            store,
            audit_key,
            operation_id,
            command,
            env,
            redactor,
        } = self;
        let env = env
            .into_iter()
            .map(|entry| ProcessExecEnv::new(entry.var, entry.value))
            .collect();
        let request = ResolvedExecProcess::new(command, env, redactor);
        match run_exec_process(request) {
            Ok(outcome) => {
                if let Err(error) =
                    record_exec_finish(&store, audit_key.as_ref(), &operation_id, &outcome)
                {
                    let finish_error = anyhow::anyhow!(
                        "vault exec child completed, but its finish audit event failed"
                    )
                    .context(error);
                    return match record_exec_failure(
                        &store,
                        audit_key.as_ref(),
                        &operation_id,
                        "audit_finish",
                    ) {
                        Ok(()) => Err(VaultError::from_anyhow(
                            VaultErrorKind::AuditTampered,
                            finish_error,
                        )),
                        Err(failure_error) => Err(VaultError::from_anyhow(
                            VaultErrorKind::AuditTampered,
                            finish_error.context(format!(
                                "additionally failed to append vault exec failure event: {failure_error}"
                            )),
                        )),
                    };
                }
                Ok(outcome)
            }
            Err(failure) => {
                let stage = failure.stage();
                let process_error = failure.into_error();
                if let Err(audit_error) =
                    record_exec_failure(&store, audit_key.as_ref(), &operation_id, stage)
                {
                    return Err(VaultError::from_anyhow(
                        VaultErrorKind::Process,
                        process_error.context(format!(
                            "additionally failed to append vault exec failure event: {audit_error}"
                        )),
                    ));
                }
                Err(VaultError::from_anyhow(
                    VaultErrorKind::Process,
                    process_error,
                ))
            }
        }
    }
}

impl std::fmt::Debug for PreparedExec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedExec")
            .field("operation_id", &self.operation_id)
            .field("argument_count", &self.command.len())
            .field("arguments", &"[REDACTED]")
            .field("environment_count", &self.env.len())
            .field("environment_values", &"[REDACTED]")
            .field("audit_key", &"[REDACTED]")
            .field("redactor", &self.redactor)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ResolvedExecEnv {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResolvedExecEnv")
            .field("var", &self.var)
            .field("value_len", &self.value.len())
            .field("value", &"[REDACTED]")
            .field("field_kind", &self.field_kind)
            .finish()
    }
}

impl std::fmt::Debug for OpenVault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenVault")
            .field("vault_id", &self.file.header.vault_id)
            .field("secret_count", &self.state.secrets.len())
            .field("dek", &"[REDACTED]")
            .field("audit_key", &"[REDACTED]")
            .finish()
    }
}

impl OpenVault {
    fn from_unlocked(envelope: UnlockedVaultEnvelope) -> Self {
        let UnlockedVaultEnvelope {
            file,
            state,
            dek,
            audit_key,
        } = envelope;
        Self {
            file,
            state,
            dek,
            audit_key,
        }
    }
}

impl VaultStore {
    pub(crate) fn init(&self, passphrase: &SecretString) -> Result<()> {
        self.with_lock(|| self.init_unlocked(passphrase))
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    fn init_unlocked(&self, passphrase: &SecretString) -> AnyResult<()> {
        if self.read_vault_text()?.is_some() {
            return Err(classified(
                VaultErrorKind::AlreadyExists,
                format!("vault already exists at {}", self.vault_path().display()),
            ));
        }
        if self.audit_exists()? {
            return Err(classified(
                VaultErrorKind::AuditTampered,
                format!(
                    "vault audit log already exists at {}; remove the stale vault home before init",
                    self.audit_path().display()
                ),
            ));
        }
        validate_new_vault_passphrase_inner(passphrase)?;

        let envelope =
            NewVaultEnvelope::seal(passphrase, now_ms(), self.initialization_kdf().clone())?;
        if let Err(error) = AuditEvent::append_unlocked(
            self,
            envelope.audit_key.as_ref(),
            AuditAction::VaultInitialized,
            serde_json::json!({
                "vault_id": envelope.file.header.vault_id,
            }),
        ) {
            let cleanup_error = rollback_failed_init(self);
            let error = error.context("failed to initialize vault audit log");
            return match cleanup_error {
                Some(cleanup_error) => Err(error.context(cleanup_error)),
                None => Err(error),
            };
        }
        if let Err(error) = self.write_vault_text_unlocked(&envelope.file_text) {
            let cleanup_error = rollback_failed_init(self);
            let error = error.context("failed to write initialized vault file");
            return match cleanup_error {
                Some(cleanup_error) => Err(error.context(cleanup_error)),
                None => Err(error),
            };
        }
        Ok(())
    }

    /// Runs a vault mutation while preserving the audit invariant: open under
    /// lock, verify the current chain, mutate in memory, append the audit
    /// intent, then save state before releasing the lock. If the process dies
    /// mid-operation, the audit may lead state but state must not lead audit.
    pub(crate) fn edit_with_audit<R>(
        &self,
        passphrase: &SecretString,
        action: AuditAction,
        edit: impl FnOnce(&mut OpenVault) -> AnyResult<R>,
        details: impl FnOnce(&R) -> serde_json::Value,
    ) -> AnyResult<R> {
        self.edit_with_audit_if(passphrase, action, edit, |_| true, details)
    }

    fn edit_with_audit_precondition<R>(
        &self,
        passphrase: &SecretString,
        precondition: VaultEditPrecondition<'_>,
        action: AuditAction,
        edit: impl FnOnce(&mut OpenVault) -> AnyResult<R>,
        details: impl FnOnce(&R) -> serde_json::Value,
    ) -> AnyResult<R> {
        self.edit_with_audit_if_precondition(
            passphrase,
            precondition,
            action,
            edit,
            |_| true,
            details,
        )
    }

    /// Runs a verified in-memory edit and persists it only when the result
    /// identifies a real state transition. A skipped edit still opens the
    /// vault and verifies the audit chain, but it neither seals state nor
    /// appends an audit event.
    pub(crate) fn edit_with_audit_if<R>(
        &self,
        passphrase: &SecretString,
        action: AuditAction,
        edit: impl FnOnce(&mut OpenVault) -> AnyResult<R>,
        should_commit: impl FnOnce(&R) -> bool,
        details: impl FnOnce(&R) -> serde_json::Value,
    ) -> AnyResult<R> {
        self.edit_with_audit_if_precondition(
            passphrase,
            VaultEditPrecondition::Unconditional,
            action,
            edit,
            should_commit,
            details,
        )
    }

    fn edit_with_audit_if_precondition<R>(
        &self,
        passphrase: &SecretString,
        precondition: VaultEditPrecondition<'_>,
        action: AuditAction,
        edit: impl FnOnce(&mut OpenVault) -> AnyResult<R>,
        should_commit: impl FnOnce(&R) -> bool,
        details: impl FnOnce(&R) -> serde_json::Value,
    ) -> AnyResult<R> {
        self.with_lock(|| {
            let mut vault = self.open_unlocked(passphrase)?;
            verify_chain_unlocked(self, vault.audit_key.as_ref()).map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "vault audit chain verification failed",
                    error,
                )
            })?;
            precondition.enforce(
                &vault,
                "vault state changed since the metadata snapshot; refresh and retry",
            )?;
            let result = edit(&mut vault)?;
            if !should_commit(&result) {
                return Ok(result);
            }
            let envelope = vault.prepare_save_unlocked()?;
            let file_text = envelope.serialize_pretty()?;
            self.validate_vault_text_len(&file_text).map_err(|error| {
                classify_source(
                    VaultErrorKind::InvalidInput,
                    "vault state is too large to save safely",
                    error,
                )
            })?;
            AuditEvent::append_unlocked(self, vault.audit_key.as_ref(), action, details(&result))
                .map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "vault audit append failed before state save",
                    error,
                )
            })?;
            self.write_vault_text_unlocked(&file_text)
                .map_err(|error| {
                    classify_source(
                        VaultErrorKind::Io,
                        "vault audit was appended, but state save failed",
                        error,
                    )
                })?;
            Ok(result)
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_secret(
        &self,
        passphrase: &SecretString,
        name: &str,
        value: SecretBytes,
    ) -> Result<()> {
        self.write_secret(passphrase, name, value, VaultWriteMode::Upsert)
    }

    pub(crate) fn write_secret(
        &self,
        passphrase: &SecretString,
        name: &str,
        value: SecretBytes,
        mode: VaultWriteMode,
    ) -> Result<()> {
        self.write_secret_precondition(
            passphrase,
            name,
            value,
            mode,
            VaultEditPrecondition::Unconditional,
        )
    }

    fn write_secret_precondition(
        &self,
        passphrase: &SecretString,
        name: &str,
        value: SecretBytes,
        mode: VaultWriteMode,
        precondition: VaultEditPrecondition<'_>,
    ) -> Result<()> {
        let name = SecretName::parse(name)?;
        self.write_secret_inner(passphrase, name, value, mode, precondition)
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    fn write_secret_inner(
        &self,
        passphrase: &SecretString,
        name: SecretName,
        value: SecretBytes,
        mode: VaultWriteMode,
        precondition: VaultEditPrecondition<'_>,
    ) -> AnyResult<()> {
        // Reject too-short values before unlocking; `OpenVault::set_secret`
        // repeats this guard for internal callers that already hold a handle.
        validate_secret_value_len(value.len())?;
        self.edit_with_audit_precondition(
            passphrase,
            precondition,
            AuditAction::SecretSet,
            |vault| {
                vault.ensure_write_mode(&name, mode, "vault secret")?;
                vault.set_secret(&name, value)
            },
            |()| {
                serde_json::json!({
                    "secret_name": name.as_str(),
                    "write_mode": mode.as_str(),
                })
            },
        )
    }

    pub(crate) fn remove_secret(&self, passphrase: &SecretString, name: &str) -> Result<bool> {
        let name = SecretName::parse(name)?;
        self.remove_secret_inner(passphrase, name)
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    fn remove_secret_inner(&self, passphrase: &SecretString, name: SecretName) -> AnyResult<bool> {
        self.edit_with_audit(
            passphrase,
            AuditAction::SecretRemove,
            |vault| Ok(vault.remove_secret(&name)),
            |removed| {
                serde_json::json!({
                    "secret_name": name.as_str(),
                    "removed": removed,
                })
            },
        )
    }

    pub(crate) fn remove_secret_required(
        &self,
        passphrase: &SecretString,
        name: &str,
    ) -> Result<()> {
        self.remove_secret_required_precondition(
            passphrase,
            name,
            VaultEditPrecondition::Unconditional,
        )
    }

    fn remove_secret_required_precondition(
        &self,
        passphrase: &SecretString,
        name: &str,
        precondition: VaultEditPrecondition<'_>,
    ) -> Result<()> {
        let name = SecretName::parse(name)?;
        self.edit_with_audit_precondition(
            passphrase,
            precondition,
            AuditAction::SecretRemove,
            |vault| {
                if !vault.remove_secret(&name) {
                    return Err(classified(
                        VaultErrorKind::NotFound,
                        format!("vault secret '{}' no longer exists", name.as_str()),
                    ));
                }
                Ok(())
            },
            |()| {
                serde_json::json!({
                    "secret_name": name.as_str(),
                    "removed": true,
                })
            },
        )
        .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    pub(crate) fn list(&self, passphrase: &SecretString) -> Result<Vec<SecretRecord>> {
        self.with_lock(|| self.open_unlocked(passphrase).map(|vault| vault.list()))
            .map_err(|error| self.map_open_error(error))
    }

    pub(crate) fn migrate(
        &self,
        passphrase: &SecretString,
        target_version: u32,
    ) -> Result<VaultMigration> {
        if target_version != FORMAT_VERSION {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                format!(
                    "unsupported vault migration target {target_version}; run `jig vault migrate --to {FORMAT_VERSION}`"
                ),
            ));
        }
        self.with_lock(|| self.migrate_unlocked(passphrase, target_version))
            .map_err(|error| self.map_open_error(error))
    }

    fn migrate_unlocked(
        &self,
        passphrase: &SecretString,
        target_version: u32,
    ) -> AnyResult<VaultMigration> {
        let vault = self.open_unlocked(passphrase)?;
        vault.verify_audit_unlocked(self).map_err(|error| {
            classify_source(
                VaultErrorKind::AuditTampered,
                "vault audit chain verification failed",
                error,
            )
        })?;
        let from_version = vault.format_version();
        if from_version == target_version {
            return Ok(VaultMigration {
                from_version,
                to_version: target_version,
                changed: false,
            });
        }
        if from_version != V1_FORMAT_VERSION {
            return Err(classified(
                VaultErrorKind::InvalidInput,
                format!("vault format {from_version} cannot be migrated to {target_version}"),
            ));
        }

        let envelope =
            MigratedVaultEnvelope::v1_to_v2(&vault.file, passphrase, &vault.dek, &vault.state)?;
        let file_text = envelope.serialize_pretty()?;
        self.validate_vault_text_len(&file_text).map_err(|error| {
            classify_source(
                VaultErrorKind::InvalidInput,
                "vault format migration would exceed the persistent vault size limit",
                error,
            )
        })?;
        AuditEvent::append_unlocked(
            self,
            vault.audit_key.as_ref(),
            AuditAction::VaultFormatMigrate,
            serde_json::json!({
                "from_version": from_version,
                "to_version": target_version,
            }),
        )
        .map_err(|error| {
            classify_source(
                VaultErrorKind::AuditTampered,
                "vault audit append failed before format migration save",
                error,
            )
        })?;
        self.write_vault_text_unlocked(&file_text)
            .map_err(|error| {
                classify_source(
                    VaultErrorKind::Io,
                    "vault format migration audit was appended, but state save failed",
                    error,
                )
            })?;
        Ok(VaultMigration {
            from_version,
            to_version: target_version,
            changed: true,
        })
    }

    pub(crate) fn list_fields(&self, passphrase: &SecretString) -> Result<Vec<FieldRecord>> {
        self.with_lock(|| {
            self.open_unlocked(passphrase)
                .map(|vault| vault.list_fields())
        })
        .map_err(|error| self.map_open_error(error))
    }

    pub(crate) fn snapshot(&self, passphrase: &SecretString) -> Result<VaultSnapshot> {
        self.with_lock(|| {
            let vault = self.open_unlocked(passphrase)?;
            let audit = vault.verify_audit_unlocked(self).map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "vault audit chain verification failed",
                    error,
                )
            })?;
            Ok(vault.snapshot(audit))
        })
        .map_err(|error| {
            if error.is::<ClassifiedVaultError>() {
                vault_error_from_anyhow(VaultErrorKind::Internal, error)
            } else {
                self.map_open_error(error)
            }
        })
    }

    fn mutate_if_unchanged(
        &self,
        passphrase: &SecretString,
        revision: VaultRevision,
        mutation: VaultMutation,
    ) -> Result<()> {
        let precondition = VaultEditPrecondition::Revision(&revision);
        match mutation {
            VaultMutation::SetField {
                reference,
                kind,
                value,
                mode,
            } => self
                .write_field_precondition(passphrase, reference, kind, value, mode, precondition)
                .map(drop),
            VaultMutation::ChangeFieldKind { reference, kind } => self
                .change_field_kind_precondition(passphrase, reference, kind, precondition)
                .map(drop),
            VaultMutation::RenameField {
                source,
                destination,
            } => self
                .rename_field_precondition(passphrase, source, destination, precondition)
                .map(drop),
            VaultMutation::RenameItem {
                source,
                destination,
            } => self
                .rename_item_precondition(passphrase, source, destination, precondition)
                .map(drop),
            VaultMutation::RemoveField { reference } => self
                .remove_field_required_precondition(passphrase, reference, precondition)
                .map(drop),
            VaultMutation::RemoveItem { item } => self
                .remove_item_precondition(passphrase, item, precondition)
                .map(drop),
            VaultMutation::SetLegacy { name, value, mode } => {
                self.write_secret_precondition(passphrase, &name, value, mode, precondition)
            }
            VaultMutation::RemoveLegacy { name } => {
                self.remove_secret_required_precondition(passphrase, &name, precondition)
            }
            VaultMutation::ConvertLegacy {
                name,
                reference,
                kind,
            } => self
                .convert_legacy_secret_precondition(
                    passphrase,
                    &name,
                    reference,
                    kind,
                    precondition,
                )
                .map(drop),
        }
    }

    pub(crate) fn activity(
        &self,
        passphrase: &SecretString,
        limit: usize,
    ) -> Result<VerifiedVaultActivity> {
        if !(1..=MAX_VAULT_ACTIVITY_RECORDS).contains(&limit) {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                format!("vault activity limit must be between 1 and {MAX_VAULT_ACTIVITY_RECORDS}"),
            ));
        }
        self.with_lock(|| {
            let vault = self.open_unlocked(passphrase)?;
            verified_activity_unlocked(self, vault.audit_key.as_ref(), limit).map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "vault audit activity verification failed",
                    error,
                )
            })
        })
        .map_err(|error| {
            if error.is::<ClassifiedVaultError>() {
                vault_error_from_anyhow(VaultErrorKind::Internal, error)
            } else {
                self.map_open_error(error)
            }
        })
    }

    pub(crate) fn apply_field_batch(
        &self,
        passphrase: &SecretString,
        mutations: Vec<FieldMutation>,
    ) -> Result<FieldBatchResult> {
        validate_field_mutations(&mutations)
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::InvalidInput, error))?;
        let audit_sets = field_batch_set_audit_metadata(&mutations);
        let audit_removes = field_batch_remove_audit_metadata(&mutations);
        self.edit_with_audit(
            passphrase,
            AuditAction::FieldBatchApply,
            |vault| {
                vault.ensure_field_format_v2()?;
                Ok(vault.apply_validated_field_batch(mutations))
            },
            |result| field_batch_audit_details(&audit_sets, &audit_removes, result),
        )
        .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    pub(crate) fn write_field(
        &self,
        passphrase: &SecretString,
        reference: VaultReference,
        kind: FieldKind,
        value: SecretBytes,
        mode: VaultWriteMode,
    ) -> Result<FieldBatchResult> {
        self.write_field_precondition(
            passphrase,
            reference,
            kind,
            value,
            mode,
            VaultEditPrecondition::Unconditional,
        )
    }

    fn write_field_precondition(
        &self,
        passphrase: &SecretString,
        reference: VaultReference,
        kind: FieldKind,
        value: SecretBytes,
        mode: VaultWriteMode,
        precondition: VaultEditPrecondition<'_>,
    ) -> Result<FieldBatchResult> {
        let mutations = vec![FieldMutation::set(reference.clone(), kind, value)];
        validate_field_mutations(&mutations)
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::InvalidInput, error))?;
        let audit_sets = field_batch_set_audit_metadata(&mutations);
        let name = reference.to_secret_name();
        self.edit_with_audit_precondition(
            passphrase,
            precondition,
            AuditAction::FieldBatchApply,
            |vault| {
                vault.ensure_field_format_v2()?;
                vault.ensure_write_mode(&name, mode, "vault field")?;
                Ok(vault.apply_validated_field_batch(mutations))
            },
            |result| {
                let mut details = field_batch_audit_details(&audit_sets, &[], result);
                details["write_mode"] = serde_json::json!(mode.as_str());
                details
            },
        )
        .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    pub(crate) fn remove_field_required(
        &self,
        passphrase: &SecretString,
        reference: VaultReference,
    ) -> Result<FieldBatchResult> {
        self.remove_field_required_precondition(
            passphrase,
            reference,
            VaultEditPrecondition::Unconditional,
        )
    }

    fn remove_field_required_precondition(
        &self,
        passphrase: &SecretString,
        reference: VaultReference,
        precondition: VaultEditPrecondition<'_>,
    ) -> Result<FieldBatchResult> {
        let mutations = vec![FieldMutation::remove(reference.clone())];
        let name = reference.to_secret_name();
        self.edit_with_audit_precondition(
            passphrase,
            precondition,
            AuditAction::FieldBatchApply,
            |vault| {
                vault.ensure_field_format_v2()?;
                if !vault.state.secrets.contains_key(name.as_str()) {
                    return Err(classified(
                        VaultErrorKind::NotFound,
                        format!("vault field '{reference}' no longer exists"),
                    ));
                }
                Ok(vault.apply_validated_field_batch(mutations))
            },
            |result| field_batch_audit_details(&[], std::slice::from_ref(&reference), result),
        )
        .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    pub(crate) fn change_field_kind(
        &self,
        passphrase: &SecretString,
        reference: VaultReference,
        kind: FieldKind,
    ) -> Result<FieldKindChangeResult> {
        self.change_field_kind_precondition(
            passphrase,
            reference,
            kind,
            VaultEditPrecondition::Unconditional,
        )
    }

    fn change_field_kind_precondition(
        &self,
        passphrase: &SecretString,
        reference: VaultReference,
        kind: FieldKind,
        precondition: VaultEditPrecondition<'_>,
    ) -> Result<FieldKindChangeResult> {
        let audit_reference = reference.clone();
        self.edit_with_audit_if_precondition(
            passphrase,
            precondition,
            AuditAction::FieldKindChange,
            |vault| {
                vault.ensure_field_format_v2()?;
                vault.change_field_kind(reference, kind)
            },
            |result| result.changed,
            |result| {
                serde_json::json!({
                    "reference": audit_reference.to_string(),
                    "from_kind": result.previous_kind.as_str(),
                    "to_kind": result.kind.as_str(),
                    "changed": result.changed,
                })
            },
        )
        .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    pub(crate) fn rename_field(
        &self,
        passphrase: &SecretString,
        from: VaultReference,
        to: VaultReference,
    ) -> Result<FieldBatchResult> {
        self.rename_field_precondition(passphrase, from, to, VaultEditPrecondition::Unconditional)
    }

    fn rename_field_precondition(
        &self,
        passphrase: &SecretString,
        from: VaultReference,
        to: VaultReference,
        precondition: VaultEditPrecondition<'_>,
    ) -> Result<FieldBatchResult> {
        if from == to {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                "field rename source and destination must differ",
            ));
        }
        let audit_from = from.clone();
        let audit_to = to.clone();
        self.edit_with_audit_precondition(
            passphrase,
            precondition,
            AuditAction::FieldRename,
            |vault| {
                vault.ensure_field_format_v2()?;
                vault.rename_field(from, to)
            },
            |result| {
                serde_json::json!({
                    "from": audit_from.to_string(),
                    "to": audit_to.to_string(),
                    "changed": !result.changed.is_empty(),
                })
            },
        )
        .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    pub(crate) fn rename_item(
        &self,
        passphrase: &SecretString,
        from: VaultItem,
        to: VaultItem,
    ) -> Result<FieldBatchResult> {
        self.rename_item_precondition(passphrase, from, to, VaultEditPrecondition::Unconditional)
    }

    fn rename_item_precondition(
        &self,
        passphrase: &SecretString,
        from: VaultItem,
        to: VaultItem,
        precondition: VaultEditPrecondition<'_>,
    ) -> Result<FieldBatchResult> {
        if from == to {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                "item rename source and destination must differ",
            ));
        }
        let audit_from = from.clone();
        let audit_to = to.clone();
        self.edit_with_audit_precondition(
            passphrase,
            precondition,
            AuditAction::ItemRename,
            |vault| {
                vault.ensure_field_format_v2()?;
                vault.rename_item(from, to)
            },
            |result| {
                serde_json::json!({
                    "from": audit_from.to_string(),
                    "to": audit_to.to_string(),
                    "field_count": result.changed.len(),
                })
            },
        )
        .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    pub(crate) fn remove_item(
        &self,
        passphrase: &SecretString,
        item: VaultItem,
    ) -> Result<FieldBatchResult> {
        self.remove_item_precondition(passphrase, item, VaultEditPrecondition::Unconditional)
    }

    fn remove_item_precondition(
        &self,
        passphrase: &SecretString,
        item: VaultItem,
        precondition: VaultEditPrecondition<'_>,
    ) -> Result<FieldBatchResult> {
        let audit_item = item.clone();
        self.edit_with_audit_precondition(
            passphrase,
            precondition,
            AuditAction::ItemRemove,
            |vault| {
                vault.ensure_field_format_v2()?;
                vault.remove_item(item)
            },
            |result| {
                serde_json::json!({
                    "item": audit_item.to_string(),
                    "removed_count": result.removed.len(),
                })
            },
        )
        .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    pub(crate) fn convert_legacy_secret(
        &self,
        passphrase: &SecretString,
        secret_name: &str,
        reference: VaultReference,
        kind: FieldKind,
    ) -> Result<LegacyConversionResult> {
        self.convert_legacy_secret_precondition(
            passphrase,
            secret_name,
            reference,
            kind,
            VaultEditPrecondition::Unconditional,
        )
    }

    fn convert_legacy_secret_precondition(
        &self,
        passphrase: &SecretString,
        secret_name: &str,
        reference: VaultReference,
        kind: FieldKind,
        precondition: VaultEditPrecondition<'_>,
    ) -> Result<LegacyConversionResult> {
        let secret_name = SecretName::parse(secret_name)?;
        if VaultReference::from_secret_name(&secret_name).is_some() {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                format!(
                    "vault secret '{}' is already a canonical field and does not need conversion",
                    secret_name.as_str()
                ),
            ));
        }
        let audit_name = secret_name.clone();
        let audit_reference = reference.clone();
        self.edit_with_audit_precondition(
            passphrase,
            precondition,
            AuditAction::LegacySecretConvert,
            |vault| {
                vault.ensure_field_format_v2()?;
                vault.convert_legacy_secret(secret_name, reference, kind)
            },
            |result| {
                serde_json::json!({
                    "secret_name": audit_name.as_str(),
                    "reference": audit_reference.to_string(),
                    "kind": result.kind.as_str(),
                })
            },
        )
        .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    pub(crate) fn preview_import_fields(
        &self,
        passphrase: &SecretString,
        references: &[VaultReference],
    ) -> Result<Vec<bool>> {
        self.plan_import_fields(passphrase, references)
            .map(|precondition| {
                precondition
                    .fields
                    .into_iter()
                    .map(|field| field.previous_kind.is_some())
                    .collect()
            })
    }

    pub(crate) fn plan_import_fields(
        &self,
        passphrase: &SecretString,
        references: &[VaultReference],
    ) -> Result<VaultImportPrecondition> {
        validate_import_references(references)
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::InvalidInput, error))?;
        self.with_lock(|| {
            let vault = self.open_unlocked(passphrase)?;
            vault.verify_audit_unlocked(self).map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "vault audit chain verification failed",
                    error,
                )
            })?;
            vault.ensure_field_format_v2()?;
            Ok(VaultImportPrecondition {
                revision: vault.revision(),
                fields: references
                    .iter()
                    .map(|reference| ImportFieldObservation {
                        reference: reference.clone(),
                        previous_kind: vault.field_kind(reference),
                    })
                    .collect(),
            })
        })
        .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    pub(crate) fn import_fields(
        &self,
        passphrase: &SecretString,
        mutations: Vec<FieldMutation>,
        replace: bool,
    ) -> Result<FieldBatchResult> {
        self.import_fields_with_precondition(passphrase, mutations, None, replace)
    }

    pub(crate) fn import_fields_if_unchanged(
        &self,
        passphrase: &SecretString,
        mutations: Vec<FieldMutation>,
        precondition: VaultImportPrecondition,
        replace: bool,
    ) -> Result<FieldBatchResult> {
        self.import_fields_with_precondition(passphrase, mutations, Some(precondition), replace)
    }

    fn import_fields_with_precondition(
        &self,
        passphrase: &SecretString,
        mutations: Vec<FieldMutation>,
        precondition: Option<VaultImportPrecondition>,
        replace: bool,
    ) -> Result<FieldBatchResult> {
        validate_import_mutations(&mutations)
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::InvalidInput, error))?;
        let fields = field_batch_set_audit_metadata(&mutations);
        if let Some(precondition) = &precondition {
            validate_import_precondition(precondition, &fields)
                .map_err(|error| vault_error_from_anyhow(VaultErrorKind::InvalidInput, error))?;
        }
        self.edit_with_audit(
            passphrase,
            AuditAction::OnePasswordImport,
            |vault| {
                vault.ensure_field_format_v2()?;
                if let Some(precondition) = precondition {
                    enforce_import_precondition(vault, &precondition, replace)?;
                } else if !replace {
                    reject_import_collisions(vault, &fields)?;
                }
                Ok(vault.apply_validated_field_batch(mutations))
            },
            |_| onepassword_import_audit_details(&fields),
        )
        .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    fn prepare_field_read(
        &self,
        passphrase: &SecretString,
        reference: VaultReference,
    ) -> Result<PreparedReveal> {
        let operation_id = ulid::Ulid::new().to_string();
        self.with_lock(|| {
            let vault = self.open_unlocked(passphrase)?;
            vault
                .append_audit_unlocked(
                    self,
                    RevealOperation::FieldRead.start_action(),
                    serde_json::json!({
                        "operation_id": operation_id,
                        "reference": reference.to_string(),
                    }),
                )
                .map_err(reveal_start_audit_error)?;
            let value = match vault.secret_value(&reference.to_secret_name()) {
                Ok(value) => value,
                Err(error) => {
                    return Err(reveal_prepare_failure_unlocked(
                        self,
                        &vault,
                        RevealOperation::FieldRead,
                        &operation_id,
                        "resolve",
                        error,
                    ));
                }
            };
            let OpenVault { audit_key, .. } = vault;
            Ok(PreparedReveal {
                lifecycle: RevealLifecycle {
                    store: self.clone(),
                    audit_key,
                    operation_id,
                    operation: RevealOperation::FieldRead,
                },
                value,
            })
        })
        .map_err(|error| self.map_reveal_prepare_error(error))
    }

    fn prepare_template_injection(
        &self,
        passphrase: &SecretString,
        template: InjectionTemplate,
    ) -> Result<PreparedReveal> {
        let operation_id = ulid::Ulid::new().to_string();
        self.with_lock(|| {
            let vault = self.open_unlocked(passphrase)?;
            vault
                .append_audit_unlocked(
                    self,
                    RevealOperation::TemplateInject.start_action(),
                    serde_json::json!({
                        "operation_id": operation_id,
                        "references": template.references().iter().map(ToString::to_string).collect::<Vec<_>>(),
                        "reference_count": template.references().len(),
                    }),
                )
                .map_err(reveal_start_audit_error)?;

            let mut values = Vec::with_capacity(template.references().len());
            for reference in template.references() {
                match vault.secret_value(&reference.to_secret_name()) {
                    Ok(value) => values.push(value),
                    Err(error) => {
                        return Err(reveal_prepare_failure_unlocked(
                            self,
                            &vault,
                            RevealOperation::TemplateInject,
                            &operation_id,
                            "resolve",
                            error,
                        ));
                    }
                }
            }
            let value = match template.render(&values) {
                Ok(value) => value,
                Err(error) => {
                    let error = classified(error.kind(), error.message());
                    return Err(reveal_prepare_failure_unlocked(
                        self,
                        &vault,
                        RevealOperation::TemplateInject,
                        &operation_id,
                        "render",
                        error,
                    ));
                }
            };
            let OpenVault { audit_key, .. } = vault;
            Ok(PreparedReveal {
                lifecycle: RevealLifecycle {
                    store: self.clone(),
                    audit_key,
                    operation_id,
                    operation: RevealOperation::TemplateInject,
                },
                value,
            })
        })
        .map_err(|error| self.map_reveal_prepare_error(error))
    }

    fn map_reveal_prepare_error(&self, error: anyhow::Error) -> VaultError {
        if error.is::<ClassifiedVaultError>() {
            vault_error_from_anyhow(VaultErrorKind::Internal, error)
        } else {
            self.map_open_error(error)
        }
    }

    fn prepare_exec(&self, passphrase: &SecretString, request: VaultExec) -> Result<PreparedExec> {
        let operation_id = ulid::Ulid::new().to_string();
        self.with_lock(|| {
            let vault = self.open_unlocked(passphrase)?;
            vault
                .append_audit_unlocked(
                    self,
                    AuditAction::ExecStart,
                    exec_start_details(&request, &operation_id),
                )
                .map_err(exec_start_audit_error)?;

            let (command, env) = match resolve_exec_environment(&vault, request) {
                Ok(resolved) => resolved,
                Err(error) => {
                    return Err(exec_prepare_failure_unlocked(
                        self,
                        &vault,
                        &operation_id,
                        "resolve",
                        error,
                    ));
                }
            };
            let concealed_values = env
                .iter()
                .filter(|entry| entry.field_kind == Some(FieldKind::Concealed))
                .map(|entry| entry.value.as_bytes())
                .collect::<Vec<_>>();
            let redactor = match redactor_from_concealed_values(&concealed_values) {
                Ok(redactor) => redactor,
                Err(error) => {
                    return Err(exec_prepare_failure_unlocked(
                        self,
                        &vault,
                        &operation_id,
                        "redaction",
                        classified(error.kind(), error.message()),
                    ));
                }
            };
            let OpenVault { audit_key, .. } = vault;
            Ok(PreparedExec {
                store: self.clone(),
                audit_key,
                operation_id,
                command,
                env,
                redactor,
            })
        })
        .map_err(|error| {
            if error.is::<ClassifiedVaultError>() {
                vault_error_from_anyhow(VaultErrorKind::Internal, error)
            } else {
                self.map_open_error(error)
            }
        })
    }

    pub(crate) fn exec(
        &self,
        passphrase: &SecretString,
        request: VaultExec,
    ) -> Result<crate::ExecOutcome> {
        // Preparation returns only after releasing the vault lock. The direct
        // execute call owns every normal terminal audit attempt; an abort may
        // intentionally leave an unmatched start event.
        self.prepare_exec(passphrase, request)?.execute()
    }

    pub(crate) fn verify_audit(&self, passphrase: &SecretString) -> Result<AuditVerification> {
        self.with_lock(|| {
            let vault = self.open_unlocked(passphrase)?;
            vault.verify_audit_unlocked(self)
        })
        .map_err(|error| {
            if error.is::<ClassifiedVaultError>() {
                vault_error_from_anyhow(VaultErrorKind::Internal, error)
            } else {
                VaultError::from_anyhow(VaultErrorKind::AuditTampered, error)
            }
        })
    }

    /// Maps open-time failures while preserving classified vault errors.
    ///
    /// If a secondary `exists` probe fails, the public kind becomes `Io` but
    /// the original open failure remains the source for diagnostics.
    pub(crate) fn map_open_error(&self, error: anyhow::Error) -> VaultError {
        if error.is::<ClassifiedVaultError>() {
            return vault_error_from_anyhow(VaultErrorKind::Internal, error);
        }
        let default = match self.exists() {
            Ok(false) => VaultErrorKind::NotFound,
            Ok(true) => VaultErrorKind::Internal,
            // Preserve the original open failure as the source; this probe only refines the kind.
            Err(_) => VaultErrorKind::Io,
        };
        VaultError::from_anyhow(default, error)
    }

    fn open_unlocked(&self, passphrase: &SecretString) -> AnyResult<OpenVault> {
        let text = self.read_vault_text()?.ok_or_else(|| {
            classified(
                VaultErrorKind::NotFound,
                format!("vault does not exist at {}", self.vault_path().display()),
            )
        })?;
        let parsed = ParsedVaultEnvelope::parse(&text)?;
        let validated = parsed.validate()?;
        let unlocked = validated.unlock(passphrase)?;
        Ok(OpenVault::from_unlocked(unlocked))
    }
}

/// Validates a passphrase for new vault creation.
///
/// # Errors
///
/// Returns an error when the passphrase is shorter than
/// [`MIN_MASTER_PASSPHRASE_LEN`] bytes.
pub fn validate_new_vault_passphrase(passphrase: &SecretString) -> Result<()> {
    validate_new_vault_passphrase_inner(passphrase).map_err(|error| {
        VaultError::new(
            classified_kind(&error).unwrap_or(VaultErrorKind::InvalidInput),
            error.to_string(),
        )
    })
}

impl OpenVault {
    fn ensure_write_mode(
        &self,
        name: &SecretName,
        mode: VaultWriteMode,
        label: &str,
    ) -> AnyResult<()> {
        let exists = self.state.secrets.contains_key(name.as_str());
        match (mode, exists) {
            (VaultWriteMode::Create, true) => Err(classified(
                VaultErrorKind::AlreadyExists,
                format!("{label} '{}' already exists", name.as_str()),
            )),
            (VaultWriteMode::Replace, false) => Err(classified(
                VaultErrorKind::NotFound,
                format!("{label} '{}' no longer exists", name.as_str()),
            )),
            (VaultWriteMode::Create, false)
            | (VaultWriteMode::Replace, true)
            | (VaultWriteMode::Upsert, _) => Ok(()),
        }
    }

    pub(crate) fn list(&self) -> Vec<SecretRecord> {
        self.state
            .secrets
            .iter()
            .map(|(name, entry)| SecretRecord {
                name: name.clone(),
                created_at_ms: entry.created_at_ms,
                updated_at_ms: entry.updated_at_ms,
                value_len: entry.value_len,
            })
            .collect()
    }

    pub(crate) fn list_fields(&self) -> Vec<FieldRecord> {
        self.state
            .secrets
            .iter()
            .filter_map(|(name, entry)| {
                let name = SecretName::parse(name).ok()?;
                let reference = VaultReference::from_secret_name(&name)?;
                Some(FieldRecord {
                    reference,
                    kind: entry.kind,
                    created_at_ms: entry.created_at_ms,
                    updated_at_ms: entry.updated_at_ms,
                    value_len: entry.value_len,
                })
            })
            .collect()
    }

    fn snapshot(&self, audit: AuditVerification) -> VaultSnapshot {
        let legacy_secrets = self
            .state
            .secrets
            .iter()
            .filter_map(|(name, entry)| {
                let canonical = SecretName::parse(name)
                    .ok()
                    .and_then(|name| VaultReference::from_secret_name(&name));
                canonical.is_none().then(|| SecretRecord {
                    name: name.clone(),
                    created_at_ms: entry.created_at_ms,
                    updated_at_ms: entry.updated_at_ms,
                    value_len: entry.value_len,
                })
            })
            .collect();
        VaultSnapshot {
            vault_id: self.file.header.vault_id.clone(),
            revision: self.revision(),
            created_at_ms: self.file.header.created_at_ms,
            format_version: self.format_version(),
            fields: self.list_fields(),
            legacy_secrets,
            audit,
        }
    }

    fn format_version(&self) -> u32 {
        self.file.header.version
    }

    fn revision(&self) -> VaultRevision {
        VaultRevision {
            vault_id: self.file.header.vault_id.clone(),
            state_nonce_b64: self.file.state_nonce_b64.clone(),
        }
    }

    fn matches_revision(&self, revision: &VaultRevision) -> bool {
        self.file.header.vault_id == revision.vault_id
            && self.file.state_nonce_b64 == revision.state_nonce_b64
    }

    fn ensure_field_format_v2(&self) -> AnyResult<()> {
        if self.format_version() != FORMAT_VERSION {
            return Err(classified(
                VaultErrorKind::InvalidInput,
                format!(
                    "vault format {} does not support field mutations; run `jig vault migrate --to {FORMAT_VERSION}`",
                    self.format_version()
                ),
            ));
        }
        Ok(())
    }

    fn contains_field(&self, reference: &VaultReference) -> bool {
        self.field_kind(reference).is_some()
    }

    fn field_kind(&self, reference: &VaultReference) -> Option<FieldKind> {
        self.state
            .secrets
            .get(reference.to_secret_name().as_str())
            .map(|entry| entry.kind)
    }

    fn change_field_kind(
        &mut self,
        reference: VaultReference,
        kind: FieldKind,
    ) -> AnyResult<FieldKindChangeResult> {
        let name = reference.to_secret_name();
        let entry = self.state.secrets.get(name.as_str()).ok_or_else(|| {
            classified(
                VaultErrorKind::NotFound,
                format!("vault field '{reference}' does not exist"),
            )
        })?;
        validate_serialized_field_value_len(&name, entry)?;
        validate_field_value_len(kind, entry.value_len)?;
        let previous_kind = entry.kind;
        let changed = previous_kind != kind;
        if changed {
            let entry = self
                .state
                .secrets
                .get_mut(name.as_str())
                .expect("validated field must remain present");
            entry.kind = kind;
            entry.updated_at_ms = now_ms();
        }
        Ok(FieldKindChangeResult {
            reference,
            previous_kind,
            kind,
            changed,
        })
    }

    fn rename_field(
        &mut self,
        from: VaultReference,
        to: VaultReference,
    ) -> AnyResult<FieldBatchResult> {
        let from_name = from.to_secret_name();
        let to_name = to.to_secret_name();
        let entry = self.state.secrets.get(from_name.as_str()).ok_or_else(|| {
            classified(
                VaultErrorKind::NotFound,
                format!("vault field '{from}' does not exist"),
            )
        })?;
        validate_serialized_field_value_len(&from_name, entry)?;
        if self.state.secrets.contains_key(to_name.as_str()) {
            return Err(classified(
                VaultErrorKind::AlreadyExists,
                format!("vault field rename destination '{to}' already exists"),
            ));
        }
        let mut entry = self
            .state
            .secrets
            .remove(from_name.as_str())
            .expect("validated field must remain present");
        entry.updated_at_ms = now_ms();
        self.state
            .secrets
            .insert(to_name.as_str().to_owned(), entry);
        Ok(FieldBatchResult {
            changed: vec![to],
            removed: vec![from],
        })
    }

    fn rename_item(&mut self, from: VaultItem, to: VaultItem) -> AnyResult<FieldBatchResult> {
        let moves = self
            .list_fields()
            .into_iter()
            .filter(|field| field.reference.item() == from.as_str())
            .map(|field| {
                let target = VaultReference::parse(&format!(
                    "jig://{}/{}",
                    to.as_str(),
                    field.reference.field()
                ))
                .map_err(VaultError::into_classified_anyhow)?;
                Ok((field.reference, target))
            })
            .collect::<AnyResult<Vec<_>>>()?;
        if moves.is_empty() {
            return Err(classified(
                VaultErrorKind::NotFound,
                format!("vault item '{from}' does not exist"),
            ));
        }
        for (source, target) in &moves {
            let source_name = source.to_secret_name();
            let entry = self
                .state
                .secrets
                .get(source_name.as_str())
                .expect("listed field must remain present");
            validate_serialized_field_value_len(&source_name, entry)?;
            if self.contains_field(target) {
                return Err(classified(
                    VaultErrorKind::AlreadyExists,
                    format!("vault item rename destination field '{target}' already exists"),
                ));
            }
        }
        let now = now_ms();
        for (source, target) in &moves {
            let source_name = source.to_secret_name();
            let target_name = target.to_secret_name();
            let mut entry = self
                .state
                .secrets
                .remove(source_name.as_str())
                .expect("validated field must remain present");
            entry.updated_at_ms = now;
            self.state
                .secrets
                .insert(target_name.as_str().to_owned(), entry);
        }
        Ok(FieldBatchResult {
            changed: moves.iter().map(|(_, target)| target.clone()).collect(),
            removed: moves.into_iter().map(|(source, _)| source).collect(),
        })
    }

    fn remove_item(&mut self, item: VaultItem) -> AnyResult<FieldBatchResult> {
        let references = self
            .list_fields()
            .into_iter()
            .filter(|field| field.reference.item() == item.as_str())
            .map(|field| field.reference)
            .collect::<Vec<_>>();
        if references.is_empty() {
            return Err(classified(
                VaultErrorKind::NotFound,
                format!("vault item '{item}' does not exist"),
            ));
        }
        for reference in &references {
            let removed = self
                .state
                .secrets
                .remove(reference.to_secret_name().as_str());
            debug_assert!(removed.is_some());
        }
        Ok(FieldBatchResult {
            changed: Vec::new(),
            removed: references,
        })
    }

    fn convert_legacy_secret(
        &mut self,
        secret_name: SecretName,
        reference: VaultReference,
        kind: FieldKind,
    ) -> AnyResult<LegacyConversionResult> {
        let target_name = reference.to_secret_name();
        if self.state.secrets.contains_key(target_name.as_str()) {
            return Err(classified(
                VaultErrorKind::AlreadyExists,
                format!("vault field conversion destination '{reference}' already exists"),
            ));
        }
        let entry = self
            .state
            .secrets
            .get(secret_name.as_str())
            .ok_or_else(|| {
                classified(
                    VaultErrorKind::NotFound,
                    format!(
                        "legacy vault secret '{}' does not exist",
                        secret_name.as_str()
                    ),
                )
            })?;
        validate_serialized_field_value_len(&secret_name, entry)?;
        validate_field_value_len(kind, entry.value_len)?;
        let mut entry = self
            .state
            .secrets
            .remove(secret_name.as_str())
            .expect("validated legacy secret must remain present");
        entry.kind = kind;
        entry.updated_at_ms = now_ms();
        self.state
            .secrets
            .insert(target_name.as_str().to_owned(), entry);
        Ok(LegacyConversionResult {
            secret_name: secret_name.as_str().to_owned(),
            reference,
            kind,
        })
    }

    pub(crate) fn set_secret(&mut self, name: &SecretName, value: SecretBytes) -> AnyResult<()> {
        validate_secret_value_len(value.len())?;
        self.set_field_value_unchecked(name, FieldKind::Concealed, value);
        Ok(())
    }

    fn apply_validated_field_batch(&mut self, mutations: Vec<FieldMutation>) -> FieldBatchResult {
        let mut result = FieldBatchResult::default();
        for mutation in mutations {
            match mutation {
                FieldMutation::Set {
                    reference,
                    kind,
                    value,
                } => {
                    let name = reference.to_secret_name();
                    self.set_field_value_unchecked(&name, kind, value);
                    result.changed.push(reference);
                }
                FieldMutation::Remove { reference } => {
                    let name = reference.to_secret_name();
                    if self.remove_secret(&name) {
                        result.removed.push(reference);
                    }
                }
            }
        }
        result
    }

    fn set_field_value_unchecked(
        &mut self,
        name: &SecretName,
        kind: FieldKind,
        mut value: SecretBytes,
    ) {
        let now = now_ms();
        let created_at_ms = self
            .state
            .secrets
            .get(name.as_str())
            .map(|entry| entry.created_at_ms)
            .unwrap_or(now);
        let mut value_b64 = Zeroizing::new(String::with_capacity(padded_base64_len(value.len())));
        B64.encode_string(value.as_slice(), &mut value_b64);
        debug_assert_eq!(value_b64.capacity(), padded_base64_len(value.len()));
        let entry = SecretEntry {
            value_b64: std::mem::take(&mut *value_b64),
            value_len: value.len(),
            created_at_ms,
            updated_at_ms: now,
            kind,
        };
        value.zeroize();
        // Replaced entries are dropped here; `SecretEntry::drop` zeroizes the
        // displaced base64 value.
        self.state.secrets.insert(name.as_str().to_string(), entry);
    }

    pub(crate) fn remove_secret(&mut self, name: &SecretName) -> bool {
        self.state.secrets.remove(name.as_str()).is_some()
    }

    pub(crate) fn secret_value(&self, name: &SecretName) -> AnyResult<SecretBytes> {
        self.field_value(name).map(|(_, value)| value)
    }

    fn field_value(&self, name: &SecretName) -> AnyResult<(FieldKind, SecretBytes)> {
        let entry = self.state.secrets.get(name.as_str()).ok_or_else(|| {
            classified(
                VaultErrorKind::NotFound,
                format!("vault secret '{}' does not exist", name.as_str()),
            )
        })?;
        validate_serialized_field_value_len(name, entry)?;
        // `decoded_len_estimate` may overestimate by a couple of bytes; the
        // buffer starts zeroed and is truncated to the decoded length below.
        let mut value = SecretBytes::zeroed(base64::decoded_len_estimate(entry.value_b64.len()));
        let decoded_len = B64
            .decode_slice(entry.value_b64.as_bytes(), value.as_mut_slice())
            .map_err(|error| {
                classify_source(
                    VaultErrorKind::Serialization,
                    format!("vault secret '{}' value is not valid base64", name.as_str()),
                    error.into(),
                )
            })?;
        value.truncate(decoded_len);
        if value.len() != entry.value_len {
            return Err(classified(
                VaultErrorKind::Serialization,
                format!(
                    "vault secret '{}' value length metadata is invalid",
                    name.as_str()
                ),
            ));
        }
        Ok((entry.kind, value))
    }

    fn prepare_save_unlocked(&self) -> AnyResult<ResealedVaultEnvelope> {
        ResealedVaultEnvelope::seal(&self.file, &self.dek, &self.state)
    }

    pub(crate) fn append_audit(
        &self,
        store: &VaultStore,
        action: AuditAction,
        details: serde_json::Value,
    ) -> AnyResult<AuditEvent> {
        AuditEvent::append(store, self.audit_key.as_ref(), action, details)
    }

    pub(crate) fn append_audit_unlocked(
        &self,
        store: &VaultStore,
        action: AuditAction,
        details: serde_json::Value,
    ) -> AnyResult<AuditEvent> {
        AuditEvent::append_unlocked(store, self.audit_key.as_ref(), action, details)
    }

    pub(crate) fn verify_audit_unlocked(&self, store: &VaultStore) -> AnyResult<AuditVerification> {
        verify_chain_unlocked(store, self.audit_key.as_ref())
    }
}

fn exec_start_details(request: &VaultExec, operation_id: &str) -> serde_json::Value {
    let variables = request
        .bindings()
        .iter()
        .map(|binding| binding.var().as_str())
        .collect::<Vec<_>>();
    let field_bindings = request
        .bindings()
        .iter()
        .filter_map(|binding| match binding.value() {
            ExecEnvValue::Literal(_) => None,
            ExecEnvValue::Field(reference) => Some(serde_json::json!({
                "var": binding.var().as_str(),
                "reference": reference.to_string(),
            })),
        })
        .collect::<Vec<_>>();
    let field_binding_count = field_bindings.len();
    serde_json::json!({
        "operation_id": operation_id,
        "argument_count": request.command_len(),
        "binding_count": request.bindings().len(),
        "literal_binding_count": request.bindings().len() - field_binding_count,
        "field_binding_count": field_binding_count,
        "variables": variables,
        "field_bindings": field_bindings,
    })
}

fn exec_start_audit_error(error: anyhow::Error) -> anyhow::Error {
    classify_source(
        VaultErrorKind::AuditTampered,
        "failed to append vault exec start audit event",
        error,
    )
}

fn exec_prepare_failure_unlocked(
    store: &VaultStore,
    vault: &OpenVault,
    operation_id: &str,
    stage: &str,
    error: anyhow::Error,
) -> anyhow::Error {
    let kind = classified_kind(&error).unwrap_or(VaultErrorKind::Internal);
    match vault.append_audit_unlocked(
        store,
        AuditAction::ExecFailed,
        exec_failure_details(operation_id, stage),
    ) {
        Ok(_) => error,
        Err(audit_error) => classify_source(
            kind,
            "vault exec preparation failed; additionally failed to append failure audit event",
            error.context(format!(
                "additional audit failure while recording vault exec failure: {audit_error}"
            )),
        ),
    }
}

fn exec_failure_details(operation_id: &str, stage: &str) -> serde_json::Value {
    serde_json::json!({
        "operation_id": operation_id,
        "stage": stage,
        // Values, argv, and raw errors never belong in the local audit chain.
        "error": "vault exec failed",
    })
}

fn record_exec_finish(
    store: &VaultStore,
    audit_key: &[u8],
    operation_id: &str,
    outcome: &crate::ExecOutcome,
) -> AnyResult<()> {
    AuditEvent::append(
        store,
        audit_key,
        AuditAction::ExecFinish,
        serde_json::json!({
            "operation_id": operation_id,
            "exit_status": outcome.exit_status,
            "exit_signal": outcome.exit_signal,
        }),
    )?;
    Ok(())
}

fn record_exec_failure(
    store: &VaultStore,
    audit_key: &[u8],
    operation_id: &str,
    stage: &str,
) -> AnyResult<()> {
    AuditEvent::append(
        store,
        audit_key,
        AuditAction::ExecFailed,
        exec_failure_details(operation_id, stage),
    )?;
    Ok(())
}

fn resolve_exec_environment(
    vault: &OpenVault,
    request: VaultExec,
) -> AnyResult<(Vec<OsString>, Vec<ResolvedExecEnv>)> {
    let (command, bindings) = request.into_parts();
    let mut env = Vec::with_capacity(bindings.len());
    let mut total_bytes = 0_usize;
    for binding in bindings {
        let (var, source) = binding.into_parts();
        let (field_kind, value, reference) = match source {
            ExecEnvValue::Literal(value) => (None, value, None),
            ExecEnvValue::Field(reference) => {
                let (kind, value) = vault.field_value(&reference.to_secret_name())?;
                (Some(kind), value, Some(reference))
            }
        };
        if value.len() > MAX_EXEC_ENV_VALUE_LEN {
            return Err(classified(
                VaultErrorKind::InvalidInput,
                format!(
                    "vault exec environment value for {} exceeds the {MAX_EXEC_ENV_VALUE_LEN} byte limit",
                    var.as_str()
                ),
            ));
        }
        total_bytes = total_bytes.checked_add(value.len()).ok_or_else(|| {
            classified(
                VaultErrorKind::InvalidInput,
                "vault exec resolved environment data exceeds supported bounds",
            )
        })?;
        if total_bytes > MAX_EXEC_ENV_TOTAL_BYTES {
            return Err(classified(
                VaultErrorKind::InvalidInput,
                format!(
                    "vault exec resolved environment data exceeds the {MAX_EXEC_ENV_TOTAL_BYTES} byte total limit"
                ),
            ));
        }
        let value = value.into_zeroizing_string().map_err(|_| {
            classified(
                VaultErrorKind::InvalidInput,
                invalid_exec_field_value_message(&var, reference.as_ref(), "must be valid UTF-8"),
            )
        })?;
        if value.as_bytes().contains(&0) {
            return Err(classified(
                VaultErrorKind::InvalidInput,
                invalid_exec_field_value_message(&var, reference.as_ref(), "must not contain NUL"),
            ));
        }
        env.push(ResolvedExecEnv {
            var,
            value,
            field_kind,
        });
    }
    Ok((command, env))
}

fn invalid_exec_field_value_message(
    var: &EnvVarName,
    reference: Option<&VaultReference>,
    requirement: &str,
) -> String {
    match reference {
        Some(reference) => format!(
            "vault exec field {reference} for {} {requirement}",
            var.as_str()
        ),
        None => format!(
            "vault exec literal environment value for {} {requirement}",
            var.as_str()
        ),
    }
}

fn reveal_start_audit_error(error: anyhow::Error) -> anyhow::Error {
    classify_source(
        VaultErrorKind::AuditTampered,
        "failed to append reveal start audit event",
        error,
    )
}

fn reveal_prepare_failure_unlocked(
    store: &VaultStore,
    vault: &OpenVault,
    operation: RevealOperation,
    operation_id: &str,
    stage: &str,
    error: anyhow::Error,
) -> anyhow::Error {
    let kind = classified_kind(&error).unwrap_or(VaultErrorKind::Internal);
    match vault.append_audit_unlocked(
        store,
        operation.failed_action(),
        reveal_failure_details(operation_id, stage),
    ) {
        Ok(_) => error,
        Err(audit_error) => classify_source(
            kind,
            format!(
                "{} preparation failed; additionally failed to append failure audit event",
                operation.label()
            ),
            error.context(format!(
                "additional audit failure while recording reveal failure: {audit_error}"
            )),
        ),
    }
}

fn reveal_failure_details(operation_id: &str, stage: &str) -> serde_json::Value {
    serde_json::json!({
        "operation_id": operation_id,
        "stage": stage,
        // Values and raw errors never belong in the local audit chain.
        "error": "vault reveal operation failed",
    })
}

fn now_ms() -> i128 {
    OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000
}

fn validate_field_mutations(mutations: &[FieldMutation]) -> AnyResult<()> {
    if mutations.is_empty() {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            "field batch must contain at least one mutation",
        ));
    }
    let mut references = BTreeSet::new();
    for mutation in mutations {
        let reference = mutation.reference();
        if !references.insert(reference) {
            return Err(classified(
                VaultErrorKind::InvalidInput,
                format!("field batch contains duplicate reference '{reference}'"),
            ));
        }
        if let FieldMutation::Set { kind, value, .. } = mutation {
            validate_field_value_len(*kind, value.len())?;
        }
    }
    Ok(())
}

fn validate_import_references(references: &[VaultReference]) -> AnyResult<()> {
    if references.is_empty() {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            "onepassword import must contain at least one field reference",
        ));
    }
    if references.len() > MAX_IMPORT_FIELDS {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            format!("onepassword import exceeds the {MAX_IMPORT_FIELDS} field limit"),
        ));
    }
    let mut unique = BTreeSet::new();
    for reference in references {
        if !unique.insert(reference) {
            return Err(classified(
                VaultErrorKind::InvalidInput,
                format!("onepassword import contains duplicate reference '{reference}'"),
            ));
        }
    }
    Ok(())
}

fn validate_import_mutations(mutations: &[FieldMutation]) -> AnyResult<()> {
    validate_field_mutations(mutations)?;
    if mutations.len() > MAX_IMPORT_FIELDS {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            format!("onepassword import exceeds the {MAX_IMPORT_FIELDS} field limit"),
        ));
    }
    let mut total_value_bytes = 0_usize;
    for mutation in mutations {
        let FieldMutation::Set { value, .. } = mutation else {
            return Err(classified(
                VaultErrorKind::InvalidInput,
                "onepassword import accepts only field set mutations",
            ));
        };
        total_value_bytes = total_value_bytes.checked_add(value.len()).ok_or_else(|| {
            classified(
                VaultErrorKind::InvalidInput,
                "onepassword import value bytes exceed supported bounds",
            )
        })?;
        if total_value_bytes > MAX_IMPORT_VALUE_BYTES {
            return Err(classified(
                VaultErrorKind::InvalidInput,
                format!(
                    "onepassword import exceeds the {MAX_IMPORT_VALUE_BYTES} byte decoded value limit"
                ),
            ));
        }
    }
    Ok(())
}

fn validate_import_precondition(
    precondition: &VaultImportPrecondition,
    fields: &[(VaultReference, FieldKind)],
) -> AnyResult<()> {
    if precondition.fields.len() != fields.len()
        || precondition
            .fields
            .iter()
            .zip(fields)
            .any(|(planned, (reference, _))| planned.reference != *reference)
    {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            "vault import mutations do not match the previewed field set",
        ));
    }
    Ok(())
}

fn enforce_import_precondition(
    vault: &OpenVault,
    precondition: &VaultImportPrecondition,
    replace: bool,
) -> AnyResult<()> {
    VaultEditPrecondition::Revision(&precondition.revision).enforce(
        vault,
        "vault state changed since the import preview; preview again",
    )?;
    if !replace {
        if let Some(field) = precondition
            .fields
            .iter()
            .find(|field| field.previous_kind.is_some())
        {
            return Err(classified(
                VaultErrorKind::AlreadyExists,
                format!(
                    "vault field '{}' already exists; enable replacement and preview again",
                    field.reference
                ),
            ));
        }
    }
    Ok(())
}

fn reject_import_collisions(
    vault: &OpenVault,
    fields: &[(VaultReference, FieldKind)],
) -> AnyResult<()> {
    let collisions = fields
        .iter()
        .filter(|(reference, _)| vault.contains_field(reference))
        .map(|(reference, _)| reference.to_string())
        .collect::<Vec<_>>();
    if collisions.is_empty() {
        return Ok(());
    }
    Err(classified(
        VaultErrorKind::AlreadyExists,
        format!(
            "onepassword import would replace existing fields without --replace: {}",
            collisions.join(", ")
        ),
    ))
}

fn field_batch_set_audit_metadata(mutations: &[FieldMutation]) -> Vec<(VaultReference, FieldKind)> {
    mutations
        .iter()
        .filter_map(|mutation| match mutation {
            FieldMutation::Set {
                reference, kind, ..
            } => Some((reference.clone(), *kind)),
            FieldMutation::Remove { .. } => None,
        })
        .collect()
}

fn field_batch_remove_audit_metadata(mutations: &[FieldMutation]) -> Vec<VaultReference> {
    mutations
        .iter()
        .filter_map(|mutation| match mutation {
            FieldMutation::Set { .. } => None,
            FieldMutation::Remove { reference } => Some(reference.clone()),
        })
        .collect()
}

fn field_batch_audit_details(
    sets: &[(VaultReference, FieldKind)],
    removes: &[VaultReference],
    result: &FieldBatchResult,
) -> serde_json::Value {
    serde_json::json!({
        "set": sets.iter().map(|(reference, kind)| serde_json::json!({
            "reference": reference.to_string(),
            "kind": kind.as_str(),
        })).collect::<Vec<_>>(),
        "remove": removes.iter().map(|reference| serde_json::json!({
            "reference": reference.to_string(),
            "removed": result.removed.contains(reference),
        })).collect::<Vec<_>>(),
    })
}

fn onepassword_import_audit_details(fields: &[(VaultReference, FieldKind)]) -> serde_json::Value {
    let concealed_count = fields
        .iter()
        .filter(|(_, kind)| *kind == FieldKind::Concealed)
        .count();
    serde_json::json!({
        "field_count": fields.len(),
        "concealed_count": concealed_count,
        "text_count": fields.len() - concealed_count,
        "fields": fields.iter().map(|(reference, kind)| serde_json::json!({
            "reference": reference.to_string(),
            "kind": kind.as_str(),
        })).collect::<Vec<_>>(),
    })
}

const fn padded_base64_len(len: usize) -> usize {
    len.div_ceil(3) * 4
}

fn validate_secret_value_len(len: usize) -> AnyResult<()> {
    if len < MIN_REDACTABLE_LEN {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            "secret value must be at least 4 bytes so redaction can match it safely",
        ));
    }
    if len > MAX_SECRET_VALUE_LEN {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            format!("secret value must be at most {MAX_SECRET_VALUE_LEN} bytes"),
        ));
    }
    Ok(())
}

fn validate_field_value_len(kind: FieldKind, len: usize) -> AnyResult<()> {
    if kind == FieldKind::Concealed {
        return validate_secret_value_len(len);
    }
    if len > MAX_SECRET_VALUE_LEN {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            format!("field value must be at most {MAX_SECRET_VALUE_LEN} bytes"),
        ));
    }
    Ok(())
}

fn validate_serialized_field_value_len(name: &SecretName, entry: &SecretEntry) -> AnyResult<()> {
    let too_short = entry.kind == FieldKind::Concealed && entry.value_len < MIN_REDACTABLE_LEN;
    if too_short || entry.value_len > MAX_SECRET_VALUE_LEN {
        return Err(classified(
            VaultErrorKind::Serialization,
            format!(
                "vault secret '{}' value length metadata is outside supported bounds",
                name.as_str()
            ),
        ));
    }
    if entry.value_b64.len() > padded_base64_len(MAX_SECRET_VALUE_LEN) {
        return Err(classified(
            VaultErrorKind::Serialization,
            format!(
                "vault secret '{}' encoded value is outside supported bounds",
                name.as_str()
            ),
        ));
    }
    Ok(())
}

fn validate_new_vault_passphrase_inner(passphrase: &SecretString) -> AnyResult<()> {
    if passphrase.expose_secret().len() < MIN_MASTER_PASSPHRASE_LEN {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            format!("vault passphrase must be at least {MIN_MASTER_PASSPHRASE_LEN} bytes"),
        ));
    }
    Ok(())
}

fn rollback_failed_init(store: &VaultStore) -> Option<String> {
    let mut failures = Vec::new();
    for path in [store.vault_path(), store.audit_path()] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => failures.push(format!("failed to remove {}: {error}", path.display())),
        }
    }
    if failures.is_empty() {
        None
    } else {
        Some(format!(
            "vault init rollback left partial state; inspect or remove {} and {} before retrying: {}",
            store.vault_path().display(),
            store.audit_path().display(),
            failures.join("; ")
        ))
    }
}

#[cfg(test)]
// Keep the broad vault facade tests out of this already-central module body.
#[path = "vault_tests.rs"]
mod tests;
