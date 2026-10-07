//! Rollback, fork, and replay detection against the out-of-home witness.

use super::*;
use crate::crypto::{random_array, seal};
use crate::store::FaultPoint;
use crate::store::witness::WitnessStore;

#[path = "rollback/retained.rs"]
mod retained;

fn field(reference: &str) -> VaultReference {
    VaultReference::parse(reference).unwrap()
}

fn store_at(temp: &tempfile::TempDir, name: &str) -> VaultStore {
    VaultStore::resolve_for_test(Some(temp.path().join(name))).unwrap()
}

fn set_value(store: &VaultStore, reference: &str, value: &[u8]) -> Result<FieldBatchResult> {
    store.write_field(
        &passphrase(),
        field(reference),
        FieldKind::Text,
        SecretBytes::new(value.to_vec()),
        VaultWriteMode::Upsert,
    )
}

fn witness(store: &VaultStore) -> WitnessStore {
    store.witness().open_existing().unwrap().unwrap()
}

fn record_path(temp: &tempfile::TempDir, store: &VaultStore) -> std::path::PathBuf {
    let id = store.header_vault_id_for_lock().unwrap();
    temp.path()
        .join(".jig-vault-witness/ids")
        .join(format!("{}.json", crate::store::witness::id_key(&id)))
}

fn snapshot_files(store: &VaultStore) -> (Vec<u8>, Vec<u8>) {
    (
        std::fs::read(store.vault_path()).unwrap(),
        std::fs::read(store.audit_path()).unwrap(),
    )
}

fn copy_home(from: &VaultStore, to: &std::path::Path) {
    std::fs::create_dir(to).unwrap();
    for name in ["vault.json", "audit.jsonl"] {
        std::fs::copy(from.root().join(name), to.join(name)).unwrap();
    }
}

fn assert_rollback(error: &VaultError) {
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
    assert!(
        error
            .to_string()
            .contains("older than its witnessed generation"),
        "{error}"
    );
    assert_stale_copy_guidance(error);
}

fn assert_stale_copy_guidance(error: &VaultError) {
    let message = error.to_string();
    assert!(message.contains("Operator step: use the current vault home"));
    assert!(message.contains("restored home"));
    assert!(message.contains("Agents must ask the operator"));
    assert!(message.contains("Never delete or edit the rollback witness or its journals"));
}

#[test]
fn vault_only_and_paired_rollbacks_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "vault");
    store.init(&passphrase()).unwrap();
    set_value(&store, "jig://Example/ONE", b"first").unwrap();
    let (old_vault, old_audit) = snapshot_files(&store);
    set_value(&store, "jig://Example/TWO", b"second").unwrap();

    std::fs::write(store.vault_path(), &old_vault).unwrap();
    assert_rollback(&store.list_fields(&passphrase()).unwrap_err());

    std::fs::write(store.audit_path(), &old_audit).unwrap();
    assert_rollback(&store.list_fields(&passphrase()).unwrap_err());
    let error = set_value(&store, "jig://Example/THREE", b"third").unwrap_err();
    assert_rollback(&error);
    assert_eq!(std::fs::read(store.vault_path()).unwrap(), old_vault);
}

#[test]
fn a_same_generation_fork_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "vault");
    store.init(&passphrase()).unwrap();
    let path = record_path(&temp, &store);
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    record["committed"]["envelope_sha256"] = serde_json::json!("f".repeat(64));
    std::fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();

    let error = store.list_fields(&passphrase()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered);
    assert!(error.to_string().contains("forks"));
}

#[test]
fn a_replayed_older_format_copy_of_a_witnessed_vault_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "vault");
    init_v2(&store, &passphrase());
    let (v2_vault, v2_audit) = snapshot_files(&store);
    store.migrate(&passphrase(), V3_FORMAT_VERSION).unwrap();

    std::fs::write(store.vault_path(), v2_vault).unwrap();
    std::fs::write(store.audit_path(), v2_audit).unwrap();
    let error = store.list_fields(&passphrase()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered);
    assert!(error.to_string().contains("already witnessed as format 3"));
    assert_stale_copy_guidance(&error);
    assert!(store.migrate(&passphrase(), V3_FORMAT_VERSION).is_err());
}

