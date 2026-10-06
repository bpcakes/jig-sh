//! Crash consistency of witnessed format 3 transactions.

use super::*;
use crate::store::FaultPoint;
use crate::store::witness::{JournalPayload, TransactionKind, WitnessStore};

fn field(reference: &str) -> VaultReference {
    VaultReference::parse(reference).unwrap()
}

fn new_store() -> (tempfile::TempDir, VaultStore) {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    (temp, store)
}

fn witness(store: &VaultStore) -> WitnessStore {
    store.witness().open_existing().unwrap().unwrap()
}

fn vault_id(store: &VaultStore) -> String {
    store.header_vault_id_for_lock().unwrap()
}

fn committed_generation(store: &VaultStore) -> u64 {
    let record = witness(store)
        .read_record(&vault_id(store))
        .unwrap()
        .unwrap();
    assert!(record.pending.is_none());
    record.committed.unwrap().generation
}

fn pending_kind(store: &VaultStore, vault_id: &str) -> Option<TransactionKind> {
    witness(store)
        .read_record(vault_id)
        .unwrap()
        .and_then(|record| record.pending)
        .map(|pending| pending.operation)
}

fn journal_candidate(store: &VaultStore) -> Option<String> {
    let (journal, _) = witness(store).read_journal(&store.target_key()).unwrap()?;
    match journal.payload {
        JournalPayload::InPlace(payload) => Some(payload.candidate_envelope),
        JournalPayload::Restore(_) => None,
    }
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

fn field_names(store: &VaultStore) -> Vec<String> {
    store
        .list_fields(&passphrase())
        .unwrap()
        .into_iter()
        .map(|record| record.reference.to_string())
        .collect()
}

#[test]
fn a_crash_before_the_pending_marker_leaves_the_previous_state_usable() {
    let (_temp, store) = new_store();
    store.init(&passphrase()).unwrap();
    let before_vault = store.read_vault_text().unwrap().unwrap();
    let before_audit = store.read_audit_text().unwrap().unwrap();
    store.arm_fault_for_test(FaultPoint::AfterJournal);

    let error = set_value(&store, "jig://Example/LOST", b"never committed").unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Io);
    assert!(journal_candidate(&store).is_some());
    assert_eq!(pending_kind(&store, &vault_id(&store)), None);
    assert_eq!(store.read_vault_text().unwrap().unwrap(), before_vault);
    assert_eq!(store.read_audit_text().unwrap().unwrap(), before_audit);

    // The unreferenced journal is discarded and the old state stays valid.
    assert!(field_names(&store).is_empty());
    assert!(journal_candidate(&store).is_none());
    assert_eq!(committed_generation(&store), 1);
    set_value(&store, "jig://Example/NEXT", b"next value").unwrap();
    assert_eq!(committed_generation(&store), 2);
}

#[test]
fn every_crash_after_the_pending_marker_finishes_the_exact_candidate() {
    for point in [
        FaultPoint::AfterPending,
        FaultPoint::PartialAudit,
        FaultPoint::AfterAudit,
        FaultPoint::AfterEnvelope,
        FaultPoint::AfterPromotion,
    ] {
        let (_temp, store) = new_store();
        store.init(&passphrase()).unwrap();
        let before_events = audit_events(&store).len();
        store.arm_fault_for_test(point);

        let error = set_value(&store, "jig://Example/KEPT", b"kept value").unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::Io, "{point:?}");
        assert!(error.to_string().contains("did not finish"), "{point:?}");
        let candidate = journal_candidate(&store).expect("pending journal");

        // Any authenticated command finishes the recorded transaction.
        assert_eq!(field_names(&store), vec!["jig://Example/KEPT"], "{point:?}");
        assert_eq!(
            store.read_vault_text().unwrap().unwrap(),
            candidate,
            "{point:?}"
        );
        assert!(journal_candidate(&store).is_none(), "{point:?}");
        assert_eq!(committed_generation(&store), 2, "{point:?}");
        let events = audit_events(&store);
        assert_eq!(events.len(), before_events + 1, "{point:?}");
        assert_eq!(events.last().unwrap().details["generation"], 2);
        store.verify_audit(&passphrase()).unwrap();

        // Recovery is idempotent: reopening changes nothing further.
        let after = store.read_vault_text().unwrap().unwrap();
        field_names(&store);
        assert_eq!(store.read_vault_text().unwrap().unwrap(), after);
        assert_eq!(audit_events(&store).len(), before_events + 1);
    }
}

