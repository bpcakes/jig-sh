#![cfg(target_os = "linux")]

#[path = "vault_lifecycle_parts/support.rs"]
mod vault_lifecycle_support;

use vault_lifecycle_support::*;

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

    assert_incomplete_change_is_atomic(&home, &source);
    change_to_backup_passphrase(&home, &source, &old, &backup_passphrase, &fields_before);

    let (backup_one, backup_one_bytes) = create_first_backup(temp.path(), &home);

    assert_backup_overwrite_contract(&home, &source, &backup_one, &backup_one_bytes);

    change_to_latest_and_verify(&home, &source, &latest);

    assert_restore_contract(temp.path(), &backup_one, &backup_passphrase, &fields_before);
}
