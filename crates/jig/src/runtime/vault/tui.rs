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
mod tests {
    use jig_vault::{FieldKind, VaultWriteMode};
    use jig_vault_tui::{VaultAction, VaultActionResult, VaultBackend};
    use secrecy::SecretString;

    use super::*;
    use crate::command::{VaultRuntimeOptions, VaultTuiRequest};

    fn request(home: std::path::PathBuf) -> VaultTuiRequest {
        VaultTuiRequest {
            vault: VaultRuntimeOptions {
                home: Some(home),
                ..Default::default()
            },
        }
    }

    #[cfg(unix)]
    fn lifecycle_backup_path(temp: &tempfile::TempDir) -> std::path::PathBuf {
        // Recompute the owned action path so the Linux-only restore use can be
        // compiled out on macOS without leaving a redundant final clone.
        temp.path().join("vault.backup")
    }

    fn mutate(
        backend: &VaultTuiBackend,
        snapshot: &VaultSnapshot,
        mutation: VaultMutation,
    ) -> std::result::Result<VaultSnapshot, VaultUiError> {
        let result = backend.execute(VaultAction::Mutate {
            revision: snapshot.revision.clone(),
            mutation,
        })?;
        let VaultActionResult::Snapshot(snapshot) = result else {
            panic!("vault mutation did not return a snapshot");
        };
        Ok(snapshot)
    }

    #[test]
    fn descriptor_does_not_create_an_absent_home() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("absent");

        let backend = VaultTuiBackend::new(request(home.clone())).unwrap();

        assert_eq!(backend.descriptor().home_state, VaultHomeState::Absent);
        assert_eq!(backend.home_state().unwrap(), VaultHomeState::Absent);
        assert!(!home.exists());

