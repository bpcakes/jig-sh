use super::*;

const PASSPHRASE: &str = "correct horse battery staple";

fn legacy_backend(temp: &tempfile::TempDir, version: u32) -> (VaultTuiBackend, VaultSnapshot) {
    let home = temp.path().join(format!("vault-v{version}"));
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    vault
        .init_format_for_test(&SecretString::from(PASSPHRASE.to_owned()), version)
        .unwrap();
    let backend = VaultTuiBackend::new(request(home)).unwrap();
    let snapshot = backend
        .unlock(SecretBytes::new(PASSPHRASE.as_bytes().to_vec()))
        .unwrap();
    assert_eq!(snapshot.format_version, version);
    (backend, snapshot)
}

#[test]
fn migrate_action_moves_legacy_vaults_to_the_latest_format_and_keeps_managing() {
    for version in [1, 2] {
        let temp = tempfile::tempdir().unwrap();
        let (backend, _) = legacy_backend(&temp, version);

        let result = backend.execute(VaultAction::MigrateToLatest).unwrap();
        let VaultActionResult::Snapshot(snapshot) = result else {
            panic!("migration did not return a refreshed snapshot");
        };
        assert_eq!(
            snapshot.format_version,
            jig_vault::LATEST_VAULT_FORMAT_VERSION
        );

        let snapshot = mutate(
            &backend,
            &snapshot,
            VaultMutation::SetField {
                reference: "jig://Example/AFTER_MIGRATION".parse().unwrap(),
                kind: FieldKind::Text,
                value: SecretBytes::new(b"post-migration value".to_vec()),
                mode: VaultWriteMode::Create,
            },
        )
        .unwrap();
        assert_eq!(snapshot.fields.len(), 1);
        assert_eq!(
            snapshot.format_version,
            jig_vault::LATEST_VAULT_FORMAT_VERSION
        );
    }
}

#[test]
fn version_two_vaults_keep_field_management_without_migrating() {
    let temp = tempfile::tempdir().unwrap();
    let (backend, snapshot) = legacy_backend(&temp, 2);
    let snapshot = mutate(
        &backend,
        &snapshot,
        VaultMutation::SetField {
            reference: "jig://Example/LEGACY_FORMAT".parse().unwrap(),
            kind: FieldKind::Concealed,
            value: SecretBytes::new(b"still version two".to_vec()),
            mode: VaultWriteMode::Create,
        },
    )
    .unwrap();
    assert_eq!(snapshot.format_version, 2);
    assert_eq!(snapshot.fields.len(), 1);
}

