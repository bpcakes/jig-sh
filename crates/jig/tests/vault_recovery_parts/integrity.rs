use super::*;

fn assert_integrity_guidance(error: &str) {
    assert!(
        error.contains("Operator step: preserve the vault, audit log, and recovery data"),
        "{error}"
    );
    assert!(error.contains("Never delete or edit the rollback witness or its journals"));
    assert!(error.contains("Agents must ask the operator"));
    assert!(!error.contains(PASSPHRASE));
    assert!(!error.contains("remove the stale vault home"));
    assert!(!error.contains("restore audit.jsonl"));
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
fn missing_audit_routes_all_formats_and_preflight_paths_to_the_operator() {
    for version in [1, 2, 3] {
        let temp = private_tempdir();
        let home = temp.path().join("ExampleVault");
        let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
        vault.init_format_for_test(&passphrase(), version).unwrap();
        let envelope = std::fs::read(home.join("vault.json")).unwrap();
        std::fs::remove_file(home.join("audit.jsonl")).unwrap();
        let output = temp.path().join("ExampleVault.backup");
        let listed = jig(&["field", "list"], &home);
        if version == 3 {
            assert_integrity_guidance(&failure(&listed));
        } else {
            // Legacy metadata listing does not verify audit history; retain
            // that behavior while testing the operations that do refuse it.
            assert!(json(&listed)["fields"].as_array().unwrap().is_empty());
        }
        for args in [
            vec!["audit", "verify"],
            vec!["backup", "create", "--out", output.to_str().unwrap()],
            vec!["passphrase", "change"],
        ] {
            let error = failure(&jig(&args, &home));
            assert_integrity_guidance(&error);
            assert!(
                error.contains("audit log is missing") || error.contains("missing required file"),
                "{error}"
            );
        }
        // Preflight must preserve its established I/O kind while supplying
        // recovery metadata, before passphrase capture or output creation.
        let error = Vault::preflight_passphrase_change(home.clone()).unwrap_err();
        assert_eq!(error.kind(), jig_vault::VaultErrorKind::Io);
        assert_eq!(error.recovery(), Some(jig_vault::VaultRecovery::Integrity));
        let error = Vault::preflight_backup_create(home.clone(), &output, false).unwrap_err();
        assert_eq!(error.kind(), jig_vault::VaultErrorKind::Io);
        assert_eq!(error.recovery(), Some(jig_vault::VaultRecovery::Integrity));
        assert_eq!(std::fs::read(home.join("vault.json")).unwrap(), envelope);
        assert!(!home.join("audit.jsonl").exists());
        assert!(!output.exists());
    }
}

#[test]
fn unrecorded_init_artifacts_are_preserved_with_operator_guidance() {
    let temp = private_tempdir();
    let home = temp.path().join("ExampleVault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    vault.init_format_for_test(&passphrase(), 2).unwrap();
    let audit = std::fs::read(home.join("audit.jsonl")).unwrap();
    std::fs::remove_file(home.join("vault.json")).unwrap();
    let error = failure(&jig(&["init"], &home));
    assert!(error.contains("audit log already exists"));
    assert_integrity_guidance(&error);
    assert_eq!(std::fs::read(home.join("audit.jsonl")).unwrap(), audit);
    assert!(!home.join("vault.json").exists());
}

#[test]
fn audit_removed_after_backup_preflight_preserves_the_revalidation_error_kind() {
    let temp = private_tempdir();
    let home = temp.path().join("ExampleVault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    vault.init(&passphrase()).unwrap();
    let output = temp.path().join("ExampleVault.backup");
    let request = Vault::preflight_backup_create(home.clone(), &output, false).unwrap();
    let envelope = std::fs::read(home.join("vault.json")).unwrap();
    std::fs::remove_file(home.join("audit.jsonl")).unwrap();
    let error = Vault::create_backup(&passphrase(), request).unwrap_err();
    assert_eq!(error.kind(), jig_vault::VaultErrorKind::InvalidInput);
    assert_eq!(error.recovery(), Some(jig_vault::VaultRecovery::Integrity));
    assert_eq!(std::fs::read(home.join("vault.json")).unwrap(), envelope);
    assert!(!home.join("audit.jsonl").exists());
    assert!(!output.exists());
}