#[test]
fn unwitnessed_legacy_vaults_keep_legacy_behavior() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "vault");
    init_v2(&store, &passphrase());
    set_value(&store, "jig://Example/LEGACY", b"legacy value").unwrap();
    let id = store.header_vault_id_for_lock().unwrap();
    assert!(witness(&store).read_record(&id).unwrap().is_none());
}

#[test]
fn copies_sharing_a_vault_id_fence_each_other() {
    let temp = tempfile::tempdir().unwrap();
    let first = store_at(&temp, "first");
    first.init(&passphrase()).unwrap();
    set_value(&first, "jig://Example/SHARED", b"shared").unwrap();
    copy_home(&first, &temp.path().join("second"));
    let second = store_at(&temp, "second");
    second.list_fields(&passphrase()).unwrap();

    set_value(&first, "jig://Example/FIRST", b"first wins").unwrap();
    let before = snapshot_files(&second);
    assert_rollback(&set_value(&second, "jig://Example/SECOND", b"second loses").unwrap_err());
    assert_eq!(snapshot_files(&second), before);
}

#[test]
fn same_id_copies_serialize_on_one_witness_lock() {
    let temp = tempfile::tempdir().unwrap();
    let first = store_at(&temp, "first");
    first.init(&passphrase()).unwrap();
    copy_home(&first, &temp.path().join("second"));
    let second = store_at(&temp, "second");
    let id = first.header_vault_id_for_lock().unwrap();

    let held = std::time::Duration::from_millis(400);
    let witness_root = temp.path().join(".jig-vault-witness");
    let (locked_tx, locked_rx) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let location = crate::store::witness::WitnessLocation::at(witness_root);
        let witness = location.open_or_create().unwrap();
        let _lock = witness.lock_id(&id).unwrap();
        locked_tx.send(()).unwrap();
        std::thread::sleep(held);
    });
    locked_rx.recv().unwrap();
    let started = std::time::Instant::now();
    second.list(&passphrase()).unwrap();
    assert!(started.elapsed() >= held / 2, "{:?}", started.elapsed());
    holder.join().unwrap();
}

#[cfg(unix)]
#[test]
fn malformed_symlinked_or_unreadable_witness_records_fail_closed() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "vault");
    store.init(&passphrase()).unwrap();
    let path = record_path(&temp, &store);
    let original = std::fs::read(&path).unwrap();

    std::fs::write(&path, b"{not json").unwrap();
    assert!(store.list_fields(&passphrase()).is_err());

    std::fs::remove_file(&path).unwrap();
    let outside = temp.path().join("outside-record.json");
    std::fs::write(&outside, &original).unwrap();
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    assert!(store.list_fields(&passphrase()).is_err());

    std::fs::remove_file(&path).unwrap();
    std::fs::write(&path, &original).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Running as root bypasses mode bits; the remaining checks still apply.
    if unsafe { libc::geteuid() } != 0 {
        assert!(store.list_fields(&passphrase()).is_err());
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    store.list_fields(&passphrase()).unwrap();
}

#[test]
fn a_missing_record_reenrolls_as_first_use() {
    // Deleting the witness is outside the guarantee: it looks like first use.
    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "vault");
    store.init(&passphrase()).unwrap();
    set_value(&store, "jig://Example/ONE", b"first").unwrap();
    let path = record_path(&temp, &store);
    std::fs::remove_file(&path).unwrap();
    store.list_fields(&passphrase()).unwrap();
    assert!(path.exists());
}

#[test]
fn the_isolated_test_witness_lives_beside_the_vault_home() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "vault");
    store.init(&passphrase()).unwrap();
    assert!(record_path(&temp, &store).exists());
    assert!(!store.root().join(".jig-vault-witness").exists());
}

#[test]
fn a_vault_home_inside_the_witness_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let inside = temp.path().join("outer/.jig-vault-witness/vault");
    std::fs::create_dir_all(inside.parent().unwrap()).unwrap();
    let error = VaultStore::resolve_for_test(Some(temp.path().join("outer/.jig-vault-witness")))
        .unwrap_err();
    assert!(error.to_string().contains("must not overlap"), "{error}");
}

