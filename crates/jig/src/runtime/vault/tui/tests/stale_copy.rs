use super::*;

#[test]
fn stale_and_replayed_copies_keep_operator_guidance_in_tui_errors() {
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    for version in [2, 3] {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("ExampleVault");
        let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
        vault.init_format_for_test(&passphrase, version).unwrap();
        let old_vault = std::fs::read(home.join("vault.json")).unwrap();
        let old_audit = std::fs::read(home.join("audit.jsonl")).unwrap();
        if version == 2 {
            vault.migrate(&passphrase, 3).unwrap();
        } else {
            vault
                .set_field(
                    &passphrase,
                    "jig://Example/TOKEN".parse().unwrap(),
                    FieldKind::Concealed,
                    SecretBytes::new(b"concealed test value".to_vec()),
                )
                .unwrap();
        }
        std::fs::write(home.join("vault.json"), &old_vault).unwrap();
        std::fs::write(home.join("audit.jsonl"), &old_audit).unwrap();

        let backend = VaultTuiBackend::new(request(home.clone())).unwrap();
        let error = backend
            .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
            .unwrap_err();
        assert_eq!(error.kind(), VaultUiErrorKind::Audit);
        let message = error.message();
        assert!(message.contains("Operator step: use the current vault home"));
        assert!(message.contains("restored home"));
        assert!(message.contains("Agents must ask the operator"));
        assert!(message.contains("Never delete or edit the rollback witness or its journals"));
        assert!(!message.contains("correct horse battery staple"));
        assert!(!message.contains("concealed test value"));
        assert_eq!(std::fs::read(home.join("vault.json")).unwrap(), old_vault);
        assert_eq!(std::fs::read(home.join("audit.jsonl")).unwrap(), old_audit);
    }
}
