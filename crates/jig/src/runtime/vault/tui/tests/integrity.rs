use super::*;
use jig_vault::test_support::{TransactionFaultPoint, arm_transaction_fault};

#[test]
fn integrity_refusals_keep_their_kind_and_operator_guidance_in_tui() {
    for pending in [false, true] {
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
            vault.init(&passphrase).unwrap();
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
    }
}
