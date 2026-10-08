//! Witnessed restore transactions and their recovery.

use super::*;
use crate::store::FaultPoint;
use crate::store::VaultStore;
use crate::store::witness::WitnessLocation;

fn private_temp() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    temp
}

fn field(reference: &str) -> VaultReference {
    VaultReference::parse(reference).unwrap()
}

fn set_text(vault: &Vault, reference: &str, value: &[u8]) {
    vault
        .set_field(
            &test_passphrase(),
            field(reference),
            FieldKind::Text,
            SecretBytes::new(value.to_vec()),
        )
        .unwrap();
}

fn source(temp: &Path) -> (PathBuf, Vault) {
    let home = temp.join("source");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    vault.init(&test_passphrase()).unwrap();
    set_text(&vault, "jig://Example/ARCHIVED", b"archived value");
    (home, vault)
}

fn backup(home: &Path, output: &Path, passphrase: &SecretString) {
    let request = Vault::preflight_backup_create(home.to_path_buf(), output, false).unwrap();
    Vault::create_backup(passphrase, request).unwrap();
}

fn restore(input: &Path, target: &Path) -> crate::Result<BackupRestoreResult> {
    let request = Vault::preflight_backup_restore(input, target.to_path_buf())?;
    Vault::restore_backup(&test_passphrase(), request)
}

fn committed_generation(home: &Path, vault_id: &str) -> u64 {
    let location = WitnessLocation::for_home(home).unwrap();
    let record = location
        .open_existing()
        .unwrap()
        .unwrap()
        .read_record(vault_id)
        .unwrap()
        .unwrap();
    assert!(record.pending.is_none());
    record.committed.unwrap().generation
}

fn staging_dirs(temp: &Path) -> Vec<String> {
    fs::read_dir(temp)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("jig-vault-restore"))
        .collect()
}

#[test]
fn an_older_archive_restores_above_a_witness_that_is_ahead() {
    let temp = private_temp();
    let (home, vault) = source(temp.path());
    let archive = temp.path().join("older.backup");
    backup(&home, &archive, &test_passphrase());
    set_text(&vault, "jig://Example/LATER", b"later value");
    set_text(&vault, "jig://Example/LATEST", b"latest value");
    let id = vault.snapshot(&test_passphrase()).unwrap().vault_id;
    let witnessed = committed_generation(&home, &id);

    let target = temp.path().join("restored");
    let restored = restore(&archive, &target).unwrap();
    assert_eq!(restored.generation, Some(witnessed + 1));
    assert_eq!(committed_generation(&target, &id), witnessed + 1);
    let restored_vault = Vault::resolve_for_test(Some(restored.root)).unwrap();
    // Older content is recovered deliberately; the live source is fenced.
    assert_eq!(
        restored_vault
            .list_fields(&test_passphrase())
            .unwrap()
            .len(),
        1
    );
    let error = vault.list_fields(&test_passphrase()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("older than its witnessed generation")
    );
}

#[test]
fn a_witnessed_v2_archive_is_restored_as_version_three() {
    let temp = private_temp();
    let home = temp.path().join("source");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    vault
        .init_format_for_test(&test_passphrase(), V2_FORMAT_VERSION)
        .unwrap();
    set_text(&vault, "jig://Example/LEGACY", b"legacy value");
    let archive = temp.path().join("v2.backup");
    backup(&home, &archive, &test_passphrase());
    vault
        .migrate(&test_passphrase(), V3_FORMAT_VERSION)
        .unwrap();
    let id = vault.snapshot(&test_passphrase()).unwrap().vault_id;

    let target = temp.path().join("restored");
    let restored = restore(&archive, &target).unwrap();
    assert_eq!(restored.format_version, V3_FORMAT_VERSION);
    assert_eq!(restored.source_format_version, V2_FORMAT_VERSION);
    assert_eq!(restored.generation, Some(2));
    let restored_vault = Vault::resolve_for_test(Some(restored.root.clone())).unwrap();
    assert_eq!(
        restored_vault
            .list_fields(&test_passphrase())
            .unwrap()
            .len(),
        1
    );
    restored_vault.verify_audit(&test_passphrase()).unwrap();
    let events = fs::read_to_string(restored.root.join("audit.jsonl")).unwrap();
    let last: crate::audit::AuditEvent =
        serde_json::from_str(events.lines().last().unwrap()).unwrap();
    assert_eq!(last.action, "backup_restore");
    assert_eq!(last.details["source_format_version"], V2_FORMAT_VERSION);
    assert_eq!(last.details["target_format_version"], V3_FORMAT_VERSION);
    assert!(last.details["source_generation"].is_null());
    assert_eq!(committed_generation(&target, &id), 2);
}

