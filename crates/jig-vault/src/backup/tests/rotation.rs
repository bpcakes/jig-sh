//! Backups and a format 3 passphrase change that rotates the data-encryption
//! key: an archive stays tied to the credential it was created with, and an
//! in-flight backup still finishes after a rotation.

use super::*;
use crate::store::VaultStore;

fn credential(text: &str) -> SecretString {
    SecretString::from(text.to_owned())
}

fn private_temp() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    temp
}

fn source(temp: &Path) -> (PathBuf, Vault) {
    let home = temp.join("source");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    vault.init(&test_passphrase()).unwrap();
    vault
        .set_field(
            &test_passphrase(),
            reference(),
            FieldKind::Concealed,
            SecretBytes::new(b"rotated-backup-value".to_vec()),
        )
        .unwrap();
    (home, vault)
}

fn backup(home: &Path, output: &Path, credential: &SecretString) {
    let request = Vault::preflight_backup_create(home.to_path_buf(), output, false).unwrap();
    Vault::create_backup(credential, request).unwrap();
}

fn restore(archive: &Path, target: &Path, credential: &SecretString) -> crate::Result<()> {
    let request = Vault::preflight_backup_restore(archive, target.to_path_buf())?;
    Vault::restore_backup(credential, request).map(|_| ())
}

fn assert_refused(archive: &Path, target: &Path, credential: &SecretString) {
    let error = restore(archive, target, credential).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Authentication, "{error}");
    assert!(!target.exists());
}

#[test]
fn each_backup_keeps_the_credential_current_when_it_was_created() {
    let temp = private_temp();
    let (home, vault) = source(temp.path());
    let first = credential("first rotated passphrase for backups");
    let second = credential("second rotated passphrase for backups");
    vault.change_passphrase(&test_passphrase(), &first).unwrap();
    let after_first = temp.path().join("after-first.backup");
    backup(&home, &after_first, &first);
    vault.change_passphrase(&first, &second).unwrap();
    let after_second = temp.path().join("after-second.backup");
    backup(&home, &after_second, &second);

    // A backup made after one rotation but before the next still opens with
    // the credential it was created with, and with no other.
    assert_refused(
        &after_first,
        &temp.path().join("refused-original"),
        &test_passphrase(),
    );
    assert_refused(&after_first, &temp.path().join("refused-later"), &second);
    restore(&after_first, &temp.path().join("from-first"), &first).unwrap();
    // A backup made after the latest rotation needs the new credential.
    assert_refused(&after_second, &temp.path().join("refused-older"), &first);
    restore(&after_second, &temp.path().join("from-second"), &second).unwrap();
    let restored = Vault::resolve_for_test(Some(temp.path().join("from-second"))).unwrap();
    assert_eq!(restored.list_fields(&second).unwrap().len(), 1);
}

#[test]
fn a_backup_snapshotted_before_a_rotation_finishes_after_it() {
    let temp = private_temp();
    let (home, vault) = source(temp.path());
    let new = credential("replacement passphrase during a backup");
    let store = VaultStore::open_existing(home.clone()).unwrap();
    let snapshot = store
        .prepare_backup_snapshot(&test_passphrase(), ulid::Ulid::new().to_string())
        .unwrap();

    vault.change_passphrase(&test_passphrase(), &new).unwrap();

    // The rest of `create`: seal the captured pre-rotation state under the
    // credential the backup started with, then record the finish.
    let BackupSnapshot {
        store,
        audit_key,
        operation_id,
        source_vault_id,
        source_format_version,
        vault_bytes,
        audit_bytes,
    } = snapshot;
    let lifecycle = BackupLifecycle {
        store,
        audit_key,
        operation_id,
    };
    let sealed = seal_archive(
        &test_passphrase(),
        &source_vault_id,
        source_format_version,
        &vault_bytes,
        &audit_bytes,
        now_ms(),
    )
    .unwrap();
    let archive = temp.path().join("in-flight.backup");
    fs::write(&archive, &sealed.bytes).unwrap();
    lifecycle
        .record_finish(sealed.bytes.len(), sealed.created_at_ms)
        .unwrap();

    let audit = fs::read_to_string(home.join("audit.jsonl")).unwrap();
    let last: serde_json::Value = serde_json::from_str(audit.lines().last().unwrap()).unwrap();
    assert_eq!(last["action"], "backup_finish");
    vault.verify_audit(&new).unwrap();
    // The archive holds the pre-rotation envelope under its original
    // credential: older ciphertext is never revoked by rotation.
    assert_refused(&archive, &temp.path().join("refused"), &new);
    restore(&archive, &temp.path().join("restored"), &test_passphrase()).unwrap();
}
