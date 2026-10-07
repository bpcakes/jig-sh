//! In-flight operations must check persisted state again before audit append.

use super::*;

#[test]
fn retained_handle_refuses_an_older_same_id_envelope() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "ExampleVault");
    store.init(&passphrase()).unwrap();
    set_value(&store, "jig://Example/TOKEN", b"retained value").unwrap();
    let older = std::fs::read(store.vault_path()).unwrap();
    set_value(&store, "jig://Example/OTHER", b"newer value").unwrap();
    let reveal = store
        .prepare_field_read(&passphrase(), field("jig://Example/TOKEN"))
        .unwrap();
    // Keep the current audit and witness, isolating the envelope comparison.
    std::fs::write(store.vault_path(), &older).unwrap();
    let audit = std::fs::read(store.audit_path()).unwrap();

    let error = reveal.write_to(&mut Vec::new()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
    assert_eq!(std::fs::read(store.audit_path()).unwrap(), audit);
    assert_eq!(std::fs::read(store.vault_path()).unwrap(), older);
}

#[test]
fn retained_handle_refuses_a_same_id_legacy_replay() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "ExampleVault");
    init_v2(&store, &passphrase());
    set_value(&store, "jig://Example/TOKEN", b"retained value").unwrap();
    let legacy = std::fs::read(store.vault_path()).unwrap();
    store.migrate(&passphrase(), V3_FORMAT_VERSION).unwrap();
    let reveal = store
        .prepare_field_read(&passphrase(), field("jig://Example/TOKEN"))
        .unwrap();
    std::fs::write(store.vault_path(), &legacy).unwrap();
    let audit = std::fs::read(store.audit_path()).unwrap();

    let error = reveal.write_to(&mut Vec::new()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
    assert_eq!(std::fs::read(store.audit_path()).unwrap(), audit);
    assert_eq!(std::fs::read(store.vault_path()).unwrap(), legacy);
}

#[test]
fn retained_handle_refuses_a_missing_vault_file() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "ExampleVault");
    store.init(&passphrase()).unwrap();
    set_value(&store, "jig://Example/TOKEN", b"retained value").unwrap();
    let reveal = store
        .prepare_field_read(&passphrase(), field("jig://Example/TOKEN"))
        .unwrap();
    std::fs::remove_file(store.vault_path()).unwrap();
    let audit = std::fs::read(store.audit_path()).unwrap();

    let error = reveal.write_to(&mut Vec::new()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
    assert_eq!(std::fs::read(store.audit_path()).unwrap(), audit);
    assert!(!store.vault_path().exists());
}

#[test]
fn retained_handle_cannot_reenroll_a_missing_checkpoint() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "ExampleVault");
    store.init(&passphrase()).unwrap();
    set_value(&store, "jig://Example/TOKEN", b"retained value").unwrap();
    let reveal = store
        .prepare_field_read(&passphrase(), field("jig://Example/TOKEN"))
        .unwrap();
    // Removing test-only authority exercises the guard; production witness
    // deletion is outside the rollback guarantee and is not a recovery step.
    let record = record_path(&temp, &store);
    std::fs::remove_file(&record).unwrap();
    let before = snapshot_files(&store);

    let error = reveal.write_to(&mut Vec::new()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
    assert_eq!(snapshot_files(&store), before);
    assert!(!record.exists());
}