#[cfg(unix)]
#[test]
fn private_outputs_refuse_witness_destinations_and_hard_link_aliases() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    vault
        .set_field(
            &passphrase(),
            field("jig://Example/TOKEN"),
            FieldKind::Concealed,
            SecretBytes::new(b"output-test-secret".to_vec()),
        )
        .unwrap();
    let witness_root = std::fs::canonicalize(temp.path().join(".jig-vault-witness")).unwrap();
    let id = vault.store.header_vault_id_for_lock().unwrap();
    let record = witness_root
        .join("ids")
        .join(format!("{}.json", crate::store::witness::id_key(&id)));
    let before = std::fs::read(&record).unwrap();

    let inside = witness_root.join("journals/exported.txt");
    let error = vault.preflight_private_output(&inside, true).unwrap_err();
    assert!(error.to_string().contains("rollback witness"), "{error}");
    assert!(
        vault
            .read_field_to_file(&passphrase(), field("jig://Example/TOKEN"), &inside, true)
            .is_err()
    );

    let alias = temp.path().join("record-alias.json");
    std::fs::hard_link(&record, &alias).unwrap();
    let error = vault.preflight_private_output(&alias, true).unwrap_err();
    assert!(error.to_string().contains("rollback witness"), "{error}");
    assert!(Vault::preflight_backup_create(vault.root().to_path_buf(), &alias, true).is_err());
    assert_eq!(std::fs::read(&record).unwrap(), before);
}

#[test]
fn a_state_whose_mac_is_not_its_mutation_event_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "vault");
    store.init(&passphrase()).unwrap();
    let init_mac = audit_events(&store)[0].mac.clone();
    set_value(&store, "jig://Example/ONE", b"first").unwrap();

    let mut file: VaultFile =
        serde_json::from_str(&store.read_vault_text().unwrap().unwrap()).unwrap();
    let mut state: serde_json::Value =
        serde_json::from_slice(&decrypt_state_for_test(&file, &passphrase())).unwrap();
    let dek = store.open_unlocked(&passphrase()).unwrap().dek;
    // Point generation 2 at the generation 1 initialization event.
    state["mutation_audit_mac"] = serde_json::json!(init_mac);
    let plaintext = Zeroizing::new(serde_json::to_vec(&state).unwrap());
    let nonce = random_array::<NONCE_LEN>().unwrap();
    file.state_b64 = B64.encode(
        seal(
            &dek,
            &nonce,
            &payload_aad(&file.header, AeadRole::State),
            &plaintext,
        )
        .unwrap(),
    );
    file.state_nonce_b64 = B64.encode(nonce);
    store
        .write_vault_text(&serde_json::to_string_pretty(&file).unwrap())
        .unwrap();

    let error = store.list_fields(&passphrase()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered);
    assert!(error.to_string().contains("not anchored"), "{error}");
}

#[test]
fn retained_handles_refuse_to_append_while_a_transaction_is_pending() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "vault");
    store.init(&passphrase()).unwrap();
    set_value(&store, "jig://Example/TOKEN", b"retained-handle-value").unwrap();
    let reveal = store
        .prepare_field_read(&passphrase(), field("jig://Example/TOKEN"))
        .unwrap();
    store.arm_fault_for_test(FaultPoint::AfterPending);
    set_value(&store, "jig://Example/OTHER", b"pending value").unwrap_err();

    let mut sink = Vec::new();
    let error = reveal.write_to(&mut sink).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
    // The authenticated recovery still finishes the pending edit.
    assert_eq!(store.list_fields(&passphrase()).unwrap().len(), 2);
}

#[test]
fn retained_handles_refuse_an_audit_reverted_before_the_mutation_anchor() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "vault");
    store.init(&passphrase()).unwrap();
    let before_mutation = std::fs::read(store.audit_path()).unwrap();
    set_value(&store, "jig://Example/TOKEN", b"retained-handle-value").unwrap();
    let reveal = store
        .prepare_field_read(&passphrase(), field("jig://Example/TOKEN"))
        .unwrap();
    // A valid chain prefix that predates the committed mutation event.
    std::fs::write(store.audit_path(), &before_mutation).unwrap();

    let error = reveal.write_to(&mut Vec::new()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
    assert_eq!(std::fs::read(store.audit_path()).unwrap(), before_mutation);
}

