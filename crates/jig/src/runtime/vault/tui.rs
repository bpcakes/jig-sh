use std::{
    io::Write,
    sync::{Mutex, MutexGuard},
};

use anyhow::Result;
#[cfg(all(unix, not(test)))]
use anyhow::anyhow;
use jig_vault::{
    PreparedPrivateFile, PrivateFilePrecondition, SecretBytes, Vault, VaultError, VaultErrorKind,
    VaultHomeState, VaultImportPrecondition, VaultReference, VaultRevision, VaultSnapshot,
    validate_new_vault_passphrase,
};
use jig_vault_tui::{
    ImportFieldChange, ImportPlanToken, ImportPreview, ImportPreviewAuthorization,
    ImportPreviewRow, VaultAction, VaultActionResult, VaultBackend, VaultCommittedAction,
    VaultDescriptor, VaultMutation, VaultUiError, VaultUiErrorKind,
};
use secrecy::SecretString;

use crate::command::{VaultImportEnvironment, VaultTuiRequest};

use super::{ResolvedVaultRuntime, resolve_vault_runtime, vault};

pub(crate) fn run(request: VaultTuiRequest, initial_passphrase: Option<SecretBytes>) -> Result<()> {
    let backend = VaultTuiBackend::new(request)?;
    #[cfg(all(unix, not(test)))]
    {
        let signal_session = crate::doctor::DoctorSignalSession::start().map_err(|_| {
            anyhow!(
                "Vault TUI was not started because the process-wide signal session is unavailable"
            )
        })?;
        let cancellation = signal_session.cancellation();
        let result = jig_vault_tui::run(backend, initial_passphrase, move || {
            cancellation.cancelled()
        });
        crate::codex::finish_signal_supervised(
            result,
            signal_session.finish(),
            "Vault TUI signal supervision could not retire safely",
        )
    }
    #[cfg(any(not(unix), test))]
    {
        jig_vault_tui::run(backend, initial_passphrase, || false)
    }
}

struct VaultTuiBackend {
    resolved: ResolvedVaultRuntime,
    descriptor: VaultDescriptor,
    session: Mutex<VaultTuiSession>,
}

#[derive(Default)]
struct VaultTuiSession {
    credential: Option<SecretString>,
    pending_import: Option<PendingImportPlan>,
}

impl VaultTuiSession {
    fn erase(&mut self) {
        self.credential = None;
        self.pending_import = None;
    }

    fn take_pending_import(
        &mut self,
        token: &ImportPlanToken,
    ) -> std::result::Result<PendingImportPlan, VaultUiError> {
        let matches = self
            .pending_import
            .as_ref()
            .is_some_and(|pending| pending.token == *token);
        if !matches {
            return Err(VaultUiError::new(
                VaultUiErrorKind::Conflict,
                "The 1Password import preview expired or was already used; preview again.",
            ));
        }
        Ok(self
            .pending_import
            .take()
            .expect("matching pending import remains installed"))
    }
}

struct PendingImportPlan {
    token: ImportPlanToken,
    environment: VaultImportEnvironment,
    vault: VaultImportPrecondition,
    destination: PrivateFilePrecondition,
    out_env: std::path::PathBuf,
    recovery_command: String,
}

struct OnePasswordPreviewRequest {
    env_file: std::path::PathBuf,
    item: jig_vault::VaultItem,
    out_env: std::path::PathBuf,
    replace: bool,
    overwrite: bool,
    dry_run: bool,
}

impl VaultTuiBackend {
    fn new(request: VaultTuiRequest) -> Result<Self> {
        let resolved = resolve_vault_runtime(&request.vault)?;
        // `Vault::status` is deliberately non-creating, so an absent target
        // remains truly absent for the future restore flow.
        let status = Vault::status(resolved.home.clone())?;
        let descriptor = VaultDescriptor {
            scope: resolved.scope.to_owned(),
            scope_id: resolved.scope_id.clone(),
            repo_name: resolved.repo_name.clone(),
            home: status.root,
            home_state: status.home_state,
        };
        Ok(Self {
            resolved,
            descriptor,
            session: Mutex::new(VaultTuiSession::default()),
        })
    }

