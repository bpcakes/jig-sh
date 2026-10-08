use super::*;
use crate::test_support::with_passphrase_estimate_for_test;

#[test]
fn recorded_rekey_retries_ignore_a_changed_estimate_but_new_changes_do_not() {
    for point in [
        FaultPoint::AfterPending,
        FaultPoint::AfterAudit,
        FaultPoint::AfterEnvelope,
    ] {
        let (_temp, store) = new_store();
        let old = passphrase();
        let new = SecretString::from("passwordpasswordpassword".to_owned());
        store.init(&old).unwrap();
        set_value(&store, "jig://Example/KEPT", b"kept value").unwrap();
        store.arm_fault_for_test(point);
        // Model acceptance when the transaction was recorded; the ordinary
        // estimator rejects this candidate after the scoped override ends.
        with_passphrase_estimate_for_test(u64::MAX, || {
            assert!(store.change_passphrase_for_test(&old, &new).is_err());
        });
        assert!(crate::validate_new_vault_passphrase(&new).is_err());
        let candidate = journal_candidate(&store).unwrap();
        store.change_passphrase_for_test(&old, &new).unwrap();
        assert_eq!(store.read_vault_text().unwrap().unwrap(), candidate);
        assert_eq!(committed_generation(&store), 3);
        assert!(journal_candidate(&store).is_none());
        assert_eq!(store.list_fields(&new).unwrap().len(), 1);
        assert_eq!(
            store.list(&old).unwrap_err().kind(),
            VaultErrorKind::Authentication
        );
        assert_eq!(
            audit_events(&store)
                .iter()
                .filter(|event| event.action == "passphrase_change")
                .count(),
            1
        );

        let before_audit = store.read_audit_text().unwrap();
        let error = store.change_passphrase_for_test(&new, &new).unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
        assert_eq!(error.message(), crate::NEW_VAULT_PASSPHRASE_POLICY);
        assert_eq!(store.read_vault_text().unwrap().unwrap(), candidate);
        assert_eq!(store.read_audit_text().unwrap(), before_audit);
    }
}

#[test]
fn a_rejected_nonmatching_credential_cannot_finish_a_pending_rekey() {
    let (_temp, store) = new_store();
    let old = passphrase();
    let new = SecretString::from("new correct horse battery staple".to_owned());
    store.init(&old).unwrap();
    store.arm_fault_for_test(FaultPoint::AfterPending);
    assert!(store.change_passphrase_for_test(&old, &new).is_err());
    let before_vault = store.read_vault_text().unwrap();
    let before_audit = store.read_audit_text().unwrap();
    let before_record = witness(&store).read_record(&vault_id(&store)).unwrap();
    let candidate = journal_candidate(&store);
    let rejected = SecretString::from("passwordpasswordpassword".to_owned());
    let error = store
        .change_passphrase_for_test(&old, &rejected)
        .unwrap_err();
    assert_eq!(error.message(), crate::NEW_VAULT_PASSPHRASE_POLICY);
    assert_eq!(store.read_vault_text().unwrap(), before_vault);
    assert_eq!(store.read_audit_text().unwrap(), before_audit);
    assert_eq!(
        witness(&store).read_record(&vault_id(&store)).unwrap(),
        before_record
    );
    assert_eq!(journal_candidate(&store), candidate);
}

#[test]
fn a_rejected_credential_does_not_finish_another_operation_or_delete_an_orphan() {
    for initializing in [true, false] {
        for point in [FaultPoint::AfterJournal, FaultPoint::AfterPending] {
            let (_temp, store) = new_store();
            if !initializing {
                store.init(&passphrase()).unwrap();
            }
            store.arm_fault_for_test(point);
            let result = if initializing {
                store.init(&passphrase())
            } else {
                set_value(&store, "jig://Example/KEPT", b"pending value").map(|_| ())
            };
            assert!(result.is_err());
            let before_vault = store.read_vault_text().unwrap();
            let before_audit = store.read_audit_text().unwrap();
            let candidate = journal_candidate(&store);
            let (journal, _) = witness(&store)
                .read_journal(&store.target_key())
                .unwrap()
                .unwrap();
            let before_record = witness(&store).read_record(&journal.vault_id).unwrap();
            // This credential authenticates the pending operation but must
            // not authorize completing it as a rejected passphrase change.
            let error = with_passphrase_estimate_for_test(0, || {
                store
                    .change_passphrase_for_test(&passphrase(), &passphrase())
                    .unwrap_err()
            });
            assert_eq!(error.message(), crate::NEW_VAULT_PASSPHRASE_POLICY);
            assert_eq!(store.read_vault_text().unwrap(), before_vault);
            assert_eq!(store.read_audit_text().unwrap(), before_audit);
            assert_eq!(journal_candidate(&store), candidate);
            assert_eq!(
                witness(&store).read_record(&journal.vault_id).unwrap(),
                before_record
            );
        }
    }
}
