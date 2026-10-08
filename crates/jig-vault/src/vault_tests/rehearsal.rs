//! The format 3 release rehearsal, from the frozen version 2 fixture:
//! migrate, edit, rotate the data-encryption key, back up, restore into an
//! absent home, then unlock and verify the audit chain. It replays the
//! old-key attack and the witnessed refusals of older copies. Every home and
//! the witness beside them are disposable, and every credential is
//! test-only.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::rotation::{dek_opens_state, extract_dek};
use super::*;
use crate::BackupRestoreResult;
use crate::test_fixtures::{
    GENERATED_V2_AUDIT_JSONL, GENERATED_V2_BACKUP, GENERATED_V2_CONCEALED_REFERENCE,
    GENERATED_V2_TEXT_REFERENCE, GENERATED_V2_TEXT_VALUE, GENERATED_V2_VAULT_ID,
    GENERATED_V2_VAULT_JSON, generated_v2_passphrase, install_generated_v2_fixture,
};

const EDITED_VALUE: &[u8] = b"rehearsal-edited-token";

fn rotated() -> SecretString {
    SecretString::from("rehearsal replacement passphrase after migration".to_owned())
}

/// A private directory whose homes all share one disposable witness.
fn private_temp() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    temp
}

/// The frozen version 2 vault, migrated to format 3.
fn migrated_fixture(home: &Path) -> Vault {
    let vault = Vault::resolve_for_test(Some(home.to_path_buf())).unwrap();
    install_generated_v2_fixture(&vault.store);
    let migration = vault
        .migrate(&generated_v2_passphrase(), V3_FORMAT_VERSION)
        .unwrap();
    assert_eq!(
        (
            migration.from_version,
            migration.to_version,
            migration.changed
        ),
        (V2_FORMAT_VERSION, V3_FORMAT_VERSION, true)
    );
    vault
}

fn envelope_at(home: &Path) -> VaultFile {
    serde_json::from_str(&fs::read_to_string(home.join("vault.json")).unwrap()).unwrap()
}

fn value(home: &Path, passphrase: &SecretString, reference: &str) -> Vec<u8> {
    let store = VaultStore::resolve_for_test(Some(home.to_path_buf())).unwrap();
    let opened = store.open_unlocked(passphrase).unwrap();
    let name = VaultReference::parse(reference).unwrap().to_secret_name();
    opened.secret_value(&name).unwrap().as_slice().to_vec()
}

/// Installs `vault_json` and `audit_jsonl` as a separate private home.
fn copied_home(base: &Path, name: &str, vault_json: &str, audit_jsonl: &str) -> PathBuf {
    let home = base.join(name);
    fs::create_dir(&home).unwrap();
    fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
    for (file, contents) in [("vault.json", vault_json), ("audit.jsonl", audit_jsonl)] {
        fs::write(home.join(file), contents).unwrap();
        fs::set_permissions(home.join(file), fs::Permissions::from_mode(0o600)).unwrap();
    }
    home
}

fn assert_refused(home: &Path, passphrase: &SecretString, reason: &str) {
    let vault = Vault::resolve_for_test(Some(home.to_path_buf())).unwrap();
    let error = vault.list_fields(passphrase).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered, "{error}");
    assert!(error.to_string().contains(reason), "{error}");
}

fn restore(archive: &Path, target: PathBuf, passphrase: &SecretString) -> BackupRestoreResult {
    let request = Vault::preflight_backup_restore(archive, target).unwrap();
    Vault::restore_backup(passphrase, request).unwrap()
}