    fn session(&self) -> std::result::Result<MutexGuard<'_, VaultTuiSession>, VaultUiError> {
        self.session.lock().map_err(|_| {
            VaultUiError::new(
                VaultUiErrorKind::Other,
                "Vault TUI credential state is unavailable.",
            )
        })
    }

    fn passphrase_from_bytes(
        bytes: SecretBytes,
    ) -> std::result::Result<SecretString, VaultUiError> {
        bytes.into_secret_string().map_err(|_| {
            VaultUiError::new(
                VaultUiErrorKind::InvalidInput,
                "Vault passphrases must be valid UTF-8.",
            )
        })
    }

    fn with_vault<T>(
        &self,
        operation: impl FnOnce(&Vault, &SecretString) -> jig_vault::Result<T>,
    ) -> std::result::Result<T, VaultUiError> {
        // Holding this mutex for the complete core call makes `lock()` wait for
        // an in-flight non-cancellable operation before dropping credentials.
        let session = self.session()?;
        let passphrase = session.credential.as_ref().ok_or_else(|| {
            VaultUiError::new(VaultUiErrorKind::Authentication, "Vault is locked.")
        })?;
        let selected = vault(&self.resolved).map_err(map_anyhow_error)?;
        operation(&selected, passphrase).map_err(map_vault_error)
    }

    fn snapshot(&self) -> std::result::Result<VaultSnapshot, VaultUiError> {
        self.with_vault(Vault::snapshot)
    }

    fn finish_committed(&self, action: VaultCommittedAction) -> VaultActionResult {
        committed_action_result(action, self.refresh())
    }

    fn clear_pending_import(&self) -> std::result::Result<(), VaultUiError> {
        self.session()?.pending_import = None;
        Ok(())
    }

    fn retain_pending_import(
        &self,
        plan: PendingImportPlan,
    ) -> std::result::Result<(), VaultUiError> {
        let mut session = self.session()?;
        if session.credential.is_none() {
            return Err(VaultUiError::new(
                VaultUiErrorKind::Authentication,
                "Vault is locked.",
            ));
        }
        session.pending_import = Some(plan);
        Ok(())
    }

    fn take_pending_import(
        &self,
        token: &ImportPlanToken,
    ) -> std::result::Result<PendingImportPlan, VaultUiError> {
        self.session()?.take_pending_import(token)
    }

    fn discard_pending_import(
        &self,
        token: &ImportPlanToken,
    ) -> std::result::Result<(), VaultUiError> {
        self.take_pending_import(token).map(drop)
    }

    fn preview_onepassword_import(
        &self,
        request: OnePasswordPreviewRequest,
        cancelled: &dyn Fn() -> bool,
    ) -> std::result::Result<ImportPreview, VaultUiError> {
        let OnePasswordPreviewRequest {
            env_file,
            item,
            out_env,
            replace,
            overwrite,
            dry_run,
        } = request;
        self.clear_pending_import()?;
        ensure_operation_active(cancelled)?;
        let environment = super::super::vault_env::parse_onepassword_env_file_with_cancellation(
            &env_file, &item, cancelled,
        )
        .map_err(map_anyhow_error)?;
        ensure_operation_active(cancelled)?;
        let entries = super::super::vault_import::import_entries(&environment);
        let references = entries
            .iter()
            .map(|entry| entry.reference.clone())
            .collect::<Vec<_>>();
        super::super::vault_import::preflight_destination(&out_env).map_err(map_anyhow_error)?;
        let recovery_command = super::super::vault_import::recovery_command(
            &env_file,
            &item,
            &out_env,
            &self.descriptor.home,
        )
        .map_err(map_anyhow_error)?;
        ensure_operation_active(cancelled)?;
        let (destination, vault) = self.with_vault(|selected, passphrase| {
            let destination = selected.preview_private_output(&out_env)?;
            let vault = selected.plan_import_fields(passphrase, &references)?;
            Ok((destination, vault))
        })?;
        ensure_operation_active(cancelled)?;
        let destination_exists = destination.destination_exists();
        let rows = entries
            .into_iter()
            .zip(vault.fields_with_previous_kinds())
            .map(|(entry, (planned_reference, previous_kind))| {
                debug_assert_eq!(&entry.reference, planned_reference);
                ImportPreviewRow {
                    variable: entry.name,
                    reference: entry.reference,
                    change: ImportFieldChange::from_previous_kind(previous_kind, entry.kind),
                }
            })
            .collect();
        let authorization = if dry_run {
            ImportPreviewAuthorization::DryRun
        } else {
            let token = ImportPlanToken::generate();
            self.retain_pending_import(PendingImportPlan {
                token: token.clone(),
                environment,
                vault,
                destination,
                out_env: out_env.clone(),
                recovery_command,
            })?;
            ImportPreviewAuthorization::Commit(token)
        };
        Ok(ImportPreview {
            env_file,
            item,
            out_env,
            replace,
            overwrite,
            authorization,
            rows,
            destination_exists,
        })
    }

    fn commit_onepassword_import(
        &self,
        token: ImportPlanToken,
        replace: bool,
        overwrite: bool,
        cancelled: &dyn Fn() -> bool,
    ) -> std::result::Result<VaultActionResult, VaultUiError> {
        ensure_operation_active(cancelled)?;
        let PendingImportPlan {
            token: _,
            environment,
            vault,
            destination,
            out_env,
            recovery_command,
        } = self.take_pending_import(&token)?;
        let entries = super::super::vault_import::import_entries(&environment);
        if destination.destination_exists() && !overwrite {
            return Err(VaultUiError::new(
                VaultUiErrorKind::Conflict,
                format!(
                    "Vault import destination {} already exists; enable Overwrite to replace it atomically.",
                    out_env.display()
                ),
            ));
        }
        if !replace {
            if let Some(entry) = entries
                .iter()
                .zip(vault.fields())
                .find_map(|(entry, (_, existed))| existed.then_some(entry))
            {
                return Err(VaultUiError::new(
                    VaultUiErrorKind::Conflict,
                    format!(
                        "Vault field '{}' already exists; enable Replace to import over it.",
                        entry.reference
                    ),
                ));
            }
        }

        let imported =
            super::super::vault_import::resolve_import_with_cancellation(environment, cancelled)
                .map_err(map_anyhow_error)?;
        ensure_operation_active(cancelled)?;
        let prepared =
            PreparedPrivateFile::prepare_if_unchanged(destination, imported.destination, overwrite)
                .map_err(map_vault_error)?;
        ensure_operation_active(cancelled)?;
        self.with_vault(|selected, passphrase| {
            selected.import_fields_if_unchanged(passphrase, imported.mutations, vault, replace)
        })?;
        if let Err(error) = prepared.install() {
            return Err(VaultUiError::new(
                map_vault_error_kind(error.kind()),
                format!(
                    "Vault import succeeded, but destination installation failed: {}. Safe rerun: {recovery_command}",
                    error.message()
                ),
            ));
        }
        Ok(self.finish_committed(VaultCommittedAction::Imported))
    }

    fn change_session_passphrase(
        &self,
        new_passphrase: SecretBytes,
    ) -> std::result::Result<VaultActionResult, VaultUiError> {
        let new_passphrase = Self::passphrase_from_bytes(new_passphrase)?;
        let mut session = self.session()?;
        let current = session.credential.as_ref().ok_or_else(|| {
            VaultUiError::new(VaultUiErrorKind::Authentication, "Vault is locked.")
        })?;
        let selected = vault(&self.resolved).map_err(map_anyhow_error)?;
        selected
            .change_passphrase(current, &new_passphrase)
            .map_err(map_vault_error)?;
        session.credential = Some(new_passphrase);
        drop(session);
        Ok(self.finish_committed(VaultCommittedAction::PassphraseChanged))
    }

    fn initialize_with_outcome(
        &self,
        passphrase: SecretBytes,
    ) -> std::result::Result<VaultActionResult, VaultUiError> {
        let passphrase = Self::passphrase_from_bytes(passphrase)?;
        validate_new_vault_passphrase(&passphrase).map_err(map_vault_error)?;
        let selected = vault(&self.resolved).map_err(map_anyhow_error)?;
        selected.init(&passphrase).map_err(map_vault_error)?;
        match self.session() {
            Ok(mut session) => session.credential = Some(passphrase),
            Err(refresh_error) => {
                return Ok(committed_action_result(
                    VaultCommittedAction::Initialized,
                    Err(refresh_error),
                ));
            }
        }
        Ok(self.finish_committed(VaultCommittedAction::Initialized))
    }
}

