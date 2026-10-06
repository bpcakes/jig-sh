//! Durability of witness entries after interrupted attempts, and journal
//! classification that never trusts the journal's own vault ID.

use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::store::durable::recording::{FsOp, fail_next_sync_of, record};

const OTHER_VAULT_ID: &str = "01EXAMPLEOTHERVAULTID0000000";

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap()
}

fn private_dirs(root: &Path, children: &[&str]) {
    fs::create_dir_all(root).unwrap();
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
    for child in children {
        fs::create_dir(root.join(child)).unwrap();
        fs::set_permissions(root.join(child), fs::Permissions::from_mode(0o700)).unwrap();
    }
}

fn syncs(ops: &[FsOp]) -> Vec<PathBuf> {
    ops.iter()
        .filter_map(|op| match op {
            FsOp::SyncDir(path) => Some(path.clone()),
            _ => None,
        })
        .collect()
}

fn position(ops: &[FsOp], wanted: &FsOp) -> usize {
    ops.iter()
        .position(|op| op == wanted)
        .unwrap_or_else(|| panic!("{wanted:?} missing from {ops:?}"))
}

/// A record for `VAULT_ID` whose pending marker names `journal`'s target
/// and binds its exact bytes.
fn pending_for(store: &WitnessStore, journal: &Journal) -> WitnessRecord {
    let digest = store.write_journal(journal).unwrap();
    let mut record = committed_record(1);
    record.pending = Some(PendingMarker {
        operation: journal.operation,
        target_key: journal.target.target_key.clone(),
        journal_sha256: digest,
        next: journal.next.clone(),
    });
    store.write_record(&record).unwrap();
    record
}

/// Rewrites the stored journal with another valid vault ID; it stays
/// parseable and valid, only its claim (and digest) changes.
fn claim_other_vault(store: &WitnessStore, target_key: &str) -> Vec<u8> {
    let path = store.journal_path(target_key);
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["vault_id"] = OTHER_VAULT_ID.into();
    let bytes = serde_json::to_vec(&value).unwrap();
    fs::write(&path, &bytes).unwrap();
    store.read_journal(target_key).unwrap().unwrap();
    bytes
}

#[test]
fn a_retried_open_syncs_entries_an_interrupted_attempt_left_unsynced() {
    let temp = tempfile::tempdir().unwrap();
    let profile = canonical(temp.path()).join("profile");
    let root = profile.join(".jig/vault-witness");
    // An earlier attempt created the whole tree, then crashed before any
    // containing-directory sync.
    private_dirs(&profile, &[]);
    private_dirs(&profile.join(".jig"), &[]);
    private_dirs(&root, &[IDS_DIR, JOURNALS_DIR, LOCKS_DIR]);

    let (opened, ops) = record(|| WitnessLocation::at(root.clone()).open_or_create());
    opened.unwrap();
    let synced = syncs(&ops);
    for directory in [&root, &profile.join(".jig"), &profile] {
        assert!(
            synced.contains(directory),
            "{directory:?} not in {synced:?}"
        );
    }

    // Once this process made them durable, later opens skip those syncs.
    let (opened, ops) = record(|| WitnessLocation::at(root.clone()).open_or_create());
    opened.unwrap();
    assert!(syncs(&ops).iter().all(|path| !path.starts_with(&profile)));
}

#[test]
fn a_cache_reset_only_forgets_entries_under_its_own_directory() {
    use crate::store::durable::recording::forget_durable_entries_under;

    let temp = tempfile::tempdir().unwrap();
    let profile = canonical(temp.path()).join("profile");
    let root = profile.join(".jig/vault-witness");
    let open = || record(|| WitnessLocation::at(root.clone()).open_or_create()).1;
    let synced_here = |ops: &[FsOp]| syncs(ops).iter().any(|path| path.starts_with(&profile));
    assert!(synced_here(&open()));

    // Another test resetting its own directory leaves these entries known.
    let other = tempfile::tempdir().unwrap();
    forget_durable_entries_under(&canonical(other.path()));
    assert!(!synced_here(&open()));

    forget_durable_entries_under(&profile);
    assert!(synced_here(&open()));
}