#[test]
fn a_crash_during_recovery_is_itself_recoverable() {
    let (_temp, store) = new_store();
    store.init(&passphrase()).unwrap();
    store.arm_fault_for_test(FaultPoint::AfterPending);
    set_value(&store, "jig://Example/KEPT", b"kept value").unwrap_err();
    let candidate = journal_candidate(&store).unwrap();
    for point in [
        FaultPoint::PartialAudit,
        FaultPoint::AfterAudit,
        FaultPoint::AfterEnvelope,
    ] {
        store.arm_fault_for_test(point);
        assert!(store.list_fields(&passphrase()).is_err(), "{point:?}");
    }
    assert_eq!(field_names(&store), vec!["jig://Example/KEPT"]);
    assert_eq!(store.read_vault_text().unwrap().unwrap(), candidate);
    assert_eq!(
        audit_events(&store)
            .iter()
            .filter(|event| event.action == "field_batch_apply")
            .count(),
        1
    );
}

#[test]
fn a_preexisting_torn_audit_tail_is_replaced_exactly_once() {
    let (_temp, store) = new_store();
    store.init(&passphrase()).unwrap();
    let mut audit = store.read_audit_text().unwrap().unwrap();
    audit.push_str("{\"partial\"");
    std::fs::write(store.audit_path(), &audit).unwrap();
    store.arm_fault_for_test(FaultPoint::AfterPending);
    set_value(&store, "jig://Example/KEPT", b"kept value").unwrap_err();
    // The torn tail is still present while the transaction is pending.
    assert!(
        store
            .read_audit_text()
            .unwrap()
            .unwrap()
            .ends_with("{\"partial\"")
    );

    assert_eq!(field_names(&store), vec!["jig://Example/KEPT"]);
    let events = audit_events(&store);
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].details["truncated_torn_tail_bytes"], 10);
    assert_eq!(
        store.verify_audit(&passphrase()).unwrap().torn_tail_bytes,
        0
    );
}

#[test]
fn unexpected_audit_bytes_during_recovery_fail_closed() {
    let (_temp, store) = new_store();
    store.init(&passphrase()).unwrap();
    store.arm_fault_for_test(FaultPoint::AfterPending);
    set_value(&store, "jig://Example/KEPT", b"kept value").unwrap_err();
    let mut audit = store.read_audit_text().unwrap().unwrap();
    audit.push_str("{\"unexpected\":true}\n");
    std::fs::write(store.audit_path(), &audit).unwrap();

    let error = store.list_fields(&passphrase()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered);
    assert!(pending_kind(&store, &vault_id(&store)).is_some());
}

#[test]
fn an_interrupted_init_resumes_with_its_original_passphrase() {
    for point in [FaultPoint::AfterPending, FaultPoint::AfterAudit] {
        let (_temp, store) = new_store();
        store.arm_fault_for_test(point);
        let error = store.init(&passphrase()).unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::Io, "{point:?}");
        assert!(!store.exists().unwrap());

        let wrong = SecretString::from("a different strong passphrase".to_owned());
        let error = store.init(&wrong).unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::Authentication);
        assert!(error.to_string().contains("initialization is pending"));
        assert!(!store.exists().unwrap());

        store.init(&passphrase()).unwrap();
        assert!(store.exists().unwrap());
        assert_eq!(committed_generation(&store), 1);
        store.verify_audit(&passphrase()).unwrap();
        assert_eq!(audit_events(&store).len(), 1);
    }
}