#[test]
fn an_init_orphan_journal_offers_initialization_instead_of_unlock() {
    use jig_vault::test_support::{TransactionFaultPoint, arm_transaction_fault};

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("ExampleVault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    arm_transaction_fault(TransactionFaultPoint::AfterJournal);
    vault
        .init(&SecretString::from(PASSPHRASE.to_owned()))
        .unwrap_err();

    let backend = VaultTuiBackend::new(request(home.clone())).unwrap();
    assert_eq!(
        backend.descriptor().home_state,
        VaultHomeState::Uninitialized
    );
    assert_eq!(backend.home_state().unwrap(), VaultHomeState::Uninitialized);
    assert!(!Vault::status(Some(home)).unwrap().pending_transaction);
    let snapshot = backend
        .initialize(SecretBytes::new(PASSPHRASE.as_bytes().to_vec()))
        .unwrap();
    assert_eq!(
        snapshot.format_version,
        jig_vault::LATEST_VAULT_FORMAT_VERSION
    );
    assert_eq!(backend.home_state().unwrap(), VaultHomeState::Initialized);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_pending_absent_restore_target_is_presented_for_unlock_and_finished() {
    use jig_vault::test_support::{TransactionFaultPoint, arm_transaction_fault};
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let source_home = temp.path().join("source");
    let source = Vault::resolve_for_test(Some(source_home.clone())).unwrap();
    let passphrase = SecretString::from(PASSPHRASE.to_owned());
    source.init(&passphrase).unwrap();
    let archive = temp.path().join("source.backup");
    let create = Vault::preflight_backup_create(source_home, &archive, false).unwrap();
    Vault::create_backup(&passphrase, create).unwrap();
    let target = temp.path().join("restored");
    let restore = Vault::preflight_backup_restore(&archive, target.clone()).unwrap();
    arm_transaction_fault(TransactionFaultPoint::AfterPending);
    assert!(Vault::restore_backup(&passphrase, restore).is_err());

    let backend = VaultTuiBackend::new(request(target.clone())).unwrap();
    assert_eq!(backend.descriptor().home_state, VaultHomeState::Initialized);
    assert!(!target.exists());
    let credential_error = backend
        .unlock(SecretBytes::new(b"incorrect credential sentinel".to_vec()))
        .unwrap_err();
    assert_eq!(credential_error.kind(), VaultUiErrorKind::Authentication);
    assert!(credential_error.message().contains("backup's passphrase"));
    assert!(
        credential_error
            .message()
            .contains(crate::runtime::VAULT_PASSPHRASE_OPERATOR_GUIDANCE)
    );
    assert!(
        !credential_error
            .message()
            .contains("incorrect credential sentinel")
    );
    assert!(!target.exists());
    std::fs::create_dir(&target).unwrap();
    let occupant = target.join("occupied");
    std::fs::write(&occupant, b"existing contents").unwrap();
    let conflict = backend
        .unlock(SecretBytes::new(PASSPHRASE.as_bytes().to_vec()))
        .unwrap_err();
    assert_eq!(conflict.kind(), VaultUiErrorKind::Conflict);
    assert!(
        conflict
            .message()
            .contains(crate::runtime::vault::scope::VAULT_STORAGE_OPERATOR_STEP)
    );
    assert!(!conflict.message().contains(PASSPHRASE));
    assert_eq!(std::fs::read(&occupant).unwrap(), b"existing contents");
    assert!(
        Vault::status(Some(target.clone()))
            .unwrap()
            .pending_transaction
    );
    // Remove only this test-owned collision and the open's known home lock.
    std::fs::remove_file(target.join("vault.lock")).unwrap();
    std::fs::remove_file(occupant).unwrap();
    std::fs::remove_dir(&target).unwrap();
    // Failure to read an unrelated witness record must not reroute this
    // absent pending target to initialization or restore.
    let damaged = temp
        .path()
        .join(".jig-vault-witness/ids")
        .join(format!("{}.json", "f".repeat(64)));
    std::fs::write(&damaged, b"invalid witness record").unwrap();
    assert!(VaultTuiBackend::new(request(target.clone())).is_err());
    assert!(backend.home_state().is_err());
    assert!(!target.exists());
    std::fs::remove_file(damaged).unwrap();
    let snapshot = backend
        .unlock(SecretBytes::new(PASSPHRASE.as_bytes().to_vec()))
        .unwrap();
    assert_eq!(
        snapshot.format_version,
        jig_vault::LATEST_VAULT_FORMAT_VERSION
    );
    assert!(target.join("vault.json").exists());
    assert_eq!(backend.home_state().unwrap(), VaultHomeState::Initialized);
}

#[test]
fn pending_rekey_errors_route_credentials_to_operator_in_tui() {
    use jig_vault::test_support::{TransactionFaultPoint, arm_transaction_fault};

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("vault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    vault
        .init(&SecretString::from(PASSPHRASE.to_owned()))
        .unwrap();
    let backend = VaultTuiBackend::new(request(home.clone())).unwrap();
    backend
        .unlock(SecretBytes::new(PASSPHRASE.as_bytes().to_vec()))
        .unwrap();
    let new_passphrase = b"new correct horse battery staple";
    arm_transaction_fault(TransactionFaultPoint::AfterPending);
    let error = backend
        .execute(VaultAction::ChangePassphrase {
            new_passphrase: SecretBytes::new(new_passphrase.to_vec()),
        })
        .unwrap_err();
    assert_eq!(error.kind(), VaultUiErrorKind::Io);
    assert!(
        error
            .message()
            .contains(crate::runtime::VAULT_PASSPHRASE_OPERATOR_GUIDANCE)
    );
    let locked = VaultTuiBackend::new(request(home.clone())).unwrap();
    let error = locked
        .unlock(SecretBytes::new(PASSPHRASE.as_bytes().to_vec()))
        .unwrap_err();
    assert_eq!(error.kind(), VaultUiErrorKind::Authentication);
    assert!(error.message().contains("new passphrase"));
    assert!(
        error
            .message()
            .contains(crate::runtime::VAULT_PASSPHRASE_OPERATOR_GUIDANCE)
    );
    assert!(!error.message().contains(PASSPHRASE));
    assert!(
        !error
            .message()
            .contains(std::str::from_utf8(new_passphrase).unwrap())
    );
    assert!(
        Vault::status(Some(home.clone()))
            .unwrap()
            .pending_transaction
    );
    locked
        .unlock(SecretBytes::new(new_passphrase.to_vec()))
        .unwrap();
    assert!(!Vault::status(Some(home)).unwrap().pending_transaction);
}
