use super::*;
use jig_vault::test_support::{
    TransactionFaultPoint, arm_transaction_fault, with_passphrase_estimate_for_test,
};

#[test]
fn tui_retries_a_recorded_rekey_after_its_credential_fails_current_policy() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("ExampleVault");
    let old = b"correct horse battery staple";
    let recorded = b"passwordpasswordpassword";
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    vault
        .init(&SecretString::from(
            "correct horse battery staple".to_owned(),
        ))
        .unwrap();
    let backend = VaultTuiBackend::new(request(home.clone())).unwrap();
    backend.unlock(SecretBytes::new(old.to_vec())).unwrap();
    arm_transaction_fault(TransactionFaultPoint::AfterPending);
    with_passphrase_estimate_for_test(u64::MAX, || {
        let error = backend
            .execute(VaultAction::ChangePassphrase {
                new_passphrase: SecretBytes::new(recorded.to_vec()),
            })
            .unwrap_err();
        assert_eq!(error.kind(), VaultUiErrorKind::Io);
    });
    assert!(jig_vault::validate_new_vault_passphrase_bytes(recorded).is_err());
    backend
        .execute(VaultAction::ChangePassphrase {
            new_passphrase: SecretBytes::new(recorded.to_vec()),
        })
        .unwrap();
    backend.refresh().unwrap();
    assert!(
        !Vault::status(Some(home.clone()))
            .unwrap()
            .pending_transaction
    );
    let before_vault = std::fs::read(home.join("vault.json")).unwrap();
    let before_audit = std::fs::read(home.join("audit.jsonl")).unwrap();
    let error = backend
        .execute(VaultAction::ChangePassphrase {
            new_passphrase: SecretBytes::new(recorded.to_vec()),
        })
        .unwrap_err();
    assert_eq!(error.kind(), VaultUiErrorKind::InvalidInput);
    assert!(
        error
            .message()
            .contains(jig_vault::NEW_VAULT_PASSPHRASE_POLICY)
    );
    assert_eq!(
        std::fs::read(home.join("vault.json")).unwrap(),
        before_vault
    );
    assert_eq!(
        std::fs::read(home.join("audit.jsonl")).unwrap(),
        before_audit
    );
    backend.refresh().unwrap();
}
