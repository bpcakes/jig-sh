use std::path::{Path, PathBuf};

use jig_vault::test_support::{TransactionFaultPoint, arm_transaction_fault};
use jig_vault::{VaultErrorKind, VaultRecovery};
use secrecy::SecretString;

use super::super::*;
use crate::command::VaultBackupRestoreRequest;
use crate::runtime::VAULT_PASSPHRASE_OPERATOR_GUIDANCE;
use crate::test_env::lock_env;

const PASSPHRASE: &str = "correct horse battery staple";
const NEW_PASSPHRASE: &str = "new correct horse battery staple";

fn options(home: &Path) -> VaultRuntimeOptions {
    VaultRuntimeOptions {
        home: Some(home.to_path_buf()),
        ..Default::default()
    }
}

fn list_error(home: &Path, passphrase: &str) -> anyhow::Error {
    set_captured_passphrase(SecretString::from(passphrase.to_owned())).unwrap();
    dispatch_for_test(VaultCommand::Field(VaultFieldCommand::List(
        VaultFieldListRequest {
            item: None,
            vault: options(home),
        },
    )))
    .unwrap_err()
}

fn assert_credential(error: &anyhow::Error, kind: VaultErrorKind) {
    let core = error.downcast_ref::<jig_vault::VaultError>().unwrap();
    assert_eq!(core.kind(), kind);
    assert_eq!(core.recovery(), Some(VaultRecovery::Credential));
    assert!(
        error
            .to_string()
            .contains(VAULT_PASSPHRASE_OPERATOR_GUIDANCE)
    );
    assert!(!error.to_string().contains(PASSPHRASE));
    assert!(!error.to_string().contains(NEW_PASSPHRASE));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn pending_restore(temp: &tempfile::TempDir) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let source_home = temp.path().join("source");
    let source = Vault::resolve_for_test(Some(source_home.clone())).unwrap();
    let passphrase = SecretString::from(PASSPHRASE.to_owned());
    source.init(&passphrase).unwrap();
    let archive = temp.path().join("source.backup");
    let create = Vault::preflight_backup_create(source_home, &archive, false).unwrap();
    Vault::create_backup(&passphrase, create).unwrap();
    let target = temp.path().join("restored");
    let prepared = Vault::preflight_backup_restore(&archive, target.clone()).unwrap();
    set_captured_passphrase(passphrase).unwrap();
    arm_transaction_fault(TransactionFaultPoint::AfterPending);
    let error = dispatch_for_test(VaultCommand::Backup(VaultBackupCommand::Restore(Box::new(
        VaultBackupRestoreRequest {
            input: archive,
            prepared: Some(prepared),
            vault: options(&target),
        },
    ))))
    .unwrap_err();
    assert_credential(&error, VaultErrorKind::Io);
    assert!(error.to_string().contains("backup's passphrase"));
    target
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn generic_cli_restore_conflicts_route_storage_recovery_and_preserve_staging() {
    let _guard = lock_env();
    let temp = tempfile::tempdir().unwrap();
    let target = pending_restore(&temp);
    std::fs::create_dir(&target).unwrap();
    let occupant = target.join("occupied");
    std::fs::write(&occupant, b"existing contents").unwrap();
    let before: Vec<_> = std::fs::read_dir(temp.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    let error = list_error(&target, PASSPHRASE);
    let core = error.downcast_ref::<jig_vault::VaultError>().unwrap();
    assert_eq!(core.kind(), VaultErrorKind::AlreadyExists);
    assert_eq!(core.recovery(), Some(VaultRecovery::StorageConflict));
    assert!(
        error
            .to_string()
            .contains(scope::VAULT_STORAGE_OPERATOR_STEP)
    );
    assert!(
        error
            .to_string()
            .contains("without deleting the vault rollback witness or its journals")
    );

    set_captured_passphrase(SecretString::from(PASSPHRASE.to_owned())).unwrap();
    let raw = dispatch_raw(VaultCommand::Read(VaultReadRequest {
        reference: "jig://Example/TOKEN".parse().unwrap(),
        reveal: true,
        out_file: None,
        overwrite: false,
        vault: options(&target),
    }))
    .unwrap_err();
    assert_eq!(raw.to_string(), error.to_string());
    assert!(!error.to_string().contains(PASSPHRASE));
    assert_eq!(std::fs::read(&occupant).unwrap(), b"existing contents");
    assert!(before.iter().all(|path| path.exists()));
    assert!(
        Vault::status(Some(target.clone()))
            .unwrap()
            .pending_transaction
    );
    // The authenticated open also creates its home lock in this test-owned
    // conflicting directory. Remove only these known fixture entries.
    std::fs::remove_file(target.join("vault.lock")).unwrap();
    std::fs::remove_file(occupant).unwrap();
    std::fs::remove_dir(&target).unwrap();
    set_captured_passphrase(SecretString::from(PASSPHRASE.to_owned())).unwrap();
    dispatch_for_test(VaultCommand::Field(VaultFieldCommand::List(
        VaultFieldListRequest {
            item: None,
            vault: options(&target),
        },
    )))
    .unwrap();
    assert!(!Vault::status(Some(target)).unwrap().pending_transaction);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn restore_preflight_and_wrong_pending_credential_have_operator_guidance() {
    let _guard = lock_env();
    let temp = tempfile::tempdir().unwrap();
    let target = pending_restore(&temp);
    let error = list_error(&target, "incorrect credential sentinel");
    assert_credential(&error, VaultErrorKind::Authentication);
    assert!(!error.to_string().contains("incorrect credential sentinel"));
    assert!(error.to_string().contains("backup's passphrase"));
    assert!(!target.exists());
    let occupied = temp.path().join("occupied");
    std::fs::create_dir(&occupied).unwrap();
    let mut command = VaultCommand::Backup(VaultBackupCommand::Restore(Box::new(
        VaultBackupRestoreRequest {
            input: temp.path().join("source.backup"),
            prepared: None,
            vault: options(&occupied),
        },
    )));
    let error = preflight_scoped_command(&mut command).unwrap_err();
    assert_eq!(
        error
            .downcast_ref::<jig_vault::VaultError>()
            .unwrap()
            .kind(),
        VaultErrorKind::AlreadyExists
    );
    assert!(
        error
            .to_string()
            .contains(scope::VAULT_STORAGE_OPERATOR_STEP)
    );
}

#[test]
fn pending_rekey_wrong_credential_routes_to_operator_without_changing_kind() {
    let _guard = lock_env();
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("vault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    let current = SecretString::from(PASSPHRASE.to_owned());
    vault.init(&current).unwrap();
    arm_transaction_fault(TransactionFaultPoint::AfterPending);
    let error = vault
        .change_passphrase(&current, &SecretString::from(NEW_PASSPHRASE.to_owned()))
        .unwrap_err();
    assert_credential(&super::vault_operator_guidance(error), VaultErrorKind::Io);
    let error = list_error(&home, PASSPHRASE);
    assert_credential(&error, VaultErrorKind::Authentication);
    assert!(error.to_string().contains("new passphrase"));
    assert!(Vault::status(Some(home)).unwrap().pending_transaction);
    assert!(
        vault
            .snapshot(&SecretString::from(NEW_PASSPHRASE.to_owned()))
            .is_ok()
    );
}

#[test]
fn ordinary_collision_does_not_get_directory_recovery_guidance() {
    let error = jig_vault::VaultError::new(VaultErrorKind::AlreadyExists, "field already exists");
    let error = super::vault_operator_guidance(error);
    assert_eq!(error.to_string(), "field already exists");
    assert_eq!(
        error
            .downcast_ref::<jig_vault::VaultError>()
            .unwrap()
            .kind(),
        VaultErrorKind::AlreadyExists
    );
}
