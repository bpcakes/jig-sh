use super::*;

fn assert_integrity_guidance(error: &str) {
    assert!(
        error.contains("Operator step: preserve the vault, audit log, and recovery data"),
        "{error}"
    );
    assert!(error.contains("Never delete or edit the rollback witness or its journals"));
    assert!(error.contains("Agents must ask the operator"));
    assert!(!error.contains(PASSPHRASE));
}

#[test]
fn missing_or_mismatched_pending_journals_route_to_the_operator_without_repairing_data() {
    for missing in [true, false] {
        let temp = private_tempdir();
        let home = temp.path().join("ExampleVault");
        let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
        arm_transaction_fault(TransactionFaultPoint::AfterPending);
        assert!(vault.init(&passphrase()).is_err());
        let journal = std::fs::read_dir(temp.path().join(".jig-vault-witness/journals"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut bytes = std::fs::read(&journal).unwrap();
        if missing {
            std::fs::remove_file(&journal).unwrap();
        } else {
            // Valid JSON with a different digest from the authoritative marker.
            bytes.push(b'\n');
            std::fs::write(&journal, &bytes).unwrap();
        }
        let error = failure(&jig(&["init"], &home));
        assert_integrity_guidance(&error);
        assert!(
            error.contains(if missing {
                "journal is missing"
            } else {
                "journal does not match"
            }),
            "{error}"
        );
        assert_eq!(json(&jig(&["status"], &home))["pending_transaction"], true);
        assert!(!home.join("vault.json").exists());
        if missing {
            assert!(!journal.exists());
        } else {
            assert_eq!(std::fs::read(journal).unwrap(), bytes);
        }
    }
}

#[test]
fn missing_audit_routes_to_the_operator_and_preserves_the_envelope() {
    let temp = private_tempdir();
    let home = temp.path().join("ExampleVault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    vault.init(&passphrase()).unwrap();
    let envelope = std::fs::read(home.join("vault.json")).unwrap();
    std::fs::remove_file(home.join("audit.jsonl")).unwrap();
    let error = failure(&jig(&["field", "list"], &home));
    assert!(error.contains("audit log is missing"));
    assert!(!error.contains("restore audit.jsonl"));
    assert_integrity_guidance(&error);
    assert_eq!(std::fs::read(home.join("vault.json")).unwrap(), envelope);
    assert!(!home.join("audit.jsonl").exists());
}