#[test]
fn an_orphan_journal_is_unlinked_and_its_abandoned_staging_kept() {
    let temp = private_temp();
    let (home, _vault) = source(temp.path());
    let archive = temp.path().join("vault.backup");
    backup(&home, &archive, &test_passphrase());
    let target = temp.path().join("restored");

    // A failure after ownership was handed off never deletes the staging.
    crate::store::arm_fault_for_test(FaultPoint::AfterJournal);
    assert!(restore(&archive, &target).is_err());
    assert!(!target.exists());
    let [abandoned] = staging_dirs(temp.path()).try_into().unwrap();
    let abandoned = temp.path().join(abandoned);
    let bytes = staged_bytes(&abandoned);

    restore(&archive, &target).unwrap();
    assert!(target.exists());
    assert!(journal_paths(&target).is_empty());
    assert_eq!(staged_bytes(&abandoned), bytes);
    assert_eq!(staging_dirs(temp.path()).len(), 1);
}

/// The exact files of one directory, by name.
fn staged_bytes(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

#[test]
fn a_pending_restore_resumes_only_for_the_exact_archive() {
    let temp = private_temp();
    let (home, _vault) = source(temp.path());
    let archive = temp.path().join("first.backup");
    backup(&home, &archive, &test_passphrase());
    let other_archive = temp.path().join("second.backup");
    backup(&home, &other_archive, &test_passphrase());
    let target = temp.path().join("restored");

    crate::store::arm_fault_for_test(FaultPoint::AfterPending);
    let error = restore(&archive, &target).unwrap_err();
    assert!(error.to_string().contains("did not finish"), "{error}");
    assert!(!target.exists());

    let error = restore(&other_archive, &target).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AlreadyExists);
    assert!(error.to_string().contains("different archive"), "{error}");
    assert!(!target.exists());

    let wrong = SecretString::from("a different strong passphrase".to_owned());
    let request = Vault::preflight_backup_restore(&archive, target.clone()).unwrap();
    assert_eq!(
        Vault::restore_backup(&wrong, request).unwrap_err().kind(),
        VaultErrorKind::Authentication
    );

    let restored = restore(&archive, &target).unwrap();
    // The source sits at generation 2 (init plus one edit).
    assert_eq!(restored.generation, Some(3));
    assert!(staging_dirs(temp.path()).is_empty());
    Vault::resolve_for_test(Some(target))
        .unwrap()
        .verify_audit(&test_passphrase())
        .unwrap();
}

#[test]
fn any_authenticated_open_finishes_a_pending_restore_without_the_archive() {
    let temp = private_temp();
    let (home, _vault) = source(temp.path());
    let archive = temp.path().join("vault.backup");
    backup(&home, &archive, &test_passphrase());
    let target = temp.path().join("restored");
    crate::store::arm_fault_for_test(FaultPoint::AfterPending);
    restore(&archive, &target).unwrap_err();
    fs::remove_file(&archive).unwrap();

    // Resolving keeps the absent target absent for its no-replace install.
    let status = Vault::status(Some(target.clone())).unwrap();
    assert!(status.pending_transaction);
    let vault = Vault::resolve_for_test(Some(target.clone())).unwrap();
    assert!(!target.exists());
    assert_eq!(vault.list_fields(&test_passphrase()).unwrap().len(), 1);
    assert!(target.exists());
    assert!(!Vault::status(Some(target)).unwrap().pending_transaction);
    assert!(staging_dirs(temp.path()).is_empty());
}

#[test]
fn a_restore_installed_before_promotion_is_recognized_on_retry() {
    let temp = private_temp();
    let (home, _vault) = source(temp.path());
    let archive = temp.path().join("vault.backup");
    backup(&home, &archive, &test_passphrase());
    let target = temp.path().join("restored");
    crate::store::arm_fault_for_test(FaultPoint::AfterEnvelope);
    restore(&archive, &target).unwrap_err();
    assert!(target.exists());

    // Preflight lets the present target reach the retry, which then proves
    // the installed home is its own successor before promoting it.
    let restored = restore(&archive, &target).unwrap();
    assert_eq!(restored.generation, Some(3));
    let id = restored.vault_id;
    assert_eq!(committed_generation(&target, &id), 3);
}

