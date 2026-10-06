use super::*;

const VAULT_ID: &str = "01EXAMPLEWITNESSVAULTID00000";
const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn checkpoint(generation: u64) -> Checkpoint {
    Checkpoint {
        generation,
        envelope_sha256: DIGEST_A.into(),
        mutation_audit_mac: DIGEST_B.into(),
    }
}

fn committed_record(generation: u64) -> WitnessRecord {
    let mut record = WitnessRecord::new(VAULT_ID);
    record.committed = Some(checkpoint(generation));
    record
}

fn witness() -> (tempfile::TempDir, WitnessStore) {
    let temp = tempfile::tempdir().unwrap();
    let store = WitnessLocation::at(temp.path().join("witness"))
        .open_or_create()
        .unwrap();
    (temp, store)
}

fn journal(target_key: &str) -> Journal {
    Journal {
        schema: JOURNAL_SCHEMA,
        operation: TransactionKind::Edit,
        vault_id: VAULT_ID.into(),
        target: JournalTarget {
            target_key: target_key.into(),
            parent_device: 1,
            parent_inode: 2,
        },
        previous: Some(checkpoint(1)),
        previous_envelope_sha256: Some(DIGEST_A.into()),
        next: checkpoint(2),
        payload: JournalPayload::InPlace(InPlacePayload {
            candidate_envelope: "{\"envelope\":true}".into(),
            audit: AuditTransition {
                prefix_len: 10,
                prefix_tip_mac: Some(DIGEST_B.into()),
                torn_suffix_len: 0,
                torn_suffix_sha256: None,
                append: "{\"event\":1}\n".into(),
            },
        }),
    }
}

#[test]
fn per_user_root_is_independent_of_vault_home_selection() {
    assert_eq!(
        per_user_root(Path::new("/home/example")).unwrap(),
        Path::new("/home/example/.jig/vault-witness")
    );
}

#[test]
fn test_builds_isolate_the_witness_beside_the_vault_home() {
    let temp = tempfile::tempdir().unwrap();
    let home = crate::path_security::physical_path(&temp.path().join("vault"), "test").unwrap();
    if std::env::var_os(WITNESS_ROOT_ENV_FOR_TESTS).is_none() {
        let location = WitnessLocation::for_home(&home).unwrap();
        assert_eq!(
            location.root(),
            home.parent().unwrap().join(".jig-vault-witness")
        );
        location.ensure_disjoint(&home).unwrap();
    }
}

#[test]
fn overlapping_vault_homes_and_witnesses_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let witness = WitnessLocation::at(temp.path().join("witness"));
    for home in [
        temp.path().join("witness"),
        temp.path().join("witness/ids/inner"),
        temp.path().to_path_buf(),
    ] {
        let error = witness.ensure_disjoint(&home).unwrap_err().to_string();
        assert!(error.contains("must not overlap"), "{}", home.display());
    }
    witness.ensure_disjoint(&temp.path().join("vault")).unwrap();
    assert!(
        witness
            .contains(&temp.path().join("witness/journals/x.json"))
            .unwrap()
    );
    assert!(!witness.contains(&temp.path().join("vault/output")).unwrap());
}

#[test]
fn opening_an_absent_witness_creates_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let location = WitnessLocation::at(temp.path().join("witness"));
    assert!(location.open_existing().unwrap().is_none());
    assert!(!temp.path().join("witness").exists());
}

