use std::{
    io::Write,
    sync::{Mutex, MutexGuard},
};

use anyhow::Result;
#[cfg(all(unix, not(test)))]
use anyhow::anyhow;
use jig_vault::{SecretBytes, Vault, VaultError, VaultErrorKind, VaultReference, VaultSnapshot};
use jig_vault_tui::{
    VaultAction, VaultActionResult, VaultBackend, VaultDescriptor, VaultUiError, VaultUiErrorKind,
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
    credential: Mutex<Option<SecretString>>,
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
            credential: Mutex::new(None),
        })
    }

    fn credential(
        &self,
    ) -> std::result::Result<MutexGuard<'_, Option<SecretString>>, VaultUiError> {
        self.credential.lock().map_err(|_| {
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
        let credential = self.credential()?;
        let passphrase = credential.as_ref().ok_or_else(|| {
            VaultUiError::new(VaultUiErrorKind::Authentication, "Vault is locked.")
        })?;
        let selected = vault(&self.resolved).map_err(map_anyhow_error)?;
        operation(&selected, passphrase).map_err(map_vault_error)
    }

    fn snapshot(&self) -> std::result::Result<VaultSnapshot, VaultUiError> {
        self.with_vault(Vault::snapshot)
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
        *self.credential()? = Some(passphrase);
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
        *self.credential()? = Some(passphrase);
        Ok(snapshot)
    }

    fn lock(&self) {
        if let Ok(mut credential) = self.credential.lock() {
            *credential = None;
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
            _ => Err(VaultUiError::new(
                VaultUiErrorKind::Unsupported,
                "This Vault TUI action is not available in the browsing milestone.",
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
    let kind = match error.kind() {
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
    };
    VaultUiError::new(kind, error.message())
}

fn map_anyhow_error(error: anyhow::Error) -> VaultUiError {
    VaultUiError::new(VaultUiErrorKind::Other, error.to_string())
}

#[cfg(test)]
mod tests {
    use jig_vault_tui::VaultBackend;
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
}
