#![cfg(target_os = "linux")]

use std::os::unix::fs::PermissionsExt;
#[cfg(target_os = "linux")]
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::{Command, Output};

use jig_vault::{FieldKind, FieldMutation, SecretBytes, Vault, VaultReference};
use secrecy::SecretString;

const OLD_PASSPHRASE: &str = "test-only-old-passphrase";
const BACKUP_PASSPHRASE: &str = "test-only-backup-passphrase";
const LATEST_PASSPHRASE: &str = "test-only-latest-passphrase";
const FIELD_VALUE: &[u8] = b"test-only-lifecycle-secret";

fn reference(value: &str) -> VaultReference {
    value.parse().unwrap()
}

fn private_tempdir() -> tempfile::TempDir {
    // macOS exposes its temporary directory through /var, which is a symlink
    // to /private/var. Vault output correctly rejects symlinked ancestors, so
    // build fixtures beneath the canonical temporary root.
    let temp_root = std::env::temp_dir().canonicalize().unwrap();
    let temp = tempfile::Builder::new()
        .prefix("jig-vault-lifecycle-")
        .tempdir_in(temp_root)
        .unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    temp
}

fn initialized_vault(root: &Path) -> (std::path::PathBuf, Vault) {
    let home = root.join("vault-home");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    let passphrase = SecretString::from(OLD_PASSPHRASE.to_owned());
    vault.init(&passphrase).unwrap();
    vault
        .apply_field_batch(
            &passphrase,
            vec![
                FieldMutation::set(
                    reference("jig://Production/TOKEN"),
                    FieldKind::Concealed,
                    SecretBytes::new(FIELD_VALUE.to_vec()),
                ),
                FieldMutation::set(
                    reference("jig://Production/MODE"),
                    FieldKind::Text,
                    SecretBytes::new(b"test".to_vec()),
                ),
            ],
        )
        .unwrap();
    (home, vault)
}

fn jig_with_passphrases(
    args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>,
    current: &str,
    new: Option<&str>,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    command
        .args(args)
        .env("JIG_VAULT_PASSPHRASE", current)
        .env_remove("JIG_VAULT_NEW_PASSPHRASE");
    if let Some(new) = new {
        command.env("JIG_VAULT_NEW_PASSPHRASE", new);
    }
    command.output().unwrap()
}

fn output_json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON ({error}); stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn combined_output(output: &Output) -> Vec<u8> {
    let mut combined = output.stdout.clone();
    combined.extend_from_slice(&output.stderr);
    combined
}

fn assert_contains_no_lifecycle_secrets(bytes: &[u8]) {
    for secret in [
        FIELD_VALUE,
        OLD_PASSPHRASE.as_bytes(),
        BACKUP_PASSPHRASE.as_bytes(),
        LATEST_PASSPHRASE.as_bytes(),
    ] {
        assert!(
            !bytes.windows(secret.len()).any(|part| part == secret),
            "lifecycle output contained protected test bytes"
        );
    }
}

