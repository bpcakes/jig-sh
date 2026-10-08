//! Retained history must not impose a lifetime quota on unrelated access.

use super::*;
use crate::Vault;
use secrecy::SecretString;

#[test]
fn history_beyond_the_former_production_cap_keeps_open_and_output_available() {
    let (temp, store) = witness();
    let _profile = override_root_for_test(store.root().to_path_buf());
    let vault = Vault::resolve_for_test(Some(temp.path().join("ExampleVault"))).unwrap();
    let passphrase = SecretString::from("capacity-fixture-vault-passphrase".to_owned());
    vault.init(&passphrase).unwrap();
    let id = vault.snapshot(&passphrase).unwrap().vault_id;
    let record_path = store.record_path(&id);
    let before = fs::read(&record_path).unwrap();

    // Unpublished temporary names count toward the old record-scan cap.
    // They do not pretend to be authoritative records. Exercise the actual
    // former production limit, not a smaller cfg(test) substitute.
    for index in 0..100_001 {
        fs::write(
            store.root().join(IDS_DIR).join(format!(".{index}.tmp")),
            b"",
        )
        .unwrap();
        fs::write(
            store
                .root()
                .join(LOCKS_DIR)
                .join(format!("history-{index}.lock")),
            b"",
        )
        .unwrap();
    }
    assert!(vault.list_fields(&passphrase).unwrap().is_empty());
    assert_eq!(fs::read(&record_path).unwrap(), before);

    // Existing ordinary outputs require an alias scan. New paths alone
    // would not exercise the former alias limit.
    let output = temp.path().join("output.txt");
    fs::write(&output, b"existing output").unwrap();
    vault.preflight_private_output(&output, true).unwrap();
    assert_eq!(fs::read(&output).unwrap(), b"existing output");
    assert_eq!(
        fs::read_dir(store.root().join(IDS_DIR)).unwrap().count(),
        100_002
    );
    assert!(fs::read_dir(store.root().join(LOCKS_DIR)).unwrap().count() > 100_000);

    let alias = temp.path().join("protected-output.txt");
    fs::hard_link(&record_path, &alias).unwrap();
    assert!(vault.preflight_private_output(&alias, true).is_err());
    assert_eq!(fs::read(&record_path).unwrap(), before);
}

#[test]
fn retained_records_do_not_hide_pending_or_invalid_authority() {
    let (_temp, store) = witness();
    let key = target_key(Path::new("/example/home"));
    for index in 0..128 {
        let mut record = committed_record(1);
        record.vault_id = format!("ExampleHistoricalVault{index}");
        store.write_record(&record).unwrap();
    }
    let lock = store.lock_target(&key).unwrap();
    assert!(matches!(
        store.classify_target_journal(&lock).unwrap(),
        TargetJournal::Absent
    ));
    let mut record = committed_record(1);
    record.pending = Some(PendingMarker {
        operation: TransactionKind::Edit,
        target_key: key.clone(),
        journal_sha256: DIGEST_A.into(),
        next: checkpoint(2),
    });
    store.write_record(&record).unwrap();
    assert!(store.target_has_pending_marker(&key).unwrap());
    let error = store.classify_target_journal(&lock).err().unwrap();
    assert!(error.to_string().contains("journal is missing"), "{error}");

    // Multiple matches retain bounded state, but remain visible to status
    // and cannot authorize orphan cleanup.
    store.write_journal(&journal(&key)).unwrap();
    for index in 0..8 {
        record.vault_id = format!("ExamplePendingVault{index}");
        store.write_record(&record).unwrap();
    }
    assert!(store.target_has_pending_marker(&key).unwrap());
    let error = store.classify_target_journal(&lock).err().unwrap();
    assert!(error.to_string().contains("several pending"), "{error}");
    assert!(store.journal_exists(&key));

    // Enumeration still validates every published record even after enough
    // matching markers have been retained to identify ambiguity.
    let malformed = store.record_path("ExampleMalformedVault");
    fs::write(malformed, b"{malformed").unwrap();
    assert!(store.target_has_pending_marker(&key).is_err());
    assert!(store.journal_exists(&key));
}
