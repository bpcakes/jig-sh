#![cfg(any(target_os = "linux", target_os = "macos"))]
//! Recovery of interrupted witnessed vault transactions through the CLI.
//!
//! Pending states are created in-process through the `test-utils` fault
//! hooks; the `jig` binary then finishes them. Both sides derive the same
//! disposable witness beside the temporary vault homes.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use jig_vault::test_support::{TransactionFaultPoint, arm_transaction_fault};
use jig_vault::{FieldKind, SecretBytes, Vault};
use secrecy::SecretString;

const PASSPHRASE: &str = "test-only-recovery-passphrase";

#[path = "vault_recovery_parts/rekey_policy.rs"]
mod rekey_policy;

fn passphrase() -> SecretString {
    SecretString::from(PASSPHRASE.to_owned())
}

fn private_tempdir() -> tempfile::TempDir {
    let root = std::env::temp_dir().canonicalize().unwrap();
    let temp = tempfile::Builder::new()
        .prefix("jig-vault-recovery-")
        .tempdir_in(root)
        .unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    temp
}

fn jig(args: &[&str], home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jig"))
        .arg("--json")
        .args(["vault"])
        .args(args)
        .arg("--home")
        .arg(home)
        .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
        .env_remove("JIG_VAULT_NEW_PASSPHRASE")
        .env_remove("JIG_VAULT_WITNESS_ROOT")
        .output()
        .unwrap()
}

fn json(output: &Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// The combined failure report; `--json` errors are written to stdout.
fn failure(output: &Output) -> String {
    assert!(!output.status.success());
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn source_with_backup(temp: &Path, name: &str) -> (PathBuf, Vault, PathBuf) {
    let home = temp.join(name);
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    vault.init(&passphrase()).unwrap();
    vault
        .set_field(
            &passphrase(),
            "jig://Example/TOKEN".parse().unwrap(),
            FieldKind::Text,
            SecretBytes::new(b"recovery value".to_vec()),
        )
        .unwrap();
    let archive = temp.join(format!("{name}.backup"));
    backup(&home, &archive);
    (home, vault, archive)
}

fn backup(home: &Path, archive: &Path) {
    let request = Vault::preflight_backup_create(home.to_path_buf(), archive, false).unwrap();
    Vault::create_backup(&passphrase(), request).unwrap();
}

fn pending_restore(archive: &Path, target: &Path, point: TransactionFaultPoint) {
    let request = Vault::preflight_backup_restore(archive, target.to_path_buf()).unwrap();
    arm_transaction_fault(point);
    assert!(Vault::restore_backup(&passphrase(), request).is_err());
}

#[test]
fn an_interrupted_init_is_reported_and_resumed_by_init() {
    let temp = private_tempdir();
    let home = temp.path().join("vault");
    arm_transaction_fault(TransactionFaultPoint::AfterAudit);
    assert!(
        Vault::resolve_for_test(Some(home.clone()))
            .unwrap()
            .init(&passphrase())
            .is_err()
    );

    let status = json(&jig(&["status"], &home));
    assert_eq!(status["pending_transaction"], true);
    assert_eq!(status["exists"], false);

    json(&jig(&["init"], &home));
    let status = json(&jig(&["status"], &home));
    assert_eq!(status["pending_transaction"], false);
    assert_eq!(status["exists"], true);
    assert_eq!(status["format_version"], 3);
}

#[test]
fn an_absent_restore_target_is_finished_without_the_archive_or_eager_creation() {
    let temp = private_tempdir();
    let (_home, _vault, archive) = source_with_backup(temp.path(), "source");
    let target = temp.path().join("restored");
    pending_restore(&archive, &target, TransactionFaultPoint::AfterPending);
    std::fs::remove_file(&archive).unwrap();

    let status = json(&jig(&["status"], &target));
    assert_eq!(status["pending_transaction"], true);
    assert!(!target.exists());

    let listed = json(&jig(&["field", "list"], &target));
    assert_eq!(listed["fields"].as_array().unwrap().len(), 1);
    assert!(target.exists());
    assert_eq!(
        json(&jig(&["status"], &target))["pending_transaction"],
        false
    );
}

#[test]
fn a_restore_installed_before_promotion_is_finished_by_retrying_it() {
    let temp = private_tempdir();
    let (_home, _vault, archive) = source_with_backup(temp.path(), "source");
    let target = temp.path().join("restored");
    pending_restore(&archive, &target, TransactionFaultPoint::AfterEnvelope);
    assert!(target.exists());

    let mut args = vec!["backup", "restore", "--in"];
    let archive_arg = archive.to_str().unwrap();
    args.push(archive_arg);
    let restored = json(&jig(&args, &target));
    assert_eq!(restored["restored"], true);
    assert_eq!(restored["format_version"], 3);
    assert_eq!(restored["generation"], 3);
    assert_eq!(restored["other_copies_stale"], true);
}

#[test]
fn a_retry_with_a_different_archive_is_refused_with_operator_guidance() {
    let temp = private_tempdir();
    let (home, _vault, archive) = source_with_backup(temp.path(), "source");
    let other = temp.path().join("other.backup");
    backup(&home, &other);
    let target = temp.path().join("restored");
    pending_restore(&archive, &target, TransactionFaultPoint::AfterPending);

    let error = failure(&jig(
        &["backup", "restore", "--in", other.to_str().unwrap()],
        &target,
    ));
    assert!(error.contains("different archive"), "{error}");
    assert!(error.contains("Operator step"), "{error}");
    assert!(!error.contains("recovery value"));
    assert!(!target.exists());
}

#[test]
fn a_rolled_back_copy_is_refused_value_free() {
    let temp = private_tempdir();
    let (home, vault, _archive) = source_with_backup(temp.path(), "source");
    let old_vault = std::fs::read(home.join("vault.json")).unwrap();
    let old_audit = std::fs::read(home.join("audit.jsonl")).unwrap();
    vault
        .set_field(
            &passphrase(),
            "jig://Example/LATER".parse().unwrap(),
            FieldKind::Text,
            SecretBytes::new(b"later value".to_vec()),
        )
        .unwrap();
    std::fs::write(home.join("vault.json"), old_vault).unwrap();
    std::fs::write(home.join("audit.jsonl"), old_audit).unwrap();

    let error = failure(&jig(&["field", "list"], &home));
    assert!(
        error.contains("older than its witnessed generation"),
        "{error}"
    );
    assert!(!error.contains("recovery value"));
}

#[test]
fn a_pending_restore_journal_claiming_another_vault_is_kept_and_refused() {
    let temp = private_tempdir();
    let (_home, _vault, archive) = source_with_backup(temp.path(), "source");
    let target = temp.path().join("restored");
    pending_restore(&archive, &target, TransactionFaultPoint::AfterPending);
    let journals = temp.path().join(".jig-vault-witness/journals");
    let [journal] = std::fs::read_dir(journals)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
    value["vault_id"] = "01EXAMPLEOTHERVAULTID0000000".into();
    let changed = serde_json::to_vec(&value).unwrap();
    std::fs::write(&journal, &changed).unwrap();

    let error = failure(&jig(
        &["backup", "restore", "--in", archive.to_str().unwrap()],
        &target,
    ));
    assert!(error.contains("does not match its marker"), "{error}");
    assert!(!error.contains("recovery value"));
    assert_eq!(std::fs::read(&journal).unwrap(), changed);
    assert!(!target.exists());
}