#[test]
fn a_collision_never_overwrites_the_occupant_or_drops_staging() {
    let temp = private_temp();
    let (home, _vault) = source(temp.path());
    let archive = temp.path().join("vault.backup");
    backup(&home, &archive, &test_passphrase());
    let target = temp.path().join("restored");
    crate::store::arm_fault_for_test(FaultPoint::AfterPending);
    restore(&archive, &target).unwrap_err();

    fs::create_dir(&target).unwrap();
    fs::write(target.join("occupant"), b"unrelated").unwrap();
    let error = restore(&archive, &target).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AlreadyExists, "{error:?}");
    assert!(error.to_string().contains("occupied"), "{error}");
    assert_eq!(fs::read(target.join("occupant")).unwrap(), b"unrelated");
    assert_eq!(staging_dirs(temp.path()).len(), 1);
    let store = VaultStore::open_existing(home).unwrap();
    assert!(!store.has_pending_transaction().unwrap());

    fs::remove_file(target.join("occupant")).unwrap();
    fs::remove_dir(&target).unwrap();
    restore(&archive, &target).unwrap();
    assert!(staging_dirs(temp.path()).is_empty());
}

#[test]
fn an_unrelated_pending_transaction_blocks_a_restore() {
    let temp = private_temp();
    let (home, vault) = source(temp.path());
    let archive = temp.path().join("vault.backup");
    backup(&home, &archive, &test_passphrase());
    crate::store::arm_fault_for_test(FaultPoint::AfterPending);
    vault
        .set_field(
            &test_passphrase(),
            field("jig://Example/PENDING"),
            FieldKind::Text,
            SecretBytes::new(b"pending value".to_vec()),
        )
        .unwrap_err();

    let target = temp.path().join("restored");
    let error = restore(&archive, &target).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AlreadyExists);
    assert!(!target.exists());
    assert!(staging_dirs(temp.path()).is_empty());
}

fn pending_restore(temp: &Path) -> (PathBuf, PathBuf) {
    let (home, _vault) = source(temp);
    let archive = temp.join("vault.backup");
    backup(&home, &archive, &test_passphrase());
    let target = temp.join("restored");
    crate::store::arm_fault_for_test(FaultPoint::AfterPending);
    restore(&archive, &target).unwrap_err();
    (archive, target)
}