fn committed_action_result(
    action: VaultCommittedAction,
    refresh: std::result::Result<VaultSnapshot, VaultUiError>,
) -> VaultActionResult {
    match refresh {
        Ok(snapshot) => action.with_snapshot(snapshot),
        Err(refresh_error) => VaultActionResult::Committed {
            action,
            refresh_error,
        },
    }
}

impl VaultBackend for VaultTuiBackend {
    fn descriptor(&self) -> VaultDescriptor {
        self.descriptor.clone()
    }

    fn home_state(&self) -> std::result::Result<VaultHomeState, VaultUiError> {
        Vault::status(Some(self.descriptor.home.clone()))
            .map(|status| status.home_state)
            .map_err(map_vault_error)
    }

    fn unlock(&self, passphrase: SecretBytes) -> std::result::Result<VaultSnapshot, VaultUiError> {
        let passphrase = Self::passphrase_from_bytes(passphrase)?;
        let selected = vault(&self.resolved).map_err(map_anyhow_error)?;
        let snapshot = selected.snapshot(&passphrase).map_err(map_vault_error)?;
        self.session()?.credential = Some(passphrase);
        Ok(snapshot)
    }

    fn initialize(
        &self,
        passphrase: SecretBytes,
    ) -> std::result::Result<VaultSnapshot, VaultUiError> {
        match self.initialize_with_outcome(passphrase)? {
            VaultActionResult::Snapshot(snapshot) => Ok(snapshot),
            VaultActionResult::Committed { refresh_error, .. } => Err(refresh_error),
            _ => Err(VaultUiError::new(
                VaultUiErrorKind::Other,
                "Vault initialization returned an unexpected result.",
            )),
        }
    }