#[cfg(unix)]
#[test]
fn created_witness_tree_is_private() {
    use std::os::unix::fs::PermissionsExt;

    let (_temp, store) = witness();
    for path in [
        store.root().to_path_buf(),
        store.root().join(IDS_DIR),
        store.root().join(JOURNALS_DIR),
        store.root().join(LOCKS_DIR),
    ] {
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "{}", path.display());
    }
    store.write_record(&committed_record(1)).unwrap();
    let record = store.record_path(VAULT_ID);
    assert_eq!(
        fs::metadata(record).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn keys_are_fixed_size_and_domain_separated() {
    let id = id_key(VAULT_ID);
    let target = target_key(Path::new(VAULT_ID));
    assert!(record::is_hex_digest(&id));
    assert!(record::is_hex_digest(&target));
    assert_ne!(id, target);
    assert_ne!(id_key("../escape"), id_key("escape"));
    assert!(!id_key("../escape").contains('/'));
}

#[test]
fn records_round_trip_and_reject_foreign_or_malformed_contents() {
    let (_temp, store) = witness();
    assert!(store.read_record(VAULT_ID).unwrap().is_none());
    let record = committed_record(3);
    store.write_record(&record).unwrap();
    assert_eq!(store.read_record(VAULT_ID).unwrap(), Some(record));

    let path = store.record_path(VAULT_ID);
    let mut foreign = committed_record(3);
    foreign.vault_id = "01SOMEOTHERVAULTID000000000".into();
    fs::write(&path, serde_json::to_vec(&foreign).unwrap()).unwrap();
    assert!(store.read_record(VAULT_ID).is_err());

    for malformed in [
        "{not json".to_owned(),
        serde_json::json!({"schema": 2, "vault_id": VAULT_ID, "min_format": 3, "committed": checkpoint(1), "pending": null}).to_string(),
        serde_json::json!({"schema": 1, "vault_id": VAULT_ID, "min_format": 3, "committed": null, "pending": null}).to_string(),
        serde_json::json!({"schema": 1, "vault_id": VAULT_ID, "min_format": 3, "committed": checkpoint(1), "pending": null, "extra": true}).to_string(),
    ] {
        fs::write(&path, malformed).unwrap();
        assert!(store.read_record(VAULT_ID).is_err());
    }
}

#[test]
fn pending_marker_must_advance_its_checkpoint() {
    let mut record = committed_record(4);
    record.pending = Some(PendingMarker {
        operation: TransactionKind::Edit,
        target_key: DIGEST_A.into(),
        journal_sha256: DIGEST_B.into(),
        next: checkpoint(4),
    });
    assert!(record.validate(VAULT_ID).is_err());
    record.pending.as_mut().unwrap().next.generation = 5;
    record.validate(VAULT_ID).unwrap();
}

#[cfg(unix)]
#[test]
fn symlinked_shared_or_oversized_records_fail_closed() {
    use std::os::unix::fs::PermissionsExt;

    let (temp, store) = witness();
    let path = store.record_path(VAULT_ID);
    let outside = temp.path().join("outside.json");
    fs::write(&outside, serde_json::to_vec(&committed_record(1)).unwrap()).unwrap();
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    assert!(store.read_record(VAULT_ID).is_err());
    fs::remove_file(&path).unwrap();

    store.write_record(&committed_record(1)).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.read_record(VAULT_ID).is_err());

    fs::remove_file(&path).unwrap();
    let file = File::create(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    file.set_len(RECORD_READ_LIMIT + 1).unwrap();
    assert!(store.read_record(VAULT_ID).is_err());
}

#[test]
fn journals_round_trip_with_the_digest_of_their_exact_bytes() {
    let (_temp, store) = witness();
    let key = target_key(Path::new("/example/home"));
    let journal = journal(&key);
    let digest = store.write_journal(&journal).unwrap();
    let (read, read_digest) = store.read_journal(&key).unwrap().unwrap();
    assert_eq!(read, journal);
    assert_eq!(read_digest, digest);
    assert_eq!(
        digest,
        sha256_hex(&fs::read(store.journal_path(&key)).unwrap())
    );

    let other = target_key(Path::new("/example/other"));
    fs::copy(store.journal_path(&key), store.journal_path(&other)).unwrap();
    assert!(store.read_journal(&other).is_err());

    store.remove_journal(&key).unwrap();
    store.remove_journal(&key).unwrap();
    assert!(store.read_journal(&key).unwrap().is_none());
}

#[test]
fn journals_validate_their_audit_transition_and_payload_kind() {
    let key = target_key(Path::new("/example/home"));
    let mut journal = journal(&key);
    if let JournalPayload::InPlace(payload) = &mut journal.payload {
        payload.audit.append = "no trailing newline".into();
    }
    assert!(journal.validate().is_err());

    let mut restore = self::journal(&key);
    restore.operation = TransactionKind::Restore;
    assert!(restore.validate().is_err());
}

#[test]
fn locks_are_reentrant_within_a_thread_and_exclusive_across_descriptors() {
    let (_temp, store) = witness();
    let path = store
        .root()
        .join(LOCKS_DIR)
        .join(format!("id-{}.lock", id_key(VAULT_ID)));
    let outer = store.lock_id(VAULT_ID).unwrap();
    let inner = store.lock_id(VAULT_ID).unwrap();
    let probe = File::open(&path).unwrap();
    assert!(!probe.try_lock_exclusive().unwrap());
    drop(outer);
    assert!(!probe.try_lock_exclusive().unwrap());
    drop(inner);
    assert!(probe.try_lock_exclusive().unwrap());
    FileExt::unlock(&probe).unwrap();
}
