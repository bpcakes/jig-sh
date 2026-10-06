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