    fn initialize_with_completion(
        &self,
        passphrase: SecretBytes,
    ) -> std::result::Result<VaultActionResult, VaultUiError> {
        self.initialize_with_outcome(passphrase)
    }

    fn lock(&self) {
        // Poisoning means a previous operation panicked while holding the
        // session mutex. Normal operations fail closed through `session()`,
        // but explicit erasure must still recover the guard and drop secrets.
        let mut session = match self.session.lock() {
            Ok(session) => session,
            Err(poisoned) => poisoned.into_inner(),
        };
        session.erase();
    }

    fn refresh(&self) -> std::result::Result<VaultSnapshot, VaultUiError> {
        self.snapshot()
    }

    fn execute(&self, action: VaultAction) -> std::result::Result<VaultActionResult, VaultUiError> {
        match action {
            VaultAction::Refresh => self.refresh().map(VaultActionResult::Snapshot),
            VaultAction::MigrateToV2 => {
                self.with_vault(|selected, passphrase| selected.migrate(passphrase, 2))?;
                Ok(self.finish_committed(VaultCommittedAction::Migrated))
            }
            VaultAction::Mutate { revision, mutation } => self.execute_mutation(revision, mutation),
            VaultAction::Activity { limit } => self
                .with_vault(|selected, passphrase| selected.activity(passphrase, limit))
                .map(VaultActionResult::Activity),
            VaultAction::VerifyAudit => self
                .with_vault(Vault::verify_audit)
                .map(VaultActionResult::Audit),
            VaultAction::PreviewOnePasswordImport {
                env_file,
                item,
                out_env,
                replace,
                overwrite,
                dry_run,
            } => self
                .preview_onepassword_import(
                    OnePasswordPreviewRequest {
                        env_file,
                        item,
                        out_env,
                        replace,
                        overwrite,
                        dry_run,
                    },
                    &|| false,
                )
                .map(VaultActionResult::ImportPreview),
            VaultAction::CommitOnePasswordImport {
                plan,
                replace,
                overwrite,
            } => self.commit_onepassword_import(plan, replace, overwrite, &|| false),
            VaultAction::DiscardOnePasswordImport { plan } => {
                self.discard_pending_import(&plan)?;
                Ok(VaultActionResult::ImportDiscarded)
            }
            VaultAction::CreateBackup { output, overwrite } => {
                let result = self.with_vault(|selected, passphrase| {
                    let request = Vault::preflight_backup_create(
                        selected.root().to_path_buf(),
                        &output,
                        overwrite,
                    )?;
                    Vault::create_backup(passphrase, request)
                })?;
                Ok(self.finish_committed(VaultCommittedAction::BackupCreated {
                    output,
                    bytes_written: result.bytes_written,
                    backup_version: result.backup_version,
                }))
            }
            VaultAction::RestoreBackup { input, passphrase } => {
                let passphrase = Self::passphrase_from_bytes(passphrase)?;
                let request = Vault::preflight_backup_restore(&input, self.descriptor.home.clone())
                    .map_err(map_vault_error)?;
                let restored =
                    Vault::restore_backup(&passphrase, request).map_err(map_vault_error)?;
                Ok(VaultActionResult::Restored {
                    root: restored.root,
                    vault_id: restored.vault_id,
                    format_version: restored.format_version,
                })
            }
            VaultAction::ChangePassphrase { new_passphrase } => {
                self.change_session_passphrase(new_passphrase)
            }
            VaultAction::ExportField {
                reference,
                output,
                overwrite,
            } => {
                let result = self.with_vault(|selected, passphrase| {
                    selected.preflight_private_output(&output, overwrite)?;
                    selected.read_field_to_file(passphrase, reference, &output, overwrite)
                })?;
                Ok(self.finish_committed(VaultCommittedAction::Exported {
                    output,
                    bytes_written: result.bytes_written,
                }))
            }
        }
    }