        std::fs::create_dir(&home).unwrap();
        assert_eq!(backend.home_state().unwrap(), VaultHomeState::Uninitialized);
        std::fs::write(home.join("vault.json"), b"installed").unwrap();
        assert_eq!(backend.home_state().unwrap(), VaultHomeState::Initialized);
    }

    #[test]
    fn invalid_initialization_passphrase_is_rejected_before_home_creation() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("absent");
        let backend = VaultTuiBackend::new(request(home.clone())).unwrap();

        let error = backend
            .initialize(SecretBytes::new(b"too-short".to_vec()))
            .unwrap_err();

        assert_eq!(error.kind(), VaultUiErrorKind::InvalidInput);
        assert!(!home.exists());
        assert_eq!(backend.home_state().unwrap(), VaultHomeState::Absent);
    }

    #[test]
    fn failed_unlock_retains_no_session_and_lock_drops_a_valid_session() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("vault");
        let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
        let passphrase = SecretString::from("correct horse battery staple".to_owned());
        vault.init(&passphrase).unwrap();
        let backend = VaultTuiBackend::new(request(home)).unwrap();

        let error = backend
            .unlock(SecretBytes::new(b"wrong passphrase".to_vec()))
            .unwrap_err();
        assert_eq!(error.kind(), VaultUiErrorKind::Authentication);
        assert_eq!(
            backend.refresh().unwrap_err().kind(),
            VaultUiErrorKind::Authentication
        );

        let snapshot = backend
            .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
            .unwrap();
        assert_eq!(snapshot.format_version, 2);
        backend.lock();
        assert_eq!(
            backend.refresh().unwrap_err().kind(),
            VaultUiErrorKind::Authentication
        );
    }

    #[test]
    fn committed_action_result_preserves_success_when_refresh_fails() {
        let action = VaultCommittedAction::Exported {
            output: "/tmp/private-export".into(),
            bytes_written: 17,
        };
        let refresh_error = VaultUiError::new(VaultUiErrorKind::Io, "safe refresh failure");

        let result = committed_action_result(action.clone(), Err(refresh_error.clone()));

        assert_eq!(
            result,
            VaultActionResult::Committed {
                action,
                refresh_error,
            }
        );
    }

    #[test]
    fn lock_erases_credentials_even_when_the_session_mutex_is_poisoned() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("vault");
        let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
        let passphrase = SecretString::from("correct horse battery staple".to_owned());
        vault.init(&passphrase).unwrap();
        let backend = VaultTuiBackend::new(request(home)).unwrap();
        backend
            .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
            .unwrap();

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _session = backend.session.lock().unwrap();
            panic!("poison the vault TUI session mutex");
        }));
        assert!(panic.is_err());

        backend.lock();

        let session = match backend.session.lock() {
            Ok(_) => panic!("session mutex unexpectedly lost its poisoned state"),
            Err(poisoned) => poisoned.into_inner(),
        };
        assert!(session.credential.is_none());
        assert!(session.pending_import.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn private_output_actions_reject_the_vault_home_before_sink_work() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let home = temp.path().join("vault");
        let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
        let passphrase = SecretString::from("correct horse battery staple".to_owned());
        vault.init(&passphrase).unwrap();
        vault
            .set_field(
                &passphrase,
                "jig://Production/TOKEN".parse().unwrap(),
                FieldKind::Concealed,
                SecretBytes::new(b"reserved-tui-output-sentinel".to_vec()),
            )
            .unwrap();
        let backend = VaultTuiBackend::new(request(home.clone())).unwrap();
        backend
            .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
            .unwrap();
        let vault_path = home.join("vault.json");
        let audit_path = home.join("audit.jsonl");
        let before_vault = std::fs::read(&vault_path).unwrap();
        let before_audit = std::fs::read(&audit_path).unwrap();

        let export_error = backend
            .execute(VaultAction::ExportField {
                reference: "jig://Production/TOKEN".parse().unwrap(),
                output: vault_path.clone(),
                overwrite: true,
            })
            .unwrap_err();
        assert_eq!(export_error.kind(), VaultUiErrorKind::InvalidInput);

        let source = temp.path().join("source.env");
        std::fs::write(&source, b"MODE=production\n").unwrap();
        let import_error = backend
            .execute(VaultAction::PreviewOnePasswordImport {
                env_file: source,
                item: jig_vault::VaultItem::parse("jig://Production").unwrap(),
                out_env: audit_path.clone(),
                replace: true,
                overwrite: true,
                dry_run: false,
            })
            .unwrap_err();
        assert_eq!(import_error.kind(), VaultUiErrorKind::InvalidInput);

        assert_eq!(std::fs::read(&vault_path).unwrap(), before_vault);
        assert_eq!(std::fs::read(&audit_path).unwrap(), before_audit);
        assert!(backend.session().unwrap().pending_import.is_none());
        assert!(
            !format!("{export_error:?}{import_error:?}").contains("reserved-tui-output-sentinel")
        );
    }

    #[cfg(unix)]
    #[test]
    fn import_discard_consumes_only_the_matching_pending_plan() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let home = temp.path().join("vault");
        let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
        let passphrase = SecretString::from("correct horse battery staple".to_owned());
        vault.init(&passphrase).unwrap();
        let backend = VaultTuiBackend::new(request(home)).unwrap();
        backend
            .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
            .unwrap();

        let source = temp.path().join("source.env");
        let destination = temp.path().join("generated.env");
        std::fs::write(&source, b"MODE=production\n").unwrap();
        let VaultActionResult::ImportPreview(preview) = backend
            .execute(VaultAction::PreviewOnePasswordImport {
                env_file: source,
                item: jig_vault::VaultItem::parse("jig://Production").unwrap(),
                out_env: destination.clone(),
                replace: false,
                overwrite: false,
                dry_run: false,
            })
            .unwrap()
        else {
            panic!("expected import preview");
        };
        let ImportPreviewAuthorization::Commit(plan) = preview.authorization else {
            panic!("expected commit authority");
        };

        let wrong_plan = backend
            .execute(VaultAction::DiscardOnePasswordImport {
                plan: ImportPlanToken::generate(),
            })
            .unwrap_err();
        assert_eq!(wrong_plan.kind(), VaultUiErrorKind::Conflict);
        assert!(
            backend
                .session()
                .unwrap()
                .pending_import
                .as_ref()
                .is_some_and(|pending| pending.token == plan)
        );

        assert!(matches!(
            backend
                .execute(VaultAction::DiscardOnePasswordImport { plan: plan.clone() })
                .unwrap(),
            VaultActionResult::ImportDiscarded
        ));
        assert!(backend.session().unwrap().pending_import.is_none());

        let consumed = backend
            .execute(VaultAction::CommitOnePasswordImport {
                plan,
                replace: false,
                overwrite: false,
            })
            .unwrap_err();
        assert_eq!(consumed.kind(), VaultUiErrorKind::Conflict);
        assert!(!destination.exists());
        assert!(vault.snapshot(&passphrase).unwrap().fields.is_empty());
    }

    #[test]
    fn management_actions_require_the_snapshot_revision_that_authorized_them() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("vault");
        let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
        let passphrase = SecretString::from("correct horse battery staple".to_owned());
        vault.init(&passphrase).unwrap();
        let backend = VaultTuiBackend::new(request(home)).unwrap();
        let mut current = backend
            .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
            .unwrap();

        let field: VaultReference = "jig://Production/TOKEN".parse().unwrap();
        current = mutate(
            &backend,
            &current,
            VaultMutation::SetField {
                reference: field.clone(),
                kind: FieldKind::Concealed,
                value: SecretBytes::new(b"initial-secret".to_vec()),
                mode: VaultWriteMode::Create,
            },
        )
        .unwrap();
        assert_eq!(current.fields.len(), 1);

        let collision = mutate(
            &backend,
            &current,
            VaultMutation::SetField {
                reference: field.clone(),
                kind: FieldKind::Text,
                value: SecretBytes::new(b"stale-overwrite-sentinel".to_vec()),
                mode: VaultWriteMode::Create,
            },
        )
        .unwrap_err();
        assert_eq!(collision.kind(), VaultUiErrorKind::Conflict);

        // An external CLI write invalidates every command authorized by the
        // previous TUI snapshot. In particular, a stale item delete must not
        // erase a field the operator never saw.
        let external: VaultReference = "jig://Production/EXTERNAL".parse().unwrap();
        vault
            .write_field(
                &passphrase,
                external.clone(),
                FieldKind::Concealed,
                SecretBytes::new(b"external-secret".to_vec()),
                VaultWriteMode::Create,
            )
            .unwrap();
        let after_external = backend.refresh().unwrap();
        let stale_delete = mutate(
            &backend,
            &current,
            VaultMutation::RemoveItem {
                item: jig_vault::VaultItem::parse("jig://Production").unwrap(),
            },
        )
        .unwrap_err();
        assert_eq!(stale_delete.kind(), VaultUiErrorKind::Conflict);
        let after_rejection = backend.refresh().unwrap();
        assert_eq!(after_rejection.revision, after_external.revision);
        assert_eq!(
            after_rejection.audit.event_count,
            after_external.audit.event_count
        );
        assert!(
            after_rejection
                .fields
                .iter()
                .any(|record| record.reference == external)
        );
        current = after_rejection;

        current = mutate(
            &backend,
            &current,
            VaultMutation::ChangeFieldKind {
                reference: field.clone(),
                kind: FieldKind::Text,
            },
        )
        .unwrap();
        let moved: VaultReference = "jig://Production/RENAMED".parse().unwrap();
        current = mutate(
            &backend,
            &current,
            VaultMutation::RenameField {
                source: field,
                destination: moved,
            },
        )
        .unwrap();
        let destination = jig_vault::VaultItem::parse("jig://RenamedItem").unwrap();
        current = mutate(
            &backend,
            &current,
            VaultMutation::RenameItem {
                source: jig_vault::VaultItem::parse("jig://Production").unwrap(),
                destination: destination.clone(),
            },
        )
        .unwrap();

        current = mutate(
            &backend,
            &current,
            VaultMutation::SetLegacy {
                name: "old_token".to_owned(),
                value: SecretBytes::new(b"legacy-secret".to_vec()),
                mode: VaultWriteMode::Create,
            },
        )
        .unwrap();
        let converted: VaultReference = "jig://Imported/TOKEN".parse().unwrap();
        current = mutate(
            &backend,
            &current,
            VaultMutation::ConvertLegacy {
                name: "old_token".to_owned(),
                reference: converted.clone(),
                kind: FieldKind::Concealed,
            },
        )
        .unwrap();
        assert!(current.legacy_secrets.is_empty());
        assert!(
            current
                .fields
                .iter()
                .any(|record| record.reference == converted)
        );

        current = mutate(
            &backend,
            &current,
            VaultMutation::RemoveField {
                reference: converted,
            },
        )
        .unwrap();
        let empty = mutate(
            &backend,
            &current,
            VaultMutation::RemoveItem { item: destination },
        )
        .unwrap();
        assert!(empty.fields.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn lifecycle_tools_backup_restore_rotate_verify_and_project_activity() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let home = temp.path().join("vault");
        let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
        let passphrase = SecretString::from("correct horse battery staple".to_owned());
        vault.init(&passphrase).unwrap();
        vault
            .set_field(
                &passphrase,
                "jig://Production/TOKEN".parse().unwrap(),
                FieldKind::Concealed,
                SecretBytes::new(b"lifecycle-secret-sentinel".to_vec()),
            )
            .unwrap();
        let backend = VaultTuiBackend::new(request(home.clone())).unwrap();
        backend
            .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
            .unwrap();

        let VaultActionResult::Activity(activity) = backend
            .execute(VaultAction::Activity { limit: 10 })
            .unwrap()
        else {
            panic!("expected activity");
        };
        assert!(!activity.records.is_empty());
        assert_eq!(activity.audit.torn_tail_bytes, 0);
        assert!(!format!("{activity:?}").contains("lifecycle-secret-sentinel"));
        let VaultActionResult::Audit(verification) =
            backend.execute(VaultAction::VerifyAudit).unwrap()
        else {
            panic!("expected audit verification");
        };
        assert_eq!(verification.torn_tail_bytes, 0);

        let export = temp.path().join("token.bin");
        let VaultActionResult::Exported {
            bytes_written,
            snapshot,
            ..
        } = backend
            .execute(VaultAction::ExportField {
                reference: "jig://Production/TOKEN".parse().unwrap(),
                output: export.clone(),
                overwrite: false,
            })
            .unwrap()
        else {
            panic!("expected export result");
        };
        assert_eq!(bytes_written, b"lifecycle-secret-sentinel".len());
        assert_eq!(snapshot.fields.len(), 1);
        assert_eq!(
            std::fs::read(&export).unwrap(),
            b"lifecycle-secret-sentinel"
        );
        let export_collision = backend
            .execute(VaultAction::ExportField {
                reference: "jig://Production/TOKEN".parse().unwrap(),
                output: export,
                overwrite: false,
            })
            .unwrap_err();
        assert_eq!(export_collision.kind(), VaultUiErrorKind::Conflict);

        let mut peeked = zeroize::Zeroizing::new(Vec::new());
        let peeked_len = backend
            .peek(&"jig://Production/TOKEN".parse().unwrap(), &mut *peeked)
            .unwrap();
        assert_eq!(peeked_len, b"lifecycle-secret-sentinel".len());
        assert_eq!(&peeked[..], b"lifecycle-secret-sentinel");

        let VaultActionResult::BackupCreated {
            bytes_written,
            backup_version,
            ..
        } = backend
            .execute(VaultAction::CreateBackup {
                output: lifecycle_backup_path(&temp),
                overwrite: false,
            })
            .unwrap()
        else {
            panic!("expected backup result");
        };
        assert!(bytes_written > 0);
        assert_eq!(backup_version, jig_vault::BACKUP_FORMAT_VERSION);
        let collision = backend
            .execute(VaultAction::CreateBackup {
                output: lifecycle_backup_path(&temp),
                overwrite: false,
            })
            .unwrap_err();
        assert_eq!(collision.kind(), VaultUiErrorKind::Conflict);

        #[cfg(target_os = "linux")]
        {
            let restored_home = temp.path().join("restored-vault");
            let restored_backend = VaultTuiBackend::new(request(restored_home.clone())).unwrap();
            let VaultActionResult::Restored {
                root,
                format_version,
                ..
            } = restored_backend
                .execute(VaultAction::RestoreBackup {
                    input: lifecycle_backup_path(&temp),
                    passphrase: SecretBytes::new(b"correct horse battery staple".to_vec()),
                })
                .unwrap()
            else {
                panic!("expected restore result");
            };
            assert_eq!(root, restored_home);
            assert_eq!(format_version, 2);
            let restored = restored_backend
                .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
                .unwrap();
            assert_eq!(restored.fields.len(), 1);
        }

        let new_passphrase = b"new correct horse battery staple";
        let VaultActionResult::Snapshot(rotated) = backend
            .execute(VaultAction::ChangePassphrase {
                new_passphrase: SecretBytes::new(new_passphrase.to_vec()),
            })
            .unwrap()
        else {
            panic!("expected refreshed snapshot");
        };
        assert_eq!(rotated.fields.len(), 1);
        assert_eq!(
            vault.snapshot(&passphrase).unwrap_err().kind(),
            VaultErrorKind::Authentication
        );
        assert!(
            vault
                .snapshot(&SecretString::from(
                    String::from_utf8(new_passphrase.to_vec()).unwrap()
                ))
                .is_ok()
        );
        assert!(backend.refresh().is_ok());

        let audit_path = home.join("audit.jsonl");
        let audit = std::fs::read_to_string(&audit_path).unwrap();
        std::fs::write(
            &audit_path,
            audit.replace(
                "\"action\":\"field_batch_apply\"",
                "\"action\":\"secret_get\"",
            ),
        )
        .unwrap();
        let tampered = backend.execute(VaultAction::VerifyAudit).unwrap_err();
        assert_eq!(tampered.kind(), VaultUiErrorKind::Audit);
    }

    #[cfg(unix)]
    #[test]
    fn onepassword_preview_avoids_op_and_commit_reuses_the_hardened_resolver() {
        use std::os::unix::fs::PermissionsExt;

        use crate::test_env::{EnvVarGuard, lock_env};

        let _env = lock_env();
        let temp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let home = temp.path().join("vault");
        let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
        let passphrase = SecretString::from("correct horse battery staple".to_owned());
        vault.init(&passphrase).unwrap();
        let backend = VaultTuiBackend::new(request(home)).unwrap();
        backend
            .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
            .unwrap();

        let bin = temp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let op = bin.join("op");
        let log = temp.path().join("op.log");
        std::fs::write(
            &op,
            r#"#!/bin/sh
set -eu
if [ "${JIG_VAULT_PASSPHRASE+set}" = set ] || [ "${JIG_VAULT_NEW_PASSPHRASE+set}" = set ]; then
  printf '%s\n' 'reserved-env-leaked' >> "$OP_TEST_LOG"
  exit 87
fi
printf '%s\n' "$3" >> "$OP_TEST_LOG"
printf '%s' 'resolved-tui-import-secret'
"#,
        )
        .unwrap();
        std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut path_parts = vec![bin];
        if let Some(path) = std::env::var_os("PATH") {
            path_parts.extend(std::env::split_paths(&path));
        }
        let _path = EnvVarGuard::set("PATH", std::env::join_paths(path_parts).unwrap());
        let _current = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", "must-not-reach-op");
        let _new = EnvVarGuard::set("JIG_VAULT_NEW_PASSPHRASE", "must-not-reach-op");
        let _log = EnvVarGuard::set("OP_TEST_LOG", &log);

        let source = temp.path().join("source.env");
        let destination = temp.path().join("generated.env");
        std::fs::write(&source, b"TOKEN=op://Test/Login/TOKEN\nMODE=production\n").unwrap();
        let item = jig_vault::VaultItem::parse("jig://Production").unwrap();
        let VaultActionResult::ImportPreview(preview) = backend
            .execute(VaultAction::PreviewOnePasswordImport {
                env_file: source.clone(),
                item,
                out_env: destination.clone(),
                replace: false,
                overwrite: false,
                dry_run: false,
            })
            .unwrap()
        else {
            panic!("expected import preview");
        };
        assert_eq!(preview.rows.len(), 2);
        assert!(!preview.destination_exists);
        assert!(!log.exists(), "preview unexpectedly invoked op");
        let ImportPreviewAuthorization::Commit(plan) = preview.authorization else {
            panic!("non-dry-run preview did not return commit authority");
        };

        // The approved plan owns the parsed protected source. Replacing the
        // path after preview must not change the committed field set.
        std::fs::write(&source, b"OTHER=op://Changed/Login/OTHER\n").unwrap();

        let VaultActionResult::Snapshot(imported) = backend
            .execute(VaultAction::CommitOnePasswordImport {
                plan: plan.clone(),
                replace: false,
                overwrite: false,
            })
            .unwrap()
        else {
            panic!("expected imported snapshot");
        };
        assert_eq!(imported.fields.len(), 2);
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"TOKEN=jig://Production/TOKEN\nMODE=jig://Production/MODE\n"
        );
        let log_text = std::fs::read_to_string(log).unwrap();
        assert!(log_text.contains("op://Test/Login/TOKEN"));
        assert!(!log_text.contains("op://Changed/Login/OTHER"));
        assert!(!log_text.contains("reserved-env-leaked"));

        let reused = backend
            .execute(VaultAction::CommitOnePasswordImport {
                plan,
                replace: false,
                overwrite: false,
            })
            .unwrap_err();
        assert_eq!(reused.kind(), VaultUiErrorKind::Conflict);
        assert!(reused.message().contains("already used"));

        std::fs::write(
            temp.path().join("source.env"),
            b"TOKEN=literal-token\nMODE=op://Test/Login/MODE\n",
        )
        .unwrap();
        let log_len = log_text.len();
        let VaultActionResult::ImportPreview(existing) = backend
            .execute(VaultAction::PreviewOnePasswordImport {
                env_file: temp.path().join("source.env"),
                item: jig_vault::VaultItem::parse("jig://Production").unwrap(),
                out_env: destination,
                replace: false,
                overwrite: false,
                dry_run: true,
            })
            .unwrap()
        else {
            panic!("expected existing import preview");
        };
        assert!(existing.destination_exists);
        assert!(
            existing
                .rows
                .iter()
                .all(|row| row.change.replaces_existing())
        );
        assert_eq!(
            existing.rows[0].change,
            ImportFieldChange::Replace {
                previous_kind: FieldKind::Concealed,
                kind: FieldKind::Text,
            }
        );
        assert_eq!(
            existing.rows[1].change,
            ImportFieldChange::Replace {
                previous_kind: FieldKind::Text,
                kind: FieldKind::Concealed,
            }
        );
        assert!(matches!(
            existing.authorization,
            ImportPreviewAuthorization::DryRun
        ));
        assert_eq!(
            std::fs::read_to_string(temp.path().join("op.log"))
                .unwrap()
                .len(),
            log_len
        );
    }

    #[cfg(unix)]
    #[test]
    fn import_commit_rejects_destination_and_vault_drift_from_preview() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let home = temp.path().join("vault");
        let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
        let passphrase = SecretString::from("correct horse battery staple".to_owned());
        vault.init(&passphrase).unwrap();
        let backend = VaultTuiBackend::new(request(home)).unwrap();
        backend
            .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
            .unwrap();

        let source = temp.path().join("destination-drift.env");
        let destination = temp.path().join("destination-drift.generated.env");
        std::fs::write(&source, b"MODE=production\n").unwrap();
        let VaultActionResult::ImportPreview(preview) = backend
            .execute(VaultAction::PreviewOnePasswordImport {
                env_file: source,
                item: jig_vault::VaultItem::parse("jig://DestinationDrift").unwrap(),
                out_env: destination.clone(),
                replace: false,
                overwrite: false,
                dry_run: false,
            })
            .unwrap()
        else {
            panic!("expected destination-drift preview");
        };
        let ImportPreviewAuthorization::Commit(plan) = preview.authorization else {
            panic!("expected destination-drift commit plan");
        };
        std::fs::write(&destination, b"must-not-be-overwritten").unwrap();

        let destination_error = backend
            .execute(VaultAction::CommitOnePasswordImport {
                plan,
                replace: false,
                overwrite: true,
            })
            .unwrap_err();
        assert_eq!(destination_error.kind(), VaultUiErrorKind::Conflict);
        assert!(
            destination_error
                .message()
                .contains("changed since preview")
        );
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"must-not-be-overwritten"
        );
        assert_eq!(
            vault
                .preview_import_fields(
                    &passphrase,
                    &[jig_vault::VaultReference::parse("jig://DestinationDrift/MODE").unwrap()]
                )
                .unwrap(),
            vec![false]
        );

        let source = temp.path().join("vault-drift.env");
        let destination = temp.path().join("vault-drift.generated.env");
        std::fs::write(&source, b"MODE=production\n").unwrap();
        let VaultActionResult::ImportPreview(preview) = backend
            .execute(VaultAction::PreviewOnePasswordImport {
                env_file: source,
                item: jig_vault::VaultItem::parse("jig://VaultDrift").unwrap(),
                out_env: destination.clone(),
                replace: false,
                overwrite: false,
                dry_run: false,
            })
            .unwrap()
        else {
            panic!("expected vault-drift preview");
        };
        let ImportPreviewAuthorization::Commit(plan) = preview.authorization else {
            panic!("expected vault-drift commit plan");
        };
        vault
            .set_field(
                &passphrase,
                jig_vault::VaultReference::parse("jig://External/CHANGE").unwrap(),
                FieldKind::Text,
                SecretBytes::new(b"external-change".to_vec()),
            )
            .unwrap();

        let vault_error = backend
            .execute(VaultAction::CommitOnePasswordImport {
                plan,
                replace: false,
                overwrite: false,
            })
            .unwrap_err();
        assert_eq!(vault_error.kind(), VaultUiErrorKind::Conflict);
        assert!(
            vault_error
                .message()
                .contains("changed since the import preview")
        );
        assert!(!destination.exists());
        assert_eq!(
            vault
                .preview_import_fields(
                    &passphrase,
                    &[jig_vault::VaultReference::parse("jig://VaultDrift/MODE").unwrap()]
                )
                .unwrap(),
            vec![false]
        );
    }
}
