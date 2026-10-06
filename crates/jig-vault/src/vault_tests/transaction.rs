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
