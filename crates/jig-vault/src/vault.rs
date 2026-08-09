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

use crate::audit::{AuditAction, AuditEvent, AuditVerification, verify_chain_unlocked};
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
use crate::output::{OutputInstallFailure, install_private_bytes};
use crate::redact::MIN_REDACTABLE_LEN;
use crate::run::{
    ResolvedBrokeredEnv, ResolvedBrokeredFile, ResolvedBrokeredRun, RunOutput, run_brokered,
};
use crate::store::VaultStore;
use crate::template::InjectionTemplate;
use crate::types::{EnvVarName, FieldKind, SecretName, VaultReference};
use crate::{Result, SecretBytes, VaultError, VaultErrorKind};

mod envelope;

use envelope::{
    MigratedVaultEnvelope, NewVaultEnvelope, ParsedVaultEnvelope, ResealedVaultEnvelope,
    UnlockedVaultEnvelope,
};

pub const MAX_SECRET_VALUE_LEN: usize = 1024 * 1024;
pub const MIN_MASTER_PASSPHRASE_LEN: usize = 12;

#[derive(Clone, Debug)]
pub struct Vault {
    store: VaultStore,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultStatus {
    pub root: PathBuf,
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
        let (root, exists) = VaultStore::inspect(explicit_home)?;
        Ok(VaultStatus { root, exists })
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
        self.store.set_secret(passphrase, name, value)
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
        self.store
            .apply_field_batch(passphrase, vec![FieldMutation::set(reference, kind, value)])
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

/// Metadata-only result of a completed controlled reveal operation.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevealResult {
    pub bytes_written: usize,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrokeredRunStage {
    Resolve,
    Process,
}

impl BrokeredRunStage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Resolve => "resolve",
            Self::Process => "process",
        }
    }
}

struct StartedBrokeredRun {
    // Holds the opened vault, including DEK/audit key material, until the
    // brokered child finishes or fails so the matching audit event can be
    // appended after the vault lock is released for the child lifetime.
    vault: OpenVault,
    run_id: String,
}

struct PreparedBrokeredRun {
    started: StartedBrokeredRun,
    resolved: ResolvedBrokeredRun,
}

impl StartedBrokeredRun {
    fn resolve_unlocked(
        self,
        store: &VaultStore,
        request: BrokeredRun,
    ) -> AnyResult<PreparedBrokeredRun> {
        let resolved = match resolve_brokered_run(&self.vault, request) {
            Ok(resolved) => resolved,
            Err(error) => {
                return Err(self.failure_error_unlocked(store, BrokeredRunStage::Resolve, error));
            }
        };
        Ok(PreparedBrokeredRun {
            started: self,
            resolved,
        })
    }

    fn record_finish(&self, store: &VaultStore, output: &RunOutput) -> AnyResult<()> {
        self.vault.append_audit(
            store,
            AuditAction::BrokeredRunFinish,
            serde_json::json!({
                "run_id": self.run_id,
                "exit_status": output.exit_status,
                "exit_signal": output.exit_signal,
            }),
        )?;
        Ok(())
    }

    fn failure_error(
        &self,
        store: &VaultStore,
        stage: BrokeredRunStage,
        kind: VaultErrorKind,
        error: anyhow::Error,
    ) -> VaultError {
        if let Err(audit_error) = self.record_failure(store, stage) {
            return VaultError::from_anyhow(
                kind,
                error.context(format!(
                    "brokered run failed; additionally failed to append failure audit event: {audit_error}"
                )),
            );
        }
        VaultError::from_anyhow(kind, error)
    }

    fn record_failure(&self, store: &VaultStore, stage: BrokeredRunStage) -> AnyResult<()> {
        self.vault.append_audit(
            store,
            AuditAction::BrokeredRunFailed,
            brokered_run_failure_details(&self.run_id, stage),
        )?;
        Ok(())
    }

    fn failure_error_unlocked(
        &self,
        store: &VaultStore,
        stage: BrokeredRunStage,
        error: anyhow::Error,
    ) -> anyhow::Error {
        let kind = classified_kind(&error).unwrap_or(VaultErrorKind::Internal);
        if let Err(audit_error) = self.record_failure_unlocked(store, stage) {
            return classify_source(
                kind,
                "brokered run failed; additionally failed to append failure audit event",
                error.context(format!(
                    "additional audit failure while recording brokered run failure: {audit_error}"
                )),
            );
        }
        error
    }

    fn record_failure_unlocked(
        &self,
        store: &VaultStore,
        stage: BrokeredRunStage,
    ) -> AnyResult<()> {
        self.vault.append_audit_unlocked(
            store,
            AuditAction::BrokeredRunFailed,
            brokered_run_failure_details(&self.run_id, stage),
        )?;
        Ok(())
    }
}