#[test]
fn any_authenticated_command_also_finishes_an_interrupted_init() {
    let (_temp, store) = new_store();
    store.arm_fault_for_test(FaultPoint::AfterAudit);
    store.init(&passphrase()).unwrap_err();
    assert!(store.list_fields(&passphrase()).unwrap().is_empty());
    assert!(store.exists().unwrap());
    assert_eq!(committed_generation(&store), 1);
}

#[test]
fn a_backup_finishes_an_interrupted_init_before_bounding_its_snapshot() {
    let (temp, store) = new_store();
    store.arm_fault_for_test(FaultPoint::AfterAudit);
    store.init(&passphrase()).unwrap_err();
    assert!(store.read_vault_text().unwrap().is_none());

    let output = temp.path().join("vault.backup");
    let request =
        Vault::preflight_backup_create(store.root().to_path_buf(), &output, false).unwrap();
    Vault::create_backup(&passphrase(), request).unwrap();
    assert!(output.exists());
    assert_eq!(committed_generation(&store), 1);
}

#[test]
fn a_pending_passphrase_change_finishes_only_with_the_new_passphrase() {
    let (_temp, store) = new_store();
    let old = passphrase();
    let new = SecretString::from("replacement passphrase after fault".to_owned());
    store.init(&old).unwrap();
    store.arm_fault_for_test(FaultPoint::AfterPending);
    let error = store.change_passphrase_for_test(&old, &new).unwrap_err();
    assert!(error.to_string().contains("with the new passphrase"));

    let error = store.list(&old).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Authentication);
    assert!(error.to_string().contains("unlock with the new passphrase"));
    assert!(pending_kind(&store, &vault_id(&store)).is_some());

    store.list(&new).unwrap();
    assert!(store.list(&old).is_err());
    assert_eq!(committed_generation(&store), 2);
}

#[test]
fn retrying_a_pending_passphrase_change_completes_it() {
    let (_temp, store) = new_store();
    let old = passphrase();
    let new = SecretString::from("replacement passphrase after fault".to_owned());
    store.init(&old).unwrap();
    store.arm_fault_for_test(FaultPoint::AfterAudit);
    store.change_passphrase_for_test(&old, &new).unwrap_err();

    store.change_passphrase_for_test(&old, &new).unwrap();
    store.list(&new).unwrap();
    assert_eq!(committed_generation(&store), 2);
    assert_eq!(
        audit_events(&store)
            .iter()
            .filter(|event| event.action == "passphrase_change")
            .count(),
        1
    );
}

#[test]
fn a_pending_migration_finishes_with_its_original_passphrase() {
    let (_temp, store) = new_store();
    init_v1(&store, &passphrase());
    store.arm_fault_for_test(FaultPoint::AfterPending);
    store.migrate(&passphrase(), V3_FORMAT_VERSION).unwrap_err();

    let migration = store.migrate(&passphrase(), V3_FORMAT_VERSION).unwrap();
    assert_eq!(migration.from_version, V3_FORMAT_VERSION);
    assert!(!migration.changed);
    assert_eq!(committed_generation(&store), 1);
}

#[test]
fn a_pending_migration_passes_format_preflights_and_backs_up_as_version_three() {
    let (temp, store) = new_store();
    init_v1(&store, &passphrase());
    store.arm_fault_for_test(FaultPoint::AfterAudit);
    store.migrate(&passphrase(), V3_FORMAT_VERSION).unwrap_err();

    let home = store.root().to_path_buf();
    Vault::preflight_passphrase_change(home.clone()).unwrap();
    let output = temp.path().join("vault.backup");
    let request = Vault::preflight_backup_create(home, &output, false).unwrap();
    Vault::create_backup(&passphrase(), request).unwrap();
    assert!(output.exists());
    assert_eq!(committed_generation(&store), 1);
}