#[test]
fn retained_handles_check_the_identity_they_authenticated_not_the_header() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "vault");
    store.init(&passphrase()).unwrap();
    set_value(&store, "jig://Example/TOKEN", b"retained-handle-value").unwrap();
    let reveal = store
        .prepare_field_read(&passphrase(), field("jig://Example/TOKEN"))
        .unwrap();
    store.arm_fault_for_test(FaultPoint::AfterPending);
    set_value(&store, "jig://Example/OTHER", b"pending value").unwrap_err();
    // The public header now claims an unwitnessed legacy vault.
    let mut file: serde_json::Value =
        serde_json::from_slice(&std::fs::read(store.vault_path()).unwrap()).unwrap();
    file["header"]["version"] = 2.into();
    file["header"]["vault_id"] = "01EXAMPLEUNWITNESSEDVAULT000".into();
    std::fs::write(store.vault_path(), serde_json::to_vec(&file).unwrap()).unwrap();
    let audit = std::fs::read(store.audit_path()).unwrap();

    let error = reveal.write_to(&mut Vec::new()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
    assert!(error.to_string().contains("identity changed"), "{error}");
    assert_eq!(std::fs::read(store.audit_path()).unwrap(), audit);
}

#[test]
fn retained_handles_still_finish_after_a_completed_rotation() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "vault");
    let new = SecretString::from("replacement passphrase for retained".to_owned());
    store.init(&passphrase()).unwrap();
    set_value(&store, "jig://Example/TOKEN", b"retained-handle-value").unwrap();
    let reveal = store
        .prepare_field_read(&passphrase(), field("jig://Example/TOKEN"))
        .unwrap();
    store
        .change_passphrase_for_test(&passphrase(), &new)
        .unwrap();

    let mut sink = Vec::new();
    reveal.write_to(&mut sink).unwrap();
    assert_eq!(sink, b"retained-handle-value");
    assert_eq!(
        audit_events(&store).last().unwrap().action,
        "field_read_finish"
    );
    store.verify_audit(&new).unwrap();
}

#[test]
fn status_reports_a_pending_transaction_without_side_effects() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "vault");
    store.arm_fault_for_test(FaultPoint::AfterPending);
    store.init(&passphrase()).unwrap_err();

    let status = Vault::status(Some(store.root().to_path_buf())).unwrap();
    assert!(status.pending_transaction);
    assert!(!status.exists);
    store.init(&passphrase()).unwrap();
    let status = Vault::status(Some(store.root().to_path_buf())).unwrap();
    assert!(!status.pending_transaction);
    assert!(status.exists);
}

#[test]
fn status_propagates_damaged_witness_discovery_without_writing() {
    let temp = tempfile::tempdir().unwrap();
    let pending = store_at(&temp, "pending");
    pending.arm_fault_for_test(FaultPoint::AfterPending);
    pending.init(&passphrase()).unwrap_err();
    let unrelated = store_at(&temp, "unrelated");
    unrelated.init(&passphrase()).unwrap();
    let damaged = record_path(&temp, &unrelated);
    let malformed = br#"{"schema":"ExamplePrivateRecordSentinel"}"#;
    std::fs::write(&damaged, malformed).unwrap();
    let reported_path = format!("{:?}", std::fs::canonicalize(&damaged).unwrap());
    let journal = witness(&pending)
        .read_journal(&pending.target_key())
        .unwrap()
        .unwrap()
        .1;
    let (status, operations) = crate::store::durable::recording::record(|| {
        Vault::status(Some(pending.root().to_path_buf()))
    });
    let error = status.unwrap_err();
    assert!(error.to_string().contains("pending vault transactions"));
    assert!(error.to_string().contains(&reported_path), "{error}");
    assert!(error.to_string().contains("Operator step:"), "{error}");
    // Both text and JSON CLI errors render the complete anyhow source chain.
    let message = format!("{:#}", anyhow::Error::new(error));
    assert!(!message.contains("ExamplePrivateRecordSentinel"));
    let error = unrelated.list_fields(&passphrase()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered);
    assert!(error.to_string().contains(&reported_path), "{error}");
    let message = format!("{:#}", anyhow::Error::new(error));
    assert!(!message.contains("ExamplePrivateRecordSentinel"));
    assert!(operations.is_empty());
    assert!(!pending.vault_path().exists());
    assert_eq!(std::fs::read(&damaged).unwrap(), malformed);
    assert_eq!(
        witness(&pending)
            .read_journal(&pending.target_key())
            .unwrap()
            .unwrap()
            .1,
        journal
    );
}