#[test]
fn a_failed_entry_sync_fails_the_open_and_is_retried() {
    let temp = tempfile::tempdir().unwrap();
    let root = canonical(temp.path()).join("profile/.jig/vault-witness");
    fail_next_sync_of(root.parent().unwrap());
    let error = WitnessLocation::at(root.clone())
        .open_or_create()
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("injected sync failure"),
        "{error:#}"
    );

    let (opened, ops) = record(|| WitnessLocation::at(root.clone()).open_or_create());
    opened.unwrap();
    assert!(syncs(&ops).contains(&root.parent().unwrap().to_path_buf()));
}

#[test]
fn a_returned_record_is_durable_and_a_failed_sync_returns_none() {
    let (_temp, store) = witness();
    store.write_record(&committed_record(1)).unwrap();
    let ids = store.root().join(IDS_DIR);

    let (read, ops) = record(|| store.read_record(VAULT_ID));
    assert!(read.unwrap().is_some());
    assert_eq!(syncs(&ops), vec![ids.clone()]);

    fail_next_sync_of(&ids);
    assert!(store.read_record(VAULT_ID).is_err());
}

#[test]
fn promotion_makes_its_record_durable_before_removing_the_journal() {
    let (_temp, store) = witness();
    let key = target_key(Path::new("/example/home"));
    let journal = journal(&key);
    let mut record_value = pending_for(&store, &journal);

    let (promoted, ops) = record(|| store.promote(&journal, &mut record_value));
    promoted.unwrap();
    let renamed = position(&ops, &FsOp::Rename(store.record_path(VAULT_ID)));
    let synced = position(&ops, &FsOp::SyncDir(store.root().join(IDS_DIR)));
    let removed = position(&ops, &FsOp::Remove(store.journal_path(&key)));
    assert!(renamed < synced && synced < removed, "{ops:?}");
    assert_eq!(
        ops.last(),
        Some(&FsOp::SyncDir(store.root().join(JOURNALS_DIR)))
    );

    // A retried removal of an already removed journal still syncs.
    let (removed, ops) = record(|| store.remove_journal(&key));
    removed.unwrap();
    assert_eq!(ops, vec![FsOp::SyncDir(store.root().join(JOURNALS_DIR))]);
}

#[test]
fn a_journal_claiming_another_valid_vault_keeps_its_marker_and_data() {
    let (_temp, store) = witness();
    let key = target_key(Path::new("/example/home"));
    pending_for(&store, &journal(&key));
    let changed = claim_other_vault(&store, &key);
    let lock = store.lock_target(&key).unwrap();

    let error = match store.classify_target_journal(&lock) {
        Err(error) => error,
        Ok(_) => panic!("a marker names this target; the journal is not an orphan"),
    };
    assert_eq!(
        crate::error::classified_kind(&error),
        Some(crate::VaultErrorKind::AuditTampered)
    );
    assert!(error.to_string().contains("does not match its marker"));
    assert_eq!(fs::read(store.journal_path(&key)).unwrap(), changed);
    assert!(
        store
            .read_record(VAULT_ID)
            .unwrap()
            .unwrap()
            .pending
            .is_some()
    );
}