#[test]
fn generation_overflow_is_refused_before_anything_is_written() {
    let (_temp, store) = new_store();
    store.init(&passphrase()).unwrap();
    let before = store.read_audit_text().unwrap().unwrap();
    store
        .with_lock(|| {
            let mut vault = store.open_unlocked(&passphrase())?;
            vault.state.v3.as_mut().unwrap().generation = u64::MAX;
            let error = vault
                .stage_v3_mutation(&store, AuditAction::SecretSet, serde_json::json!({}))
                .unwrap_err();
            assert!(error.to_string().contains("cannot advance"));
            Ok(())
        })
        .unwrap();
    assert_eq!(store.read_audit_text().unwrap().unwrap(), before);
}

fn journal_file(store: &VaultStore) -> std::path::PathBuf {
    let journals = store.witness().root().join("journals");
    let [path] = std::fs::read_dir(journals)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    path
}

/// Rewrites the journal to claim another valid vault ID; it stays parseable.
fn claim_other_vault(path: &std::path::Path) -> (Vec<u8>, Vec<u8>) {
    let original = std::fs::read(path).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    value["vault_id"] = "01EXAMPLEOTHERVAULTID0000000".into();
    let changed = serde_json::to_vec(&value).unwrap();
    std::fs::write(path, &changed).unwrap();
    (original, changed)
}

