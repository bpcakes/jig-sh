use super::*;
use jig_vault::test_support::{TransactionFaultPoint, arm_transaction_fault};

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn missing_pending_restore_audit_has_preservation_guidance_on_tui_unlock() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let source = temp.path().join("ExampleSource");
    let target = temp.path().join("ExampleRestoredVault");
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    Vault::resolve_for_test(Some(source.clone()))
        .unwrap()
        .init(&passphrase)
        .unwrap();
    let archive = temp.path().join("ExampleVault.backup");
    let backup = Vault::preflight_backup_create(source, &archive, false).unwrap();
    Vault::create_backup(&passphrase, backup).unwrap();
    let restore = Vault::preflight_backup_restore(&archive, target.clone()).unwrap();
    arm_transaction_fault(TransactionFaultPoint::AfterPending);
    assert!(Vault::restore_backup(&passphrase, restore).is_err());
    let staging = std::fs::read_dir(temp.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".jig-vault-restore-")
        })
        .unwrap();
    let envelope = std::fs::read(staging.join("vault.json")).unwrap();
    std::fs::remove_file(staging.join("audit.jsonl")).unwrap();
    let backend = VaultTuiBackend::new(request(target.clone())).unwrap();
    let error = backend
        .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
        .unwrap_err();
    assert_eq!(error.kind(), VaultUiErrorKind::NotFound);
    assert!(
        error
            .message()
            .contains("Operator step: preserve the vault, audit log, and recovery data")
    );
    assert!(
        error
            .message()
            .contains("Never delete or edit the rollback witness or its journals")
    );
    assert!(!error.message().contains("correct horse battery staple"));
    assert!(!target.exists());
    assert_eq!(std::fs::read(staging.join("vault.json")).unwrap(), envelope);
    assert!(!staging.join("audit.jsonl").exists());
    assert!(Vault::status(Some(target)).unwrap().pending_transaction);
}

#[test]
fn integrity_refusals_keep_their_kind_and_operator_guidance_in_tui() {
    for (version, pending) in [(1, false), (2, false), (3, false), (3, true)] {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("ExampleVault");
        let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
        let passphrase = SecretString::from("correct horse battery staple".to_owned());
        if pending {
            arm_transaction_fault(TransactionFaultPoint::AfterPending);
            assert!(vault.init(&passphrase).is_err());
            let journal = std::fs::read_dir(temp.path().join(".jig-vault-witness/journals"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            std::fs::remove_file(journal).unwrap();
        } else {
            vault.init_format_for_test(&passphrase, version).unwrap();
            std::fs::remove_file(home.join("audit.jsonl")).unwrap();
        }
        let backend = VaultTuiBackend::new(request(home)).unwrap();
        let error = backend
            .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
            .unwrap_err();
        assert_eq!(error.kind(), VaultUiErrorKind::Audit);
        assert!(
            error
                .message()
                .contains("Operator step: preserve the vault, audit log, and recovery data")
        );
        assert!(
            error
                .message()
                .contains("Never delete or edit the rollback witness or its journals")
        );
        assert!(error.message().contains("Agents must ask the operator"));
        assert!(!error.message().contains("correct horse battery staple"));
        assert!(!error.message().contains("remove the stale vault home"));
        assert!(!error.message().contains("restore audit.jsonl"));
    }
}
