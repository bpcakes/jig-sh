//! Compatibility of bytes written by the released version 2 implementation.

use super::*;
use crate::test_fixtures::{
    GENERATED_V2_AUDIT_EVENTS, GENERATED_V2_AUDIT_MAC, GENERATED_V2_CONCEALED_REFERENCE,
    GENERATED_V2_CONCEALED_VALUE, GENERATED_V2_LEGACY_NAME, GENERATED_V2_LEGACY_VALUE,
    GENERATED_V2_TEXT_REFERENCE, GENERATED_V2_TEXT_VALUE, GENERATED_V2_VAULT_ID,
    generated_v2_passphrase, install_generated_v2_fixture,
};

fn field(reference: &str) -> VaultReference {
    VaultReference::parse(reference).unwrap()
}

fn fixture_store() -> (tempfile::TempDir, VaultStore) {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    install_generated_v2_fixture(&store);
    (temp, store)
}

/// Asserts the envelope is still exactly version 2 on the wire: no header
/// generation and a state schema containing only its secret map.
fn assert_still_v2_wire_shape(store: &VaultStore, passphrase: &SecretString) {
    let text = store.read_vault_text().unwrap().unwrap();
    let raw: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(raw["header"]["version"], V2_FORMAT_VERSION);
    assert!(raw["header"].get("generation").is_none());
    let file: VaultFile = serde_json::from_str(&text).unwrap();
    let state: serde_json::Value =
        serde_json::from_slice(&decrypt_state_for_test(&file, passphrase)).unwrap();
    assert_eq!(
        state.as_object().unwrap().keys().collect::<Vec<_>>(),
        vec!["secrets"]
    );
    for event in audit_events(store) {
        assert!(event.details.get("generation").is_none());
    }
    store.verify_audit(passphrase).unwrap();
}

#[test]
fn frozen_v2_fixture_opens_with_exact_metadata_and_audit() {
    let (_temp, store) = fixture_store();
    let passphrase = generated_v2_passphrase();
    assert_eq!(
        Vault::status(Some(store.root().to_path_buf()))
            .unwrap()
            .format_version,
        Some(V2_FORMAT_VERSION)
    );

    let snapshot = store.snapshot(&passphrase).unwrap();
    assert_eq!(snapshot.format_version, V2_FORMAT_VERSION);
    assert_eq!(snapshot.vault_id, GENERATED_V2_VAULT_ID);
    let fields = snapshot
        .fields
        .iter()
        .map(|record| (record.reference.to_string(), record.kind))
        .collect::<Vec<_>>();
    assert_eq!(
        fields,
        vec![
            (
                GENERATED_V2_CONCEALED_REFERENCE.to_owned(),
                FieldKind::Concealed
            ),
            (GENERATED_V2_TEXT_REFERENCE.to_owned(), FieldKind::Text),
        ]
    );
    assert_eq!(snapshot.legacy_secrets.len(), 1);
    assert_eq!(snapshot.legacy_secrets[0].name, GENERATED_V2_LEGACY_NAME);
    assert_eq!(snapshot.audit.event_count, GENERATED_V2_AUDIT_EVENTS);
    assert_eq!(
        snapshot.audit.latest_mac.as_deref(),
        Some(GENERATED_V2_AUDIT_MAC)
    );

    let vault = store.open_unlocked(&passphrase).unwrap();
    for (name, value) in [
        (
            field(GENERATED_V2_CONCEALED_REFERENCE).to_secret_name(),
            GENERATED_V2_CONCEALED_VALUE,
        ),
        (
            field(GENERATED_V2_TEXT_REFERENCE).to_secret_name(),
            GENERATED_V2_TEXT_VALUE,
        ),
        (
            SecretName::parse(GENERATED_V2_LEGACY_NAME).unwrap(),
            GENERATED_V2_LEGACY_VALUE,
        ),
    ] {
        assert_eq!(vault.secret_value(&name).unwrap().as_slice(), value);
    }
}

