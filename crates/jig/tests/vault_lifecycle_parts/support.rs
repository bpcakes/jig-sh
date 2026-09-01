use super::*;

pub(super) fn assert_incomplete_change_is_atomic(home: &Path, source: &Vault) {
    let vault_before = std::fs::read(source.root().join("vault.json")).unwrap();
    let audit_before = std::fs::read(source.root().join("audit.jsonl")).unwrap();
    let output = jig_with_passphrases(
        [
            "--json".as_ref(),
            "vault".as_ref(),
            "passphrase".as_ref(),
            "change".as_ref(),
            "--home".as_ref(),
            home.as_os_str(),
        ],
        OLD_PASSPHRASE,
        None,
    );
    assert!(!output.status.success());
    let combined = combined_output(&output);
    let text = String::from_utf8_lossy(&combined);
    assert!(text.contains("JIG_VAULT_PASSPHRASE"));
    assert!(text.contains("JIG_VAULT_NEW_PASSPHRASE"));
    assert_contains_no_lifecycle_secrets(&combined);
    assert_eq!(
        std::fs::read(source.root().join("vault.json")).unwrap(),
        vault_before
    );
    assert_eq!(
        std::fs::read(source.root().join("audit.jsonl")).unwrap(),
        audit_before
    );
}

pub(super) fn change_to_backup_passphrase(
    home: &Path,
    source: &Vault,
    old: &SecretString,
    backup_passphrase: &SecretString,
    fields_before: &[jig_vault::FieldRecord],
) {
    let output = jig_with_passphrases(
        [
            "--json".as_ref(),
            "vault".as_ref(),
            "passphrase".as_ref(),
            "change".as_ref(),
            "--home".as_ref(),
            home.as_os_str(),
        ],
        OLD_PASSPHRASE,
        Some(BACKUP_PASSPHRASE),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output_json(&output)["changed"], true);
    assert_contains_no_lifecycle_secrets(&combined_output(&output));
    assert!(source.list_fields(old).is_err());
    assert_eq!(
        source.list_fields(backup_passphrase).unwrap(),
        fields_before
    );
}

