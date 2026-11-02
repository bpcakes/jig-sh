use super::*;

#[test]
fn text_previews_follow_unlock_mutations_and_refresh() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("ExampleVault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    vault.init(&passphrase).unwrap();
    let reference = "jig://ExampleProject/URL".parse().unwrap();
    vault
        .set_field(
            &passphrase,
            reference,
            FieldKind::Text,
            SecretBytes::new(b"https://example.invalid".to_vec()),
        )
        .unwrap();
    let reference = "jig://ExampleProject/URL".parse().unwrap();
    let backend = VaultTuiBackend::new(request(home)).unwrap();
    let snapshot = backend
        .unlock(SecretBytes::new(b"correct horse battery staple".to_vec()))
        .unwrap();
    assert_eq!(
        snapshot.text_value(&reference),
        Some("https://example.invalid")
    );
    let snapshot = mutate(
        &backend,
        &snapshot,
        VaultMutation::SetField {
            reference: reference.clone(),
            kind: FieldKind::Concealed,
            value: SecretBytes::new(b"secret-sentinel".to_vec()),
            mode: VaultWriteMode::Replace,
        },
    )
    .unwrap();
    assert_eq!(snapshot.text_value(&reference), None);
    assert_eq!(backend.refresh().unwrap().text_value(&reference), None);
    backend.lock();
    assert!(backend.refresh().is_err());
}