    fn execute_with_cancellation(
        &self,
        action: VaultAction,
        cancelled: &dyn Fn() -> bool,
    ) -> std::result::Result<VaultActionResult, VaultUiError> {
        match action {
            VaultAction::PreviewOnePasswordImport {
                env_file,
                item,
                out_env,
                replace,
                overwrite,
                dry_run,
            } => self
                .preview_onepassword_import(
                    OnePasswordPreviewRequest {
                        env_file,
                        item,
                        out_env,
                        replace,
                        overwrite,
                        dry_run,
                    },
                    cancelled,
                )
                .map(VaultActionResult::ImportPreview),
            VaultAction::CommitOnePasswordImport {
                plan,
                replace,
                overwrite,
            } => self.commit_onepassword_import(plan, replace, overwrite, cancelled),
            action => self.execute(action),
        }
    }

    fn peek(
        &self,
        reference: &VaultReference,
        output: &mut dyn Write,
    ) -> std::result::Result<usize, VaultUiError> {
        let mut output = output;
        self.with_vault(|selected, passphrase| {
            selected.read_field_to(passphrase, reference.clone(), &mut output)
        })
        .map(|result| result.bytes_written)
    }
}

impl VaultTuiBackend {
    fn execute_mutation(
        &self,
        revision: VaultRevision,
        mutation: VaultMutation,
    ) -> std::result::Result<VaultActionResult, VaultUiError> {
        self.with_vault(|selected, passphrase| {
            selected.mutate_if_unchanged(passphrase, revision, mutation)
        })?;
        Ok(self.finish_committed(VaultCommittedAction::Mutated))
    }
}

fn map_vault_error(error: VaultError) -> VaultUiError {
    let kind = map_vault_error_kind(error.kind());
    VaultUiError::new(kind, error.message())
}

fn map_vault_error_kind(kind: VaultErrorKind) -> VaultUiErrorKind {
    match kind {
        VaultErrorKind::Authentication => VaultUiErrorKind::Authentication,
        VaultErrorKind::InvalidInput => VaultUiErrorKind::InvalidInput,
        VaultErrorKind::NotFound => VaultUiErrorKind::NotFound,
        VaultErrorKind::AuditTampered => VaultUiErrorKind::Audit,
        VaultErrorKind::AlreadyExists => VaultUiErrorKind::Conflict,
        VaultErrorKind::Io => VaultUiErrorKind::Io,
        VaultErrorKind::Process | VaultErrorKind::Serialization | VaultErrorKind::Internal => {
            VaultUiErrorKind::Other
        }
        _ => VaultUiErrorKind::Other,
    }
}

fn map_anyhow_error(error: anyhow::Error) -> VaultUiError {
    VaultUiError::new(VaultUiErrorKind::Other, error.to_string())
}

fn ensure_operation_active(cancelled: &dyn Fn() -> bool) -> std::result::Result<(), VaultUiError> {
    if cancelled() {
        return Err(VaultUiError::new(
            VaultUiErrorKind::Other,
            "Vault operation was cancelled.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