fn journal_paths(target: &Path) -> Vec<PathBuf> {
    let root = WitnessLocation::for_home(target)
        .unwrap()
        .root()
        .to_path_buf();
    fs::read_dir(root.join("journals"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect()
}

#[test]
fn a_journal_that_no_longer_matches_its_marker_is_kept() {
    let temp = private_temp();
    let (archive, target) = pending_restore(temp.path());
    let [journal] = journal_paths(&target).try_into().unwrap();
    let mut bytes = fs::read(&journal).unwrap();
    bytes.push(b'\n');
    fs::write(&journal, &bytes).unwrap();

    let error = restore(&archive, &target).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
    assert!(error.to_string().contains("does not match its marker"));
    assert_eq!(fs::read(&journal).unwrap(), bytes);
    assert_eq!(staging_dirs(temp.path()).len(), 1);
    assert!(!target.exists());
}

#[test]
fn a_failed_recovery_read_never_cleans_away_the_pending_staging() {
    let temp = private_temp();
    let (archive, target) = pending_restore(temp.path());
    let [staging] = staging_dirs(temp.path()).try_into().unwrap();
    let staged_audit = temp.path().join(staging).join("audit.jsonl");
    fs::set_permissions(&staged_audit, fs::Permissions::from_mode(0o000)).unwrap();

    restore(&archive, &target).unwrap_err();
    assert_eq!(staging_dirs(temp.path()).len(), 1);
    assert!(staged_audit.exists());

    fs::set_permissions(&staged_audit, fs::Permissions::from_mode(0o600)).unwrap();
    restore(&archive, &target).unwrap();
    assert!(staging_dirs(temp.path()).is_empty());
}

#[test]
fn only_the_installed_staging_directory_counts_as_the_successor() {
    let temp = private_temp();
    let (archive, target) = pending_restore(temp.path());
    let [staging] = staging_dirs(temp.path()).try_into().unwrap();
    std::os::unix::fs::symlink(temp.path().join(staging), &target).unwrap();

    let error = restore(&archive, &target).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AlreadyExists, "{error}");
    assert_eq!(journal_paths(&target).len(), 1);
    assert_eq!(staging_dirs(temp.path()).len(), 1);

    fs::remove_file(&target).unwrap();
    restore(&archive, &target).unwrap();
    assert!(fs::symlink_metadata(&target).unwrap().is_dir());
}

#[test]
fn an_absent_pending_target_passes_existing_home_preflights() {
    let temp = private_temp();
    let (_archive, target) = pending_restore(temp.path());

    Vault::preflight_passphrase_change(target.clone()).unwrap();
    let output = temp.path().join("after-recovery.backup");
    let request = Vault::preflight_backup_create(target.clone(), &output, false).unwrap();
    assert!(!target.exists());
    Vault::create_backup(&test_passphrase(), request).unwrap();
    assert!(target.exists());
    assert!(output.exists());
    assert!(!Vault::status(Some(target)).unwrap().pending_transaction);
}

/// Rewrites a journal to claim another valid vault ID; it stays parseable.
fn claim_other_vault(path: &Path) -> (Vec<u8>, Vec<u8>) {
    let original = fs::read(path).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    value["vault_id"] = "01EXAMPLEOTHERVAULTID0000000".into();
    let changed = serde_json::to_vec(&value).unwrap();
    fs::write(path, &changed).unwrap();
    (original, changed)
}

#[test]
fn a_restore_journal_claiming_another_valid_vault_keeps_its_recovery_data() {
    let temp = private_temp();
    let (archive, target) = pending_restore(temp.path());
    let [journal] = journal_paths(&target).try_into().unwrap();
    let (original, changed) = claim_other_vault(&journal);

    // Neither a restore retry nor generic recovery mistakes it for an orphan.
    let error = restore(&archive, &target).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
    assert!(
        error.to_string().contains("does not match its marker"),
        "{error}"
    );
    let error = Vault::resolve_for_test(Some(target.clone()))
        .unwrap()
        .list_fields(&test_passphrase())
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
    assert_eq!(fs::read(&journal).unwrap(), changed);
    assert_eq!(staging_dirs(temp.path()).len(), 1);
    assert!(!target.exists());

    fs::write(&journal, original).unwrap();
    restore(&archive, &target).unwrap();
    assert!(staging_dirs(temp.path()).is_empty());
}

#[test]
fn an_unreferenced_restore_journal_claiming_another_vault_is_unlinked_alone() {
    let temp = private_temp();
    let (home, _vault) = source(temp.path());
    let archive = temp.path().join("vault.backup");
    backup(&home, &archive, &test_passphrase());
    let target = temp.path().join("restored");
    crate::store::arm_fault_for_test(FaultPoint::AfterJournal);
    restore(&archive, &target).unwrap_err();
    let [journal] = journal_paths(&target).try_into().unwrap();
    claim_other_vault(&journal);
    let [abandoned] = staging_dirs(temp.path()).try_into().unwrap();
    let abandoned = temp.path().join(abandoned);
    let bytes = staged_bytes(&abandoned);

    restore(&archive, &target).unwrap();
    assert!(journal_paths(&target).is_empty());
    assert_eq!(staged_bytes(&abandoned), bytes);
}

#[test]
fn a_target_parent_left_by_an_interrupted_attempt_is_durable_before_the_journal() {
    use crate::store::durable::recording::{FsOp, record};

    let temp = private_temp();
    let base = fs::canonicalize(temp.path()).unwrap();
    let outer = base.join("c");
    let parent = outer.join("d");
    let (home, _vault) = source(&base.join("sources"));
    let archive = base.join("vault.backup");
    backup(&home, &archive, &test_passphrase());
    // The witness of the restore target lives on another branch, so its
    // syncs cannot stand in for the target parent's.
    let _witness = crate::store::witness::override_root_for_test(base.join("a/b/witness"));
    // An earlier restore created the parents, then crashed before syncing
    // their entries.
    for dir in [&outer, &parent] {
        fs::create_dir(dir).unwrap();
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).unwrap();
    }

    let (restored, ops) = record(|| restore(&archive, &parent.join("restored")));
    restored.unwrap();
    let journal_written = ops
        .iter()
        .position(|op| matches!(op, FsOp::Rename(path) if path.parent().is_some_and(|dir| dir.ends_with("journals"))))
        .unwrap();
    for dir in [&outer, &base] {
        let synced = ops
            .iter()
            .position(|op| *op == FsOp::SyncDir(dir.clone()))
            .unwrap_or_else(|| panic!("{} never synced: {ops:?}", dir.display()));
        assert!(synced < journal_written, "{ops:?}");
    }
}

