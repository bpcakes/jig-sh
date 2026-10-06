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
fn an_orphan_journal_before_the_marker_is_discarded_on_retry() {
    let temp = private_temp();
    let (home, _vault) = source(temp.path());
    let archive = temp.path().join("vault.backup");
    backup(&home, &archive, &test_passphrase());
    let target = temp.path().join("restored");

    crate::store::arm_fault_for_test(FaultPoint::AfterJournal);
    assert!(restore(&archive, &target).is_err());
    assert!(!target.exists());
    assert_eq!(staging_dirs(temp.path()).len(), 1);

    restore(&archive, &target).unwrap();
    assert!(target.exists());
    assert!(staging_dirs(temp.path()).is_empty());
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
    assert!(!store.has_pending_journal().unwrap());

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