#[test]
fn an_orphan_proof_unlinks_only_its_targets_journal_under_its_own_lock() {
    let (_temp, store) = witness();
    let key = target_key(Path::new("/example/home"));
    store.write_journal(&journal(&key)).unwrap();
    claim_other_vault(&store, &key);
    // Other vaults' records, and an interrupted write's temporary file, do
    // not reference the target.
    store.write_record(&committed_record(1)).unwrap();
    fs::write(store.root().join(IDS_DIR).join(".abc.json.1.tmp"), b"{").unwrap();

    // A lock of another witness never classifies this one's journals.
    let (_other_temp, other) = witness();
    let foreign = other.lock_target(&key).unwrap();
    assert!(store.classify_target_journal(&foreign).is_err());

    let lock = store.lock_target(&key).unwrap();
    let TargetJournal::Orphan(orphan) = store.classify_target_journal(&lock).unwrap() else {
        panic!("no marker names the target");
    };
    let (deleted, ops) = record(|| store.delete_orphan_journal(orphan));
    deleted.unwrap();
    assert!(!store.journal_exists(&key));
    assert_eq!(
        ops,
        vec![
            FsOp::Remove(store.journal_path(&key)),
            FsOp::SyncDir(store.root().join(JOURNALS_DIR)),
        ]
    );
}

#[test]
fn records_that_cannot_be_trusted_or_the_scan_bound_never_authorize_deletion() {
    let key = target_key(Path::new("/example/home"));
    let check = |prepare: &dyn Fn(&WitnessStore)| {
        let (_temp, store) = witness();
        store.write_journal(&journal(&key)).unwrap();
        prepare(&store);
        let lock = store.lock_target(&key).unwrap();
        assert!(store.classify_target_journal(&lock).is_err());
        assert!(store.journal_exists(&key));
    };
    // A malformed record.
    check(&|store| fs::write(store.record_path(VAULT_ID), b"{not json").unwrap());
    // A record filed under another vault's key.
    check(&|store| {
        store.write_record(&committed_record(1)).unwrap();
        fs::copy(
            store.record_path(VAULT_ID),
            store.record_path(OTHER_VAULT_ID),
        )
        .unwrap();
    });
    // Too many entries to scan.
    check(&|store| {
        for index in 0..=target::MAX_RECORD_SCAN_ENTRIES {
            fs::write(
                store.root().join(IDS_DIR).join(format!(".{index}.tmp")),
                b"",
            )
            .unwrap();
        }
    });
    // Several markers naming the target.
    check(&|store| {
        let journal = journal(&key);
        let marker = pending_for(store, &journal).pending;
        let mut other = WitnessRecord::new(OTHER_VAULT_ID);
        other.committed = Some(checkpoint(1));
        other.pending = marker;
        store.write_record(&other).unwrap();
    });
}

#[test]
fn a_marker_whose_journal_is_missing_fails_closed() {
    let (_temp, store) = witness();
    let key = target_key(Path::new("/example/home"));
    pending_for(&store, &journal(&key));
    fs::remove_file(store.journal_path(&key)).unwrap();

    let lock = store.lock_target(&key).unwrap();
    let error = match store.classify_target_journal(&lock) {
        Err(error) => error,
        Ok(_) => panic!("a marker names this target without its journal"),
    };
    assert!(error.to_string().contains("journal is missing"), "{error}");
    assert!(
        store
            .read_record(VAULT_ID)
            .unwrap()
            .unwrap()
            .pending
            .is_some()
    );
}

#[test]
fn concurrent_first_opens_of_a_shared_witness_both_succeed() {
    for _ in 0..50 {
        let temp = tempfile::tempdir().unwrap();
        let location = WitnessLocation::at(canonical(temp.path()).join("witness"));
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let opens: Vec<_> = (0..2)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        location.open_or_create().map(|_| ())
                    })
                })
                .collect();
            for open in opens {
                open.join().unwrap().unwrap();
            }
        });
    }
}

#[test]
fn a_target_is_pending_while_its_journal_or_a_marker_naming_it_exists() {
    let (_temp, store) = witness();
    let key = target_key(Path::new("/example/home"));
    assert!(!store.target_pending(&key).unwrap());
    pending_for(&store, &journal(&key));
    assert!(store.target_pending(&key).unwrap());
    // A marker still names the target after its journal goes missing.
    fs::remove_file(store.journal_path(&key)).unwrap();
    let (pending, ops) = record(|| store.target_pending(&key));
    assert!(pending.unwrap());
    assert!(ops.is_empty(), "discovery must not sync: {ops:?}");
}
