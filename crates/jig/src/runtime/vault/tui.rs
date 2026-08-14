use std::{
    io::Write,
    sync::{Mutex, MutexGuard},
};

use anyhow::Result;
#[cfg(all(unix, not(test)))]
use anyhow::anyhow;
use jig_vault::{
    PreparedPrivateFile, SecretBytes, Vault, VaultError, VaultErrorKind, VaultReference,
    VaultSnapshot,
};
use jig_vault_tui::{
    ImportPreview, ImportPreviewRow, VaultAction, VaultActionResult, VaultBackend, VaultDescriptor,
    VaultUiError, VaultUiErrorKind,
};
use secrecy::SecretString;

use crate::command::VaultTuiRequest;

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
            exists: status.exists,
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

    fn preview_onepassword_import(
        &self,
        env_file: std::path::PathBuf,
        item: jig_vault::VaultItem,
        out_env: std::path::PathBuf,
        replace: bool,
        overwrite: bool,
        dry_run: bool,
    ) -> std::result::Result<ImportPreview, VaultUiError> {
        let environment = super::super::vault_env::parse_onepassword_env_file(&env_file, &item)
            .map_err(map_anyhow_error)?;
        let entries = super::super::vault_import::import_entries(&environment);
        let references = entries
            .iter()
            .map(|entry| entry.reference.clone())
            .collect::<Vec<_>>();
        let destination_exists = super::super::vault_import::preflight_destination(&out_env)
            .map_err(map_anyhow_error)?;
        PreparedPrivateFile::preflight(&out_env, destination_exists).map_err(map_vault_error)?;
        super::super::vault_import::recovery_command(
            &env_file,
            &item,
            &out_env,
            &self.descriptor.home,
        )
        .map_err(map_anyhow_error)?;
        let existing = self.with_vault(|selected, passphrase| {
            selected.preview_import_fields(passphrase, &references)
        })?;
        let rows = entries
            .into_iter()
            .zip(existing)
            .map(|(entry, replaces_existing)| ImportPreviewRow {
                variable: entry.name,
                reference: entry.reference,
                kind: entry.kind,
                replaces_existing,
            })
            .collect();
        Ok(ImportPreview {
            env_file,
            item,
            out_env,
            replace,
            overwrite,
            dry_run,
            rows,
            destination_exists,
        })
    }

    fn commit_onepassword_import(
        &self,
        env_file: std::path::PathBuf,
        item: jig_vault::VaultItem,
        out_env: std::path::PathBuf,
        replace: bool,
        overwrite: bool,
    ) -> std::result::Result<VaultSnapshot, VaultUiError> {
        let environment = super::super::vault_env::parse_onepassword_env_file(&env_file, &item)
            .map_err(map_anyhow_error)?;
        let entries = super::super::vault_import::import_entries(&environment);
        let references = entries
            .iter()
            .map(|entry| entry.reference.clone())
            .collect::<Vec<_>>();
        let destination_exists = super::super::vault_import::preflight_destination(&out_env)
            .map_err(map_anyhow_error)?;
        if destination_exists && !overwrite {
            return Err(VaultUiError::new(
                VaultUiErrorKind::Conflict,
                format!(
                    "Vault import destination {} already exists; enable Overwrite to replace it atomically.",
                    out_env.display()
                ),
            ));
        }
        PreparedPrivateFile::preflight(&out_env, overwrite).map_err(map_vault_error)?;
        let recovery_command = super::super::vault_import::recovery_command(
            &env_file,
            &item,
            &out_env,
            &self.descriptor.home,
        )
        .map_err(map_anyhow_error)?;
        let existing = self.with_vault(|selected, passphrase| {
            selected.preview_import_fields(passphrase, &references)
        })?;
        if !replace {
            if let Some((entry, _)) = entries.iter().zip(&existing).find(|(_, exists)| **exists) {
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
            super::super::vault_import::resolve_import(environment).map_err(map_anyhow_error)?;
        let prepared = PreparedPrivateFile::prepare(&out_env, imported.destination, overwrite)
            .map_err(map_vault_error)?;
        self.with_vault(|selected, passphrase| {
            selected.import_fields(passphrase, imported.mutations, replace)
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
        self.refresh()
    }

    fn change_session_passphrase(
        &self,
        new_passphrase: SecretBytes,
    ) -> std::result::Result<VaultSnapshot, VaultUiError> {
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
        selected
            .snapshot(
                session
                    .credential
                    .as_ref()
                    .expect("new credential was installed"),
            )
            .map_err(map_vault_error)
    }
}

impl VaultBackend for VaultTuiBackend {
    fn descriptor(&self) -> VaultDescriptor {
        self.descriptor.clone()
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
        let passphrase = Self::passphrase_from_bytes(passphrase)?;
        let selected = vault(&self.resolved).map_err(map_anyhow_error)?;
        selected.init(&passphrase).map_err(map_vault_error)?;
        let snapshot = selected.snapshot(&passphrase).map_err(map_vault_error)?;
        self.session()?.credential = Some(passphrase);
        Ok(snapshot)
    }

    fn lock(&self) {
        if let Ok(mut session) = self.session.lock() {
            session.credential = None;
        }
    }

    fn refresh(&self) -> std::result::Result<VaultSnapshot, VaultUiError> {
        self.snapshot()
    }

    fn execute(&self, action: VaultAction) -> std::result::Result<VaultActionResult, VaultUiError> {
        match action {
            VaultAction::Refresh => self.refresh().map(VaultActionResult::Snapshot),
            VaultAction::MigrateToV2 => {
                self.with_vault(|selected, passphrase| selected.migrate(passphrase, 2))?;
                self.refresh().map(VaultActionResult::Snapshot)
            }
            VaultAction::SetField {
                reference,
                kind,
                value,
                mode,
            } => {
                self.with_vault(|selected, passphrase| {
                    selected.write_field(passphrase, reference, kind, value, mode)
                })?;
                self.refresh().map(VaultActionResult::Snapshot)
            }
            VaultAction::ChangeFieldKind { reference, kind } => {
                self.with_vault(|selected, passphrase| {
                    selected.change_field_kind(passphrase, reference, kind)
                })?;
                self.refresh().map(VaultActionResult::Snapshot)
            }
            VaultAction::RenameField {
                source,
                destination,
            } => {
                self.with_vault(|selected, passphrase| {
                    selected.rename_field(passphrase, source, destination)
                })?;
                self.refresh().map(VaultActionResult::Snapshot)
            }
            VaultAction::RenameItem {
                source,
                destination,
            } => {
                self.with_vault(|selected, passphrase| {
                    selected.rename_item(passphrase, source, destination)
                })?;
                self.refresh().map(VaultActionResult::Snapshot)
            }
            VaultAction::RemoveField { reference } => {
                self.with_vault(|selected, passphrase| {
                    selected.remove_field_required(passphrase, reference)
                })?;
                self.refresh().map(VaultActionResult::Snapshot)
            }
            VaultAction::RemoveItem { item } => {
                self.with_vault(|selected, passphrase| selected.remove_item(passphrase, item))?;
                self.refresh().map(VaultActionResult::Snapshot)
            }
            VaultAction::SetLegacy { name, value, mode } => {
                self.with_vault(|selected, passphrase| {
                    selected.write_secret(passphrase, &name, value, mode)
                })?;
                self.refresh().map(VaultActionResult::Snapshot)
            }
            VaultAction::RemoveLegacy { name } => {
                self.with_vault(|selected, passphrase| {
                    selected.remove_secret_required(passphrase, &name)
                })?;
                self.refresh().map(VaultActionResult::Snapshot)
            }
            VaultAction::ConvertLegacy {
                name,
                reference,
                kind,
            } => {
                self.with_vault(|selected, passphrase| {
                    selected.convert_legacy_secret(passphrase, &name, reference, kind)
                })?;
                self.refresh().map(VaultActionResult::Snapshot)
            }
            VaultAction::Activity { limit } => self
                .with_vault(|selected, passphrase| selected.activity(passphrase, limit))
                .map(VaultActionResult::Activity),
            VaultAction::VerifyAudit => self
                .with_vault(Vault::verify_audit)
                .map(VaultActionResult::Audit),
            VaultAction::ImportOnePassword {
                env_file,
                item,
                out_env,
                replace,
                overwrite,
                preview,
                dry_run,
            } => {
                if preview {
                    self.preview_onepassword_import(
                        env_file, item, out_env, replace, overwrite, dry_run,
                    )
                    .map(VaultActionResult::ImportPreview)
                } else if dry_run {
                    Err(VaultUiError::new(
                        VaultUiErrorKind::InvalidInput,
                        "A 1Password dry run may preview metadata only and cannot commit.",
                    ))
                } else {
                    self.commit_onepassword_import(env_file, item, out_env, replace, overwrite)
                        .map(VaultActionResult::Snapshot)
                }
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
                let snapshot = self.refresh()?;
                Ok(VaultActionResult::BackupCreated {
                    output,
                    bytes_written: result.bytes_written,
                    backup_version: result.backup_version,
                    snapshot,
                })
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
            VaultAction::ChangePassphrase { new_passphrase } => self
                .change_session_passphrase(new_passphrase)
                .map(VaultActionResult::Snapshot),
            VaultAction::ExportField {
                reference,
                output,
                overwrite,
            } => {
                PreparedPrivateFile::preflight(&output, overwrite).map_err(map_vault_error)?;
                let result = self.with_vault(|selected, passphrase| {
                    selected.read_field_to_file(passphrase, reference, &output, overwrite)
                })?;
                let snapshot = self.refresh()?;
                Ok(VaultActionResult::Exported {
                    output,
                    bytes_written: result.bytes_written,
                    snapshot,
                })
            }
            _ => Err(VaultUiError::new(
                VaultUiErrorKind::Unsupported,
                "This Vault TUI action is not available in the current milestone.",
            )),
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

    #[test]
    fn descriptor_does_not_create_an_absent_home() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("absent");

        let backend = VaultTuiBackend::new(request(home.clone())).unwrap();

        assert!(!backend.descriptor().exists);
        assert!(!home.exists());
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
    fn management_actions_refresh_metadata_and_preserve_core_preconditions() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("vault");
        let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
        let passphrase = SecretString::from("correct horse battery staple".to_owned());
        vault.init(&passphrase).unwrap();
        let backend = VaultTuiBackend::new(request(home)).unwrap();
        backend
            .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
            .unwrap();

        let field: VaultReference = "jig://Production/TOKEN".parse().unwrap();
        let VaultActionResult::Snapshot(created) = backend
            .execute(VaultAction::SetField {
                reference: field.clone(),
                kind: FieldKind::Concealed,
                value: SecretBytes::new(b"initial-secret".to_vec()),
                mode: VaultWriteMode::Create,
            })
            .unwrap()
        else {
            panic!("expected snapshot");
        };
        assert_eq!(created.fields.len(), 1);

        // Simulate a stale TUI create after an external CLI already created
        // the destination. The atomic mode rejects rather than overwriting.
        let collision = backend
            .execute(VaultAction::SetField {
                reference: field.clone(),
                kind: FieldKind::Text,
                value: SecretBytes::new(b"stale-overwrite-sentinel".to_vec()),
                mode: VaultWriteMode::Create,
            })
            .unwrap_err();
        assert_eq!(collision.kind(), VaultUiErrorKind::Conflict);

        backend
            .execute(VaultAction::ChangeFieldKind {
                reference: field.clone(),
                kind: FieldKind::Text,
            })
            .unwrap();
        let moved: VaultReference = "jig://Production/RENAMED".parse().unwrap();
        backend
            .execute(VaultAction::RenameField {
                source: field,
                destination: moved,
            })
            .unwrap();
        let destination = jig_vault::VaultItem::parse("jig://RenamedItem").unwrap();
        backend
            .execute(VaultAction::RenameItem {
                source: jig_vault::VaultItem::parse("jig://Production").unwrap(),
                destination: destination.clone(),
            })
            .unwrap();

        backend
            .execute(VaultAction::SetLegacy {
                name: "old_token".to_owned(),
                value: SecretBytes::new(b"legacy-secret".to_vec()),
                mode: VaultWriteMode::Create,
            })
            .unwrap();
        let converted: VaultReference = "jig://Imported/TOKEN".parse().unwrap();
        let VaultActionResult::Snapshot(converted_snapshot) = backend
            .execute(VaultAction::ConvertLegacy {
                name: "old_token".to_owned(),
                reference: converted.clone(),
                kind: FieldKind::Concealed,
            })
            .unwrap()
        else {
            panic!("expected snapshot");
        };
        assert!(converted_snapshot.legacy_secrets.is_empty());
        assert!(
            converted_snapshot
                .fields
                .iter()
                .any(|record| record.reference == converted)
        );

        backend
            .execute(VaultAction::RemoveField {
                reference: converted,
            })
            .unwrap();
        let VaultActionResult::Snapshot(empty) = backend
            .execute(VaultAction::RemoveItem { item: destination })
            .unwrap()
        else {
            panic!("expected snapshot");
        };
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
        assert!(!activity.is_empty());
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

        let backup = temp.path().join("vault.backup");
        let VaultActionResult::BackupCreated {
            bytes_written,
            backup_version,
            ..
        } = backend
            .execute(VaultAction::CreateBackup {
                output: backup.clone(),
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
                output: backup.clone(),
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
                    input: backup,
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
            .execute(VaultAction::ImportOnePassword {
                env_file: source.clone(),
                item: item.clone(),
                out_env: destination.clone(),
                replace: false,
                overwrite: false,
                preview: true,
                dry_run: false,
            })
            .unwrap()
        else {
            panic!("expected import preview");
        };
        assert_eq!(preview.rows.len(), 2);
        assert!(!preview.destination_exists);
        assert!(!log.exists(), "preview unexpectedly invoked op");

        let error = backend
            .execute(VaultAction::ImportOnePassword {
                env_file: source.clone(),
                item: item.clone(),
                out_env: destination.clone(),
                replace: false,
                overwrite: false,
                preview: false,
                dry_run: true,
            })
            .unwrap_err();
        assert_eq!(error.kind(), VaultUiErrorKind::InvalidInput);
        assert!(
            !log.exists(),
            "invalid dry-run commit unexpectedly invoked op"
        );

        let VaultActionResult::Snapshot(imported) = backend
            .execute(VaultAction::ImportOnePassword {
                env_file: source,
                item,
                out_env: destination.clone(),
                replace: false,
                overwrite: false,
                preview: false,
                dry_run: false,
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
        assert!(!log_text.contains("reserved-env-leaked"));

        let log_len = log_text.len();
        let VaultActionResult::ImportPreview(existing) = backend
            .execute(VaultAction::ImportOnePassword {
                env_file: temp.path().join("source.env"),
                item: jig_vault::VaultItem::parse("jig://Production").unwrap(),
                out_env: destination,
                replace: false,
                overwrite: false,
                preview: true,
                dry_run: true,
            })
            .unwrap()
        else {
            panic!("expected existing import preview");
        };
        assert!(existing.destination_exists);
        assert!(existing.rows.iter().all(|row| row.replaces_existing));
        assert_eq!(
            std::fs::read_to_string(temp.path().join("op.log"))
                .unwrap()
                .len(),
            log_len
        );
    }
}