pub(super) fn create_first_backup(temp: &Path, home: &Path) -> (std::path::PathBuf, Vec<u8>) {
    let backup = temp.join("vault-one.backup");
    let output = jig_with_passphrases(
        [
            "--json".as_ref(),
            "vault".as_ref(),
            "backup".as_ref(),
            "create".as_ref(),
            "--out".as_ref(),
            backup.as_os_str(),
            "--home".as_ref(),
            home.as_os_str(),
        ],
        BACKUP_PASSPHRASE,
        None,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = output_json(&output);
    assert_eq!(payload["command"], "vault backup create");
    assert_eq!(payload["backup_version"], 1);
    assert!(payload["bytes_written"].as_u64().unwrap() > 0);
    assert_contains_no_lifecycle_secrets(&combined_output(&output));
    let bytes = std::fs::read(&backup).unwrap();
    assert_contains_no_lifecycle_secrets(&bytes);
    assert_backup_hides_audit_metadata(&bytes);
    assert_eq!(
        std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
        0o600
    );
    (backup, bytes)
}

fn assert_backup_hides_audit_metadata(bytes: &[u8]) {
    for audit_metadata in [
        b"vault_initialized".as_slice(),
        b"field_batch_apply".as_slice(),
        b"passphrase_change".as_slice(),
    ] {
        assert!(
            !bytes
                .windows(audit_metadata.len())
                .any(|part| part == audit_metadata),
            "backup exposed plaintext audit metadata"
        );
    }
}

pub(super) fn assert_backup_overwrite_contract(
    home: &Path,
    source: &Vault,
    backup: &Path,
    original_bytes: &[u8],
) {
    let audit_before = std::fs::read(source.root().join("audit.jsonl")).unwrap();
    let refused = jig_with_passphrases(
        [
            "--json".as_ref(),
            "vault".as_ref(),
            "backup".as_ref(),
            "create".as_ref(),
            "--out".as_ref(),
            backup.as_os_str(),
            "--home".as_ref(),
            home.as_os_str(),
        ],
        BACKUP_PASSPHRASE,
        None,
    );
    assert!(!refused.status.success());
    assert_eq!(std::fs::read(backup).unwrap(), original_bytes);
    assert_eq!(
        std::fs::read(source.root().join("audit.jsonl")).unwrap(),
        audit_before
    );
    assert_contains_no_lifecycle_secrets(&combined_output(&refused));

    let overwritten = jig_with_passphrases(
        [
            "--json".as_ref(),
            "vault".as_ref(),
            "backup".as_ref(),
            "create".as_ref(),
            "--out".as_ref(),
            backup.as_os_str(),
            "--overwrite".as_ref(),
            "--home".as_ref(),
            home.as_os_str(),
        ],
        BACKUP_PASSPHRASE,
        None,
    );
    assert!(
        overwritten.status.success(),
        "{}",
        String::from_utf8_lossy(&overwritten.stderr)
    );
    let overwritten_bytes = std::fs::read(backup).unwrap();
    assert_ne!(original_bytes, overwritten_bytes);
    assert_contains_no_lifecycle_secrets(&overwritten_bytes);
}

pub(super) fn change_to_latest_and_verify(home: &Path, source: &Vault, latest: &SecretString) {
    let changed = jig_with_passphrases(
        [
            "--json".as_ref(),
            "vault".as_ref(),
            "passphrase".as_ref(),
            "change".as_ref(),
            "--home".as_ref(),
            home.as_os_str(),
        ],
        BACKUP_PASSPHRASE,
        Some(LATEST_PASSPHRASE),
    );
    assert!(changed.status.success());
    source.verify_audit(latest).unwrap();
    let audit = std::fs::read(source.root().join("audit.jsonl")).unwrap();
    assert!(
        audit
            .windows(b"passphrase_change".len())
            .any(|part| part == b"passphrase_change")
    );
    assert_contains_no_lifecycle_secrets(&audit);
}

pub(super) fn assert_restore_contract(
    temp: &Path,
    backup: &Path,
    backup_passphrase: &SecretString,
    fields_before: &[jig_vault::FieldRecord],
) {
    assert_restore_path_guards(temp, backup);
    let restored_home = temp.join("restore-base/scopes/restored-vault-home");
    assert_wrong_passphrase_is_atomic(temp, backup, &restored_home);
    assert_restored_vault(backup, &restored_home, backup_passphrase, fields_before);
    assert_legacy_restore(temp, backup, backup_passphrase);
}

fn assert_restore_path_guards(temp: &Path, backup: &Path) {
    let symlinked_backup = temp.join("vault-link.backup");
    symlink(backup, &symlinked_backup).unwrap();
    let symlink_target = temp.join("symlink-restore-home");
    let refused_symlink = restore_command(&symlinked_backup, &symlink_target, BACKUP_PASSPHRASE);
    assert!(!refused_symlink.status.success());
    assert!(!symlink_target.exists());

    let existing_target = temp.join("existing-restore-home");
    std::fs::create_dir(&existing_target).unwrap();
    let refused_existing = restore_command(backup, &existing_target, BACKUP_PASSPHRASE);
    assert!(!refused_existing.status.success());
    assert!(existing_target.read_dir().unwrap().next().is_none());
}

fn restore_command(backup: &Path, home: &Path, passphrase: &str) -> Output {
    jig_with_passphrases(
        [
            "--json".as_ref(),
            "vault".as_ref(),
            "backup".as_ref(),
            "restore".as_ref(),
            "--in".as_ref(),
            backup.as_os_str(),
            "--home".as_ref(),
            home.as_os_str(),
        ],
        passphrase,
        None,
    )
}

fn assert_wrong_passphrase_is_atomic(temp: &Path, backup: &Path, restored_home: &Path) {
    let output = restore_command(backup, restored_home, LATEST_PASSPHRASE);
    assert!(!output.status.success());
    assert!(!restored_home.exists());
    let combined = combined_output(&output);
    assert!(String::from_utf8_lossy(&combined).contains("authenticate backup archive"));
    assert_contains_no_lifecycle_secrets(&combined);
    for created in [temp.join("restore-base"), temp.join("restore-base/scopes")] {
        assert!(created.is_dir());
        assert_eq!(
            std::fs::metadata(created).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}

fn assert_restored_vault(
    backup: &Path,
    restored_home: &Path,
    backup_passphrase: &SecretString,
    fields_before: &[jig_vault::FieldRecord],
) {
    let restored = restore_command(backup, restored_home, BACKUP_PASSPHRASE);
    let combined = combined_output(&restored);
    assert!(
        restored.status.success(),
        "{}",
        String::from_utf8_lossy(&combined)
    );
    let payload = output_json(&restored);
    assert_eq!(payload["command"], "vault backup restore");
    assert_eq!(payload["restored"], true);
    assert_eq!(payload["format_version"], 2);
    assert_contains_no_lifecycle_secrets(&combined);

    let vault = Vault::resolve_for_test(Some(restored_home.to_path_buf())).unwrap();
    assert_eq!(vault.list_fields(backup_passphrase).unwrap(), fields_before);
    let mut restored_value = Vec::new();
    vault
        .read_field_to(
            backup_passphrase,
            reference("jig://Production/TOKEN"),
            &mut restored_value,
        )
        .unwrap();
    assert_eq!(restored_value, FIELD_VALUE);
    vault.verify_audit(backup_passphrase).unwrap();
    let audit = std::fs::read(vault.root().join("audit.jsonl")).unwrap();
    assert!(
        audit
            .windows(b"backup_restore".len())
            .any(|part| part == b"backup_restore")
    );
    assert_contains_no_lifecycle_secrets(&audit);
}

fn assert_legacy_restore(temp: &Path, backup: &Path, backup_passphrase: &SecretString) {
    let legacy_home = temp.join("legacy-restored-vault");
    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    command
        .current_dir(temp)
        .args(["--json", "vault", "backup", "restore", "--in"])
        .arg(backup)
        .env("JIG_VAULT_HOME", &legacy_home)
        .env("JIG_VAULT_PASSPHRASE", BACKUP_PASSPHRASE)
        .env_remove("JIG_VAULT_NEW_PASSPHRASE")
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD");
    let restored = command.output().unwrap();
    let combined = combined_output(&restored);
    assert!(
        restored.status.success(),
        "{}",
        String::from_utf8_lossy(&combined)
    );
    let payload = output_json(&restored);
    assert_eq!(payload["vault_scope"], "legacy");
    assert_eq!(payload["vault_home"], legacy_home.display().to_string());
    assert_contains_no_lifecycle_secrets(&combined);
    Vault::resolve_for_test(Some(legacy_home))
        .unwrap()
        .verify_audit(backup_passphrase)
        .unwrap();
}