#[test]
fn frozen_v2_fixture_field_and_legacy_mutations_stay_version_two() {
    let (_temp, store) = fixture_store();
    let passphrase = generated_v2_passphrase();
    store
        .write_field(
            &passphrase,
            field("jig://ExampleProject/ADDED"),
            FieldKind::Concealed,
            SecretBytes::new(b"added-in-v2-compat".to_vec()),
            VaultWriteMode::Create,
        )
        .unwrap();
    store
        .rename_field(
            &passphrase,
            field("jig://ExampleProject/ADDED"),
            field("jig://ExampleProject/RENAMED"),
        )
        .unwrap();
    assert!(
        store
            .change_field_kind(
                &passphrase,
                field("jig://ExampleProject/RENAMED"),
                FieldKind::Text,
            )
            .unwrap()
            .changed
    );
    store
        .convert_legacy_secret(
            &passphrase,
            GENERATED_V2_LEGACY_NAME,
            field("jig://ExampleProject/CONVERTED"),
            FieldKind::Concealed,
        )
        .unwrap();
    store
        .remove_field_required(&passphrase, field("jig://ExampleProject/RENAMED"))
        .unwrap();
    store
        .set_secret(
            &passphrase,
            "another_legacy_name",
            SecretBytes::new(b"legacy write in v2".to_vec()),
        )
        .unwrap();
    store
        .import_fields(
            &passphrase,
            vec![FieldMutation::set(
                field("jig://Imported/TOKEN"),
                FieldKind::Concealed,
                SecretBytes::new(b"imported-v2-value".to_vec()),
            )],
            false,
        )
        .unwrap();

    assert_still_v2_wire_shape(&store, &passphrase);
    let names = store
        .list_fields(&passphrase)
        .unwrap()
        .into_iter()
        .map(|record| record.reference.to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            GENERATED_V2_CONCEALED_REFERENCE,
            "jig://ExampleProject/CONVERTED",
            GENERATED_V2_TEXT_REFERENCE,
            "jig://Imported/TOKEN",
        ]
    );
}

#[test]
fn frozen_v2_fixture_rekey_keeps_the_legacy_dek_reuse_contract() {
    let (_temp, store) = fixture_store();
    let passphrase = generated_v2_passphrase();
    let replacement = SecretString::from("replacement v2 fixture passphrase".to_owned());
    let before = store.open_unlocked(&passphrase).unwrap();
    let dek = before.dek.clone();
    drop(before);

    store
        .change_passphrase_for_test(&passphrase, &replacement)
        .unwrap();

    assert!(store.open_unlocked(&passphrase).is_err());
    let after = store.open_unlocked(&replacement).unwrap();
    assert_eq!(after.dek.as_ref(), dek.as_ref());
    drop(after);
    assert_still_v2_wire_shape(&store, &replacement);
    let last = audit_events(&store).pop().unwrap();
    assert_eq!(last.action, "passphrase_change");
    assert_eq!(last.details["format_version"], V2_FORMAT_VERSION);
}

#[test]
fn frozen_v2_fixture_migrates_to_v3_preserving_history_and_values() {
    let (_temp, store) = fixture_store();
    let passphrase = generated_v2_passphrase();
    let before_audit = store.read_audit_text().unwrap().unwrap();
    let before_fields = store.list_fields(&passphrase).unwrap();
    let before_legacy = store.list(&passphrase).unwrap();

    let migration = store.migrate(&passphrase, V3_FORMAT_VERSION).unwrap();
    assert!(migration.changed);
    assert_eq!(migration.from_version, V2_FORMAT_VERSION);

    let file: VaultFile = serde_json::from_str(&store.read_vault_text().unwrap().unwrap()).unwrap();
    assert_eq!(file.header.vault_id, GENERATED_V2_VAULT_ID);
    assert_eq!(file.header.generation, Some(1));
    assert_eq!(store.list_fields(&passphrase).unwrap(), before_fields);
    assert_eq!(store.list(&passphrase).unwrap(), before_legacy);
    let audit = store.read_audit_text().unwrap().unwrap();
    assert!(audit.starts_with(&before_audit));
    let verification = store.verify_audit(&passphrase).unwrap();
    assert_eq!(verification.event_count, GENERATED_V2_AUDIT_EVENTS + 1);
    let vault = store.open_unlocked(&passphrase).unwrap();
    assert_eq!(
        vault
            .secret_value(&field(GENERATED_V2_TEXT_REFERENCE).to_secret_name())
            .unwrap()
            .as_slice(),
        GENERATED_V2_TEXT_VALUE
    );
}