#[test]
fn a_pending_restore_whose_journal_is_missing_blocks_every_use_of_its_target() {
    let temp = private_temp();
    let (archive, target) = pending_restore(temp.path());
    let [journal] = journal_paths(&target).try_into().unwrap();
    fs::remove_file(&journal).unwrap();

    let error = restore(&archive, &target).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
    assert!(error.to_string().contains("journal is missing"), "{error}");
    let error = Vault::resolve_for_test(Some(target.clone()))
        .unwrap()
        .init(&test_passphrase())
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
    // No vault was initialized over the pending restore.
    assert!(!target.join("vault.json").exists());
    assert_eq!(staging_dirs(temp.path()).len(), 1);
}

#[test]
fn a_restore_target_inside_the_witness_is_refused_before_creating_anything() {
    let temp = private_temp();
    let base = fs::canonicalize(temp.path()).unwrap();
    let witness_root = base.join("witness");
    // Production shares one witness across every home; mirror that here.
    let _witness = crate::store::witness::override_root_for_test(witness_root.clone());
    let (home, vault) = source(&base.join("homes"));
    let archive = base.join("vault.backup");
    backup(&home, &archive, &test_passphrase());
    let ids = witness_root.join("ids");
    let entries = || fs::read_dir(&ids).unwrap().count();
    let before = entries();

    let target = ids
        .join(format!("{}.json", "a".repeat(64)))
        .join("ExampleVault");
    let error = Vault::preflight_backup_restore(&archive, target).unwrap_err();
    assert!(error.to_string().contains("must not overlap"), "{error}");
    assert_eq!(entries(), before);
    vault.list_fields(&test_passphrase()).unwrap();
}

#[test]
fn an_absent_target_whose_journal_is_missing_stays_absent_and_can_resume() {
    let temp = private_temp();
    let (archive, target) = pending_restore(temp.path());
    let [journal] = journal_paths(&target).try_into().unwrap();
    let original = fs::read(&journal).unwrap();
    fs::remove_file(&journal).unwrap();

    assert!(
        Vault::status(Some(target.clone()))
            .unwrap()
            .pending_transaction
    );
    let vault = Vault::resolve_for_test(Some(target.clone())).unwrap();
    assert!(!target.exists());
    let error = vault.list_fields(&test_passphrase()).unwrap_err();
    assert!(error.to_string().contains("journal is missing"), "{error}");
    assert!(!target.exists());

    // Nothing obstructs the transaction once its journal is back.
    fs::write(&journal, original).unwrap();
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o600)).unwrap();
    restore(&archive, &target).unwrap();
    assert!(staging_dirs(temp.path()).is_empty());
}

#[test]
fn a_restore_after_an_interrupted_witness_creation_finishes_the_tree() {
    let temp = private_temp();
    let base = fs::canonicalize(temp.path()).unwrap();
    let (home, _vault) = source(&base.join("sources"));
    let archive = base.join("vault.backup");
    backup(&home, &archive, &test_passphrase());
    // The target's witness root exists without any of its directories.
    let witness_root = base.join("fresh-witness");
    fs::create_dir(&witness_root).unwrap();
    fs::set_permissions(&witness_root, fs::Permissions::from_mode(0o700)).unwrap();
    let _witness = crate::store::witness::override_root_for_test(witness_root.clone());

    let target = base.join("restored");
    let request = Vault::preflight_backup_restore(&archive, target.clone()).unwrap();
    Vault::restore_backup(&test_passphrase(), request).unwrap();
    assert!(witness_root.join("ids").is_dir());
    Vault::resolve_for_test(Some(target))
        .unwrap()
        .list_fields(&test_passphrase())
        .unwrap();
}

#[test]
fn pending_staging_that_lost_its_privacy_is_never_installed() {
    let temp = private_temp();
    let (archive, target) = pending_restore(temp.path());
    let [staging] = staging_dirs(temp.path()).try_into().unwrap();
    let staging = temp.path().join(staging);
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o777)).unwrap();

    let error = restore(&archive, &target).unwrap_err();
    assert!(error.to_string().contains("no longer private"), "{error}");
    assert!(!target.exists());
    assert!(staging.exists());

    fs::set_permissions(&staging, fs::Permissions::from_mode(0o700)).unwrap();
    restore(&archive, &target).unwrap();
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

mod durability;
mod integrity;
mod ownership;