#[test]
fn passphrase_change_backup_and_restore_preserve_state_without_leaks() {
    let temp = private_tempdir();
    let (home, source) = initialized_vault(temp.path());
    let old = SecretString::from(OLD_PASSPHRASE.to_owned());
    let backup_passphrase = SecretString::from(BACKUP_PASSPHRASE.to_owned());
    let latest = SecretString::from(LATEST_PASSPHRASE.to_owned());
    let fields_before = source.list_fields(&old).unwrap();

    let vault_before_incomplete_capture = std::fs::read(source.root().join("vault.json")).unwrap();
    let audit_before_incomplete_capture = std::fs::read(source.root().join("audit.jsonl")).unwrap();
    let incomplete_capture = jig_with_passphrases(
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
    assert!(!incomplete_capture.status.success());
    let incomplete_output = combined_output(&incomplete_capture);
    let incomplete_text = String::from_utf8_lossy(&incomplete_output);
    assert!(incomplete_text.contains("JIG_VAULT_PASSPHRASE"));
    assert!(incomplete_text.contains("JIG_VAULT_NEW_PASSPHRASE"));
    assert_contains_no_lifecycle_secrets(&incomplete_output);
    assert_eq!(
        std::fs::read(source.root().join("vault.json")).unwrap(),
        vault_before_incomplete_capture
    );
    assert_eq!(
        std::fs::read(source.root().join("audit.jsonl")).unwrap(),
        audit_before_incomplete_capture
    );

    let changed = jig_with_passphrases(
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
        changed.status.success(),
        "{}",
        String::from_utf8_lossy(&changed.stderr)
    );
    assert_eq!(output_json(&changed)["changed"], true);
    assert_contains_no_lifecycle_secrets(&combined_output(&changed));
    assert!(source.list_fields(&old).is_err());
    assert_eq!(
        source.list_fields(&backup_passphrase).unwrap(),
        fields_before
    );

    let backup_one = temp.path().join("vault-one.backup");
    let created_one = jig_with_passphrases(
        [
            "--json".as_ref(),
            "vault".as_ref(),
            "backup".as_ref(),
            "create".as_ref(),
            "--out".as_ref(),
            backup_one.as_os_str(),
            "--home".as_ref(),
            home.as_os_str(),
        ],
        BACKUP_PASSPHRASE,
        None,
    );
    assert!(
        created_one.status.success(),
        "{}",
        String::from_utf8_lossy(&created_one.stderr)
    );
    let created_json = output_json(&created_one);
    assert_eq!(created_json["command"], "vault backup create");
    assert_eq!(created_json["backup_version"], 1);
    assert!(created_json["bytes_written"].as_u64().unwrap() > 0);
    assert_contains_no_lifecycle_secrets(&combined_output(&created_one));
    let backup_one_bytes = std::fs::read(&backup_one).unwrap();
    assert_contains_no_lifecycle_secrets(&backup_one_bytes);
    for audit_metadata in [
        b"vault_initialized".as_slice(),
        b"field_batch_apply".as_slice(),
        b"passphrase_change".as_slice(),
    ] {
        assert!(
            !backup_one_bytes
                .windows(audit_metadata.len())
                .any(|part| part == audit_metadata),
            "backup exposed plaintext audit metadata"
        );
    }
    assert_eq!(
        std::fs::metadata(&backup_one).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let audit_before_refusal = std::fs::read(source.root().join("audit.jsonl")).unwrap();
    let refused_overwrite = jig_with_passphrases(
        [
            "--json".as_ref(),
            "vault".as_ref(),
            "backup".as_ref(),
            "create".as_ref(),
            "--out".as_ref(),
            backup_one.as_os_str(),
            "--home".as_ref(),
            home.as_os_str(),
        ],
        BACKUP_PASSPHRASE,
        None,
    );
    assert!(!refused_overwrite.status.success());
    assert_eq!(std::fs::read(&backup_one).unwrap(), backup_one_bytes);
    assert_eq!(
        std::fs::read(source.root().join("audit.jsonl")).unwrap(),
        audit_before_refusal
    );
    assert_contains_no_lifecycle_secrets(&combined_output(&refused_overwrite));

    let overwritten = jig_with_passphrases(
        [
            "--json".as_ref(),
            "vault".as_ref(),
            "backup".as_ref(),
            "create".as_ref(),
            "--out".as_ref(),
            backup_one.as_os_str(),
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
    let overwritten_bytes = std::fs::read(&backup_one).unwrap();
    assert_ne!(backup_one_bytes, overwritten_bytes);
    assert_contains_no_lifecycle_secrets(&overwritten_bytes);

    let changed_again = jig_with_passphrases(
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
    assert!(changed_again.status.success());

    source.verify_audit(&latest).unwrap();
    let source_audit = std::fs::read(source.root().join("audit.jsonl")).unwrap();
    assert!(
        source_audit
            .windows(b"passphrase_change".len())
            .any(|part| part == b"passphrase_change")
    );
    assert_contains_no_lifecycle_secrets(&source_audit);

    #[cfg(target_os = "linux")]
    {
        let symlinked_backup = temp.path().join("vault-link.backup");
        symlink(&backup_one, &symlinked_backup).unwrap();
        let symlink_target = temp.path().join("symlink-restore-home");
        let refused_symlink = jig_with_passphrases(
            [
                "--json".as_ref(),
                "vault".as_ref(),
                "backup".as_ref(),
                "restore".as_ref(),
                "--in".as_ref(),
                symlinked_backup.as_os_str(),
                "--home".as_ref(),
                symlink_target.as_os_str(),
            ],
            BACKUP_PASSPHRASE,
            None,
        );
        assert!(!refused_symlink.status.success());
        assert!(!symlink_target.exists());

        let existing_target = temp.path().join("existing-restore-home");
        std::fs::create_dir(&existing_target).unwrap();
        let refused_existing = jig_with_passphrases(
            [
                "--json".as_ref(),
                "vault".as_ref(),
                "backup".as_ref(),
                "restore".as_ref(),
                "--in".as_ref(),
                backup_one.as_os_str(),
                "--home".as_ref(),
                existing_target.as_os_str(),
            ],
            BACKUP_PASSPHRASE,
            None,
        );
        assert!(!refused_existing.status.success());
        assert!(existing_target.read_dir().unwrap().next().is_none());

        let restored_parent = temp.path().join("restore-base/scopes");
        let restored_home = restored_parent.join("restored-vault-home");
        let wrong_passphrase = jig_with_passphrases(
            [
                "--json".as_ref(),
                "vault".as_ref(),
                "backup".as_ref(),
                "restore".as_ref(),
                "--in".as_ref(),
                backup_one.as_os_str(),
                "--home".as_ref(),
                restored_home.as_os_str(),
            ],
            LATEST_PASSPHRASE,
            None,
        );
        assert!(!wrong_passphrase.status.success());
        assert!(!restored_home.exists());
        let wrong_passphrase_output = combined_output(&wrong_passphrase);
        assert!(
            String::from_utf8_lossy(&wrong_passphrase_output)
                .contains("authenticate backup archive")
        );
        assert_contains_no_lifecycle_secrets(&wrong_passphrase_output);
        for created in [temp.path().join("restore-base"), restored_parent] {
            assert!(created.is_dir());
            assert_eq!(
                std::fs::metadata(created).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }

        let restored = jig_with_passphrases(
            [
                "--json".as_ref(),
                "vault".as_ref(),
                "backup".as_ref(),
                "restore".as_ref(),
                "--in".as_ref(),
                backup_one.as_os_str(),
                "--home".as_ref(),
                restored_home.as_os_str(),
            ],
            BACKUP_PASSPHRASE,
            None,
        );
        let restored_output = combined_output(&restored);
        assert!(
            restored.status.success(),
            "{}",
            String::from_utf8_lossy(&restored_output)
        );
        let restored_json = output_json(&restored);
        assert_eq!(restored_json["command"], "vault backup restore");
        assert_eq!(restored_json["restored"], true);
        assert_eq!(restored_json["format_version"], 2);
        assert_contains_no_lifecycle_secrets(&restored_output);

        let restored_vault = Vault::resolve_for_test(Some(restored_home)).unwrap();
        assert_eq!(
            restored_vault.list_fields(&backup_passphrase).unwrap(),
            fields_before
        );
        let mut restored_value = Vec::new();
        restored_vault
            .read_field_to(
                &backup_passphrase,
                reference("jig://Production/TOKEN"),
                &mut restored_value,
            )
            .unwrap();
        assert_eq!(restored_value, FIELD_VALUE);
        restored_vault.verify_audit(&backup_passphrase).unwrap();
        let restored_audit = std::fs::read(restored_vault.root().join("audit.jsonl")).unwrap();
        assert!(
            restored_audit
                .windows(b"backup_restore".len())
                .any(|part| part == b"backup_restore")
        );
        assert_contains_no_lifecycle_secrets(&restored_audit);

        let legacy_home = temp.path().join("legacy-restored-vault");
        let mut legacy_restore = Command::new(env!("CARGO_BIN_EXE_jig"));
        legacy_restore
            .current_dir(temp.path())
            .args(["--json", "vault", "backup", "restore", "--in"])
            .arg(&backup_one)
            .env("JIG_VAULT_HOME", &legacy_home)
            .env("JIG_VAULT_PASSPHRASE", BACKUP_PASSPHRASE)
            .env_remove("JIG_VAULT_NEW_PASSPHRASE")
            .env_remove("JIG_REPO_ROOT")
            .env_remove("JIG_INVOKE_CWD");
        let legacy_restored = legacy_restore.output().unwrap();
        let legacy_output = combined_output(&legacy_restored);
        assert!(
            legacy_restored.status.success(),
            "{}",
            String::from_utf8_lossy(&legacy_output)
        );
        let legacy_json = output_json(&legacy_restored);
        assert_eq!(legacy_json["vault_scope"], "legacy");
        assert_eq!(legacy_json["vault_home"], legacy_home.display().to_string());
        assert_contains_no_lifecycle_secrets(&legacy_output);
        Vault::resolve_for_test(Some(legacy_home))
            .unwrap()
            .verify_audit(&backup_passphrase)
            .unwrap();
    }
}