impl PreparedBrokeredRun {
    fn execute(self, store: &VaultStore) -> Result<RunOutput> {
        let Self { started, resolved } = self;
        match run_brokered(resolved) {
            Ok(output) => {
                started.record_finish(store, &output).map_err(|error| {
                    VaultError::from_anyhow(VaultErrorKind::AuditTampered, error)
                })?;
                Ok(output)
            }
            Err(error) => Err(started.failure_error(
                store,
                BrokeredRunStage::Process,
                VaultErrorKind::Process,
                error,
            )),
        }
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

        let envelope = NewVaultEnvelope::seal(passphrase, now_ms())?;
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
        self.with_lock(|| {
            let mut vault = self.open_unlocked(passphrase)?;
            verify_chain_unlocked(self, vault.audit_key.as_ref()).map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "vault audit chain verification failed",
                    error,
                )
            })?;
            let result = edit(&mut vault)?;
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

    pub(crate) fn set_secret(
        &self,
        passphrase: &SecretString,
        name: &str,
        value: SecretBytes,
    ) -> Result<()> {
        let name = SecretName::parse(name)?;
        self.set_secret_inner(passphrase, name, value)
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    fn set_secret_inner(
        &self,
        passphrase: &SecretString,
        name: SecretName,
        value: SecretBytes,
    ) -> AnyResult<()> {
        // Reject too-short values before unlocking; `OpenVault::set_secret`
        // repeats this guard for internal callers that already hold a handle.
        validate_secret_value_len(value.len())?;
        self.edit_with_audit(
            passphrase,
            AuditAction::SecretSet,
            |vault| vault.set_secret(&name, value),
            |()| {
                serde_json::json!({
                    "secret_name": name.as_str(),
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

    fn prepare_brokered_run(
        &self,
        passphrase: &SecretString,
        request: BrokeredRun,
    ) -> AnyResult<PreparedBrokeredRun> {
        let run_id = ulid::Ulid::new().to_string();
        self.with_lock(|| {
            let vault = self.open_unlocked(passphrase)?;
            let start_details = brokered_run_start_details(&request, &run_id);
            vault
                .append_audit_unlocked(self, AuditAction::BrokeredRunStart, start_details)
                .map_err(|error| {
                    classify_source(
                        VaultErrorKind::AuditTampered,
                        "failed to append brokered run start audit event",
                        error,
                    )
                })?;
            StartedBrokeredRun { vault, run_id }.resolve_unlocked(self, request)
        })
    }

    pub(crate) fn run_brokered(
        &self,
        passphrase: &SecretString,
        request: BrokeredRun,
    ) -> Result<RunOutput> {
        let prepared = self
            .prepare_brokered_run(passphrase, request)
            .map_err(|error| {
                if error.is::<ClassifiedVaultError>() {
                    // Classified errors already carry their public kind; the default
                    // only applies if a future classified source omits one.
                    vault_error_from_anyhow(VaultErrorKind::Internal, error)
                } else {
                    self.map_open_error(error)
                }
            })?;
        // Preparation returns only after the vault lock is released. The
        // prepared state owns normal terminal audit recording, while abrupt
        // process termination intentionally may leave its start unmatched.
        prepared.execute(self)
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

    fn format_version(&self) -> u32 {
        self.file.header.version
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

fn brokered_run_start_details(request: &BrokeredRun, run_id: &str) -> serde_json::Value {
    serde_json::json!({
        "run_id": run_id,
        "env": request.env().iter().map(|mapping| serde_json::json!({
            "var": mapping.var().as_str(),
            "secret_name": mapping.secret_name().as_str(),
        })).collect::<Vec<_>>(),
        "files": request.files().iter().map(|mapping| serde_json::json!({
            "var": mapping.var().as_str(),
            "secret_name": mapping.secret_name().as_str(),
        })).collect::<Vec<_>>(),
    })
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

fn brokered_run_failure_details(run_id: &str, stage: BrokeredRunStage) -> serde_json::Value {
    serde_json::json!({
        "run_id": run_id,
        "stage": stage.as_str(),
        // Do not record the original error text here. Spawn/process errors can
        // include argv or paths, and audit logs are value-free metadata.
        "error": "brokered run failed",
    })
}

fn resolve_brokered_run(vault: &OpenVault, request: BrokeredRun) -> AnyResult<ResolvedBrokeredRun> {
    let (command, env_mappings, file_mappings) = request.into_parts();
    let mut env = Vec::with_capacity(env_mappings.len());
    for mapping in env_mappings {
        let (var, secret_name) = mapping.into_parts();
        let value = vault.secret_value(&secret_name)?;
        env.push(ResolvedBrokeredEnv {
            var,
            secret_name,
            value,
        });
    }
    let mut files = Vec::with_capacity(file_mappings.len());
    for mapping in file_mappings {
        let (var, secret_name) = mapping.into_parts();
        let value = vault.secret_value(&secret_name)?;
        files.push(ResolvedBrokeredFile {
            var,
            secret_name,
            value,
        });
    }
    Ok(ResolvedBrokeredRun {
        command,
        env,
        files,
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
