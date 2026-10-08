use super::*;

#[test]
fn oversized_audit_successor_is_rejected_before_any_transaction_write() {
    let (_temp, mut store) = new_store();
    store.init(&passphrase()).unwrap();
    let before_vault = std::fs::read(store.vault_path()).unwrap();
    let before_audit = std::fs::read(store.audit_path()).unwrap();
    let id = vault_id(&store);
    let before_record = witness(&store).read_record(&id).unwrap();
    store.set_audit_text_read_limit_for_test(before_audit.len() as u64 + 8);

    // The existing state is readable, but there is no room for its next event.
    assert!(field_names(&store).is_empty());
    let error = set_value(&store, "jig://Example/TOO_LARGE", b"unchanged").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("audit append failed before state save"),
        "{error}"
    );
    assert_eq!(std::fs::read(store.vault_path()).unwrap(), before_vault);
    assert_eq!(std::fs::read(store.audit_path()).unwrap(), before_audit);
    assert_eq!(witness(&store).read_record(&id).unwrap(), before_record);
    assert!(
        witness(&store)
            .read_journal(&store.target_key())
            .unwrap()
            .is_none()
    );
    assert!(field_names(&store).is_empty());
}

#[test]
fn recovery_can_read_an_audit_successor_exactly_at_the_limit() {
    let (_temp, mut store) = new_store();
    store.init(&passphrase()).unwrap();
    store.arm_fault_for_test(FaultPoint::AfterPending);
    set_value(&store, "jig://Example/KEPT", b"kept").unwrap_err();
    let (journal, _) = witness(&store)
        .read_journal(&store.target_key())
        .unwrap()
        .unwrap();
    let JournalPayload::InPlace(payload) = journal.payload else {
        panic!("in-place mutation")
    };
    let limit = payload.audit.prefix_len + payload.audit.append.len() as u64;
    store.set_audit_text_read_limit_for_test(limit);

    store.arm_fault_for_test(FaultPoint::AfterAudit);
    assert!(store.list_fields(&passphrase()).is_err());
    assert_eq!(store.audit_len().unwrap(), Some(limit));
    assert!(pending_kind(&store, &vault_id(&store)).is_some());
    assert_eq!(field_names(&store), vec!["jig://Example/KEPT"]);
    assert_eq!(committed_generation(&store), 2);
    assert!(journal_candidate(&store).is_none());
}