#[test]
fn a_v2_vault_becomes_a_rotated_restored_v3_vault_and_refuses_its_older_copies() {
    let temp = private_temp();
    let base = temp.path();
    let home = base.join("vault");
    let vault = migrated_fixture(&home);
    let original = generated_v2_passphrase();
    vault
        .set_field(
            &original,
            VaultReference::parse(GENERATED_V2_CONCEALED_REFERENCE).unwrap(),
            FieldKind::Concealed,
            SecretBytes::new(EDITED_VALUE.to_vec()),
        )
        .unwrap();
    let before_rotation = envelope_at(&home);
    let old_dek = extract_dek(&before_rotation, &original);
    let earlier_copy = (
        vault.store.read_vault_text().unwrap().unwrap(),
        vault.store.read_audit_text().unwrap().unwrap(),
    );

    vault.change_passphrase(&original, &rotated()).unwrap();
    let after_rotation = envelope_at(&home);
    assert!(!dek_opens_state(&after_rotation, &old_dek));
    let rotated_dek = extract_dek(&after_rotation, &rotated());
    assert!(dek_opens_state(&after_rotation, &rotated_dek));

    let archive = base.join("vault.backup");
    let request = Vault::preflight_backup_create(home.clone(), &archive, false).unwrap();
    Vault::create_backup(&rotated(), request).unwrap();
    let restored = restore(&archive, base.join("restored"), &rotated());
    assert_eq!(restored.vault_id, GENERATED_V2_VAULT_ID);
    assert_eq!(
        (restored.format_version, restored.source_format_version),
        (V3_FORMAT_VERSION, V3_FORMAT_VERSION)
    );
    assert_eq!(
        restored.generation,
        after_rotation
            .header
            .generation
            .map(|generation| generation + 1)
    );

    // The restored home unlocks with the rotated credential, keeps the edit
    // and the untouched fields, and verifies its audit chain.
    let edited = value(&restored.root, &rotated(), GENERATED_V2_CONCEALED_REFERENCE);
    assert_eq!(edited, EDITED_VALUE);
    let text = value(&restored.root, &rotated(), GENERATED_V2_TEXT_REFERENCE);
    assert_eq!(text, GENERATED_V2_TEXT_VALUE);
    let restored_vault = Vault::resolve_for_test(Some(restored.root.clone())).unwrap();
    restored_vault.verify_audit(&rotated()).unwrap();
    // The restore resealed under a fresh key that no earlier key opens.
    let restored_file = envelope_at(&restored.root);
    assert!(!dek_opens_state(&restored_file, &old_dek));
    assert!(!dek_opens_state(&restored_file, &rotated_dek));

    // Every older copy of the vault ID is refused by authenticated use: the
    // live home the restore fenced, a copy from before the rotation, and the
    // version 2 original.
    assert_refused(&home, &rotated(), "refusing a rolled-back copy");
    let (vault_json, audit_jsonl) = &earlier_copy;
    let earlier = copied_home(base, "earlier", vault_json, audit_jsonl);
    assert_refused(&earlier, &original, "refusing a rolled-back copy");
    let replayed = copied_home(
        base,
        "replayed",
        GENERATED_V2_VAULT_JSON,
        GENERATED_V2_AUDIT_JSONL,
    );
    assert_refused(&replayed, &original, "refusing an older-format copy");
}

#[test]
fn a_pre_migration_v2_backup_recovers_as_v3_with_its_archived_credential() {
    let temp = private_temp();
    let base = temp.path();
    let home = base.join("vault");
    let vault = migrated_fixture(&home);
    vault
        .change_passphrase(&generated_v2_passphrase(), &rotated())
        .unwrap();
    let witnessed = envelope_at(&home).header.generation.unwrap();

    let archive = base.join("pre-migration.backup");
    fs::write(&archive, GENERATED_V2_BACKUP).unwrap();
    let restored = restore(&archive, base.join("restored"), &generated_v2_passphrase());
    assert_eq!(
        (restored.format_version, restored.source_format_version),
        (V3_FORMAT_VERSION, V2_FORMAT_VERSION)
    );
    assert_eq!(restored.generation, Some(witnessed + 1));

    // Restore recovers the archive's credential; the later rotation is not
    // carried over.
    let restored_vault = Vault::resolve_for_test(Some(restored.root)).unwrap();
    restored_vault
        .verify_audit(&generated_v2_passphrase())
        .unwrap();
    assert_eq!(
        restored_vault
            .list_fields(&generated_v2_passphrase())
            .unwrap()
            .len(),
        2
    );
    let error = restored_vault.list_fields(&rotated()).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Authentication);
    assert_refused(&home, &rotated(), "refusing a rolled-back copy");
}