#[test]
fn status_distinguishes_orphan_init_journals_from_missing_pending_journals() {
    for point in [FaultPoint::AfterJournal, FaultPoint::AfterPending] {
        let temp = tempfile::tempdir().unwrap();
        let store = store_at(&temp, "ExampleVault");
        store.arm_fault_for_test(point);
        store.init(&passphrase()).unwrap_err();
        let witness = store.witness().open_existing().unwrap().unwrap();
        let before = witness.read_journal(&store.target_key()).unwrap().unwrap();

        let status = Vault::status(Some(store.root().to_path_buf())).unwrap();
        assert_eq!(
            status.pending_transaction,
            point == FaultPoint::AfterPending
        );
        assert!(!status.exists);
        assert!(store.has_pending_transaction().unwrap());
        assert_eq!(
            witness.read_journal(&store.target_key()).unwrap().unwrap(),
            before
        );

        if point == FaultPoint::AfterPending {
            // A missing journal must not disguise a committed recovery obligation.
            std::fs::remove_file(
                temp.path()
                    .join(".jig-vault-witness/journals")
                    .join(format!("{}.json", store.target_key())),
            )
            .unwrap();
            assert!(
                Vault::status(Some(store.root().to_path_buf()))
                    .unwrap()
                    .pending_transaction
            );
            assert!(store.init(&passphrase()).is_err());
        } else {
            store.init(&passphrase()).unwrap();
            assert!(store.list_fields(&passphrase()).unwrap().is_empty());
        }
    }
}

#[test]
fn legacy_access_with_an_unavailable_witness_fails_without_changing_the_vault() {
    for version in [V1_FORMAT_VERSION, V2_FORMAT_VERSION] {
        let temp = tempfile::tempdir().unwrap();
        let store = store_at(&temp, "ExampleVault");
        init_with_format(&store, &passphrase(), version);
        let before_vault = std::fs::read(store.vault_path()).unwrap();
        let before_audit = std::fs::read(store.audit_path()).unwrap();
        let blocked = temp.path().join("unavailable-witness");
        std::fs::write(&blocked, b"not a witness directory").unwrap();
        let _override = crate::store::witness::override_root_for_test(blocked.clone());
        let reopened = VaultStore::resolve_for_test(Some(store.root().to_path_buf())).unwrap();

        assert!(reopened.list(&passphrase()).is_err());
        assert_eq!(std::fs::read(store.vault_path()).unwrap(), before_vault);
        assert_eq!(std::fs::read(store.audit_path()).unwrap(), before_audit);
        assert_eq!(std::fs::read(blocked).unwrap(), b"not a witness directory");
    }
}

#[test]
fn an_interrupted_enrollment_is_made_durable_before_a_retry_accepts_it() {
    use crate::store::durable::recording::{FsOp, fail_next_sync_of, record};

    let temp = tempfile::tempdir().unwrap();
    let store = store_at(&temp, "vault");
    store.init(&passphrase()).unwrap();
    // An interrupted enrollment leaves its record published by rename but
    // never synced; rewriting it outside the store reproduces that state.
    let record_path = record_path(&temp, &store);
    let published = std::fs::read(&record_path).unwrap();
    std::fs::remove_file(&record_path).unwrap();
    std::fs::write(&record_path, published).unwrap();
    std::fs::set_permissions(
        &record_path,
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )
    .unwrap();
    let ids = std::fs::canonicalize(record_path.parent().unwrap()).unwrap();

    // A failed sync never lets the retry accept the state.
    fail_next_sync_of(&ids);
    assert!(store.list_fields(&passphrase()).is_err());

    // The retry has no journal; it still syncs the visible record before
    // accepting the state against it.
    let (listed, ops) = record(|| store.list_fields(&passphrase()));
    listed.unwrap();
    assert!(ops.contains(&FsOp::SyncDir(ids)), "{ops:?}");
}