#[test]
fn a_pending_journal_claiming_another_valid_vault_is_kept_and_refused() {
    let (_temp, store) = new_store();
    store.init(&passphrase()).unwrap();
    store.arm_fault_for_test(FaultPoint::AfterPending);
    set_value(&store, "jig://Example/KEPT", b"kept value").unwrap_err();
    let path = journal_file(&store);
    let (original, changed) = claim_other_vault(&path);

    let error = store.list_fields(&passphrase()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
    assert!(
        error.to_string().contains("does not match its marker"),
        "{error}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), changed);
    assert!(pending_kind(&store, &vault_id(&store)).is_some());

    // The untouched recovery data still finishes the transaction.
    std::fs::write(&path, original).unwrap();
    assert_eq!(field_names(&store), vec!["jig://Example/KEPT"]);
    assert_eq!(committed_generation(&store), 2);
}

#[test]
fn an_unreferenced_journal_claiming_another_vault_is_discarded() {
    let (_temp, store) = new_store();
    store.init(&passphrase()).unwrap();
    store.arm_fault_for_test(FaultPoint::AfterJournal);
    set_value(&store, "jig://Example/LOST", b"never committed").unwrap_err();
    claim_other_vault(&journal_file(&store));

    assert!(field_names(&store).is_empty());
    assert!(journal_candidate(&store).is_none());
    assert_eq!(committed_generation(&store), 1);
}

#[test]
fn a_home_left_by_an_interrupted_attempt_is_durable_before_init_writes_its_journal() {
    use crate::store::durable::recording::{FsOp, record};
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(temp.path()).unwrap();
    // The witness lives on another branch, so its syncs cannot stand in for
    // the home's.
    let _witness = crate::store::witness::override_root_for_test(base.join("a/b/witness"));
    let outer = base.join("c");
    let inner = outer.join("d");
    let home = inner.join("vault");
    // An earlier attempt created the home and its parents, then crashed
    // before syncing any of their entries.
    for dir in [&outer, &inner, &home] {
        std::fs::create_dir(dir).unwrap();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    let ((), ops) = record(|| {
        let store = VaultStore::resolve_for_test(Some(home.clone())).unwrap();
        store.init(&passphrase()).unwrap();
    });
    let journal_written = ops
        .iter()
        .position(|op| matches!(op, FsOp::Rename(path) if path.parent().is_some_and(|dir| dir.ends_with("journals"))))
        .unwrap();
    for dir in [&inner, &outer] {
        let synced = ops
            .iter()
            .position(|op| *op == FsOp::SyncDir(dir.clone()))
            .unwrap_or_else(|| panic!("{} never synced: {ops:?}", dir.display()));
        assert!(synced < journal_written, "{ops:?}");
    }
}

#[test]
fn an_unsynced_existing_home_is_durable_before_an_edit_writes_its_journal() {
    use crate::store::durable::recording::{FsOp, record};
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(temp.path()).unwrap();
    let _witness = crate::store::witness::override_root_for_test(base.join("a/b/witness"));
    let original = VaultStore::resolve_for_test(Some(base.join("original/vault"))).unwrap();
    original.init(&passphrase()).unwrap();
    // Another process placed an identical copy into new directories and
    // crashed before syncing their entries; nothing here resolves it.
    let outer = base.join("c");
    let inner = outer.join("d");
    let home = inner.join("vault");
    for dir in [&outer, &inner, &home] {
        std::fs::create_dir(dir).unwrap();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    for name in ["vault.json", "audit.jsonl"] {
        std::fs::copy(original.root().join(name), home.join(name)).unwrap();
    }
    let copy = VaultStore::open_existing(home).unwrap();

    let (written, ops) = record(|| set_value(&copy, "jig://Example/COPY", b"copy value"));
    written.unwrap();
    let journal_written = ops
        .iter()
        .position(|op| matches!(op, FsOp::Rename(path) if path.parent().is_some_and(|dir| dir.ends_with("journals"))))
        .unwrap();
    for dir in [&inner, &outer] {
        let synced = ops
            .iter()
            .position(|op| *op == FsOp::SyncDir(dir.clone()))
            .unwrap_or_else(|| panic!("{} never synced: {ops:?}", dir.display()));
        assert!(synced < journal_written, "{ops:?}");
    }
}

#[test]
fn an_interrupted_init_whose_journal_is_missing_is_never_replaced_by_a_new_vault() {
    let (_temp, store) = new_store();
    store.arm_fault_for_test(FaultPoint::AfterPending);
    store.init(&passphrase()).unwrap_err();
    std::fs::remove_file(journal_file(&store)).unwrap();

    let error = store.init(&passphrase()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
    assert!(error.to_string().contains("journal is missing"), "{error}");
    assert!(!store.exists().unwrap());
}

#[test]
fn a_witness_root_left_without_its_directories_does_not_block_a_new_vault() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(temp.path()).unwrap();
    // The first command on this profile prepared the witness root, then
    // stopped before creating any of its directories.
    let witness_root = base.join(".jig-vault-witness");
    std::fs::create_dir(&witness_root).unwrap();
    std::fs::set_permissions(&witness_root, std::fs::Permissions::from_mode(0o700)).unwrap();

    let vault = Vault::resolve_for_test(Some(base.join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    assert!(witness_root.join("ids").is_dir());
    assert!(vault.list_fields(&passphrase()).unwrap().is_empty());
}

#[test]
fn the_bound_audit_prefix_and_predecessor_are_durable_before_the_journal() {
    use crate::store::durable::recording::{FsOp, record};

    let (_temp, store) = new_store();
    store.init(&passphrase()).unwrap();
    // An audit-only append or earlier save may have published these bytes
    // without syncing them; the transaction binds them in its journal.
    let (written, ops) = record(|| set_value(&store, "jig://Example/NEXT", b"next value"));
    written.unwrap();
    let journal_written = ops
        .iter()
        .position(|op| matches!(op, FsOp::Rename(path) if path.parent().is_some_and(|dir| dir.ends_with("journals"))))
        .unwrap();
    for state in [store.audit_path(), store.vault_path()] {
        let synced = ops
            .iter()
            .position(|op| *op == FsOp::SyncFile(state.clone()))
            .unwrap_or_else(|| panic!("{} never synced: {ops:?}", state.display()));
        assert!(synced < journal_written, "{ops:?}");
    }
}

#[test]
fn a_retried_change_to_the_same_passphrase_completes_without_another_rekey() {
    let (_temp, store) = new_store();
    let same = passphrase();
    store.init(&same).unwrap();
    store.arm_fault_for_test(FaultPoint::AfterPending);
    store.change_passphrase_for_test(&same, &same).unwrap_err();

    store.change_passphrase_for_test(&same, &same).unwrap();
    assert_eq!(committed_generation(&store), 2);
    assert_eq!(
        audit_events(&store)
            .iter()
            .filter(|event| event.action == "passphrase_change")
            .count(),
        1
    );
}
