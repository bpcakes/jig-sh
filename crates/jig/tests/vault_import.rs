#![cfg(unix)]

use std::io::Write;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use jig_vault::{FieldKind, Vault, VaultReference};
use secrecy::SecretString;

const PASSPHRASE: &str = "correct horse battery staple";
const STDERR_SECRET: &str = "op-stderr-value-must-not-leak";

fn private_tempdir() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    temp
}

fn initialized_vault(root: &Path) -> (PathBuf, Vault) {
    let home = root.join("vault-home");
    let vault = Vault::resolve(Some(home.clone())).unwrap();
    vault
        .init(&SecretString::from(PASSPHRASE.to_owned()))
        .unwrap();
    (home, vault)
}

fn install_fake_op(root: &Path) -> PathBuf {
    let bin = root.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let op = bin.join("op");
    std::fs::write(
        &op,
        format!(
            r#"#!/bin/sh
set -eu
if [ "${{JIG_VAULT_PASSPHRASE+set}}" = set ] || [ "${{JIG_VAULT_NEW_PASSPHRASE+set}}" = set ]; then
  printf '%s\n' 'reserved-passphrase-env-was-inherited' >> "$OP_TEST_LOG"
  exit 87
fi
if IFS= read -r unexpected; then
  printf '%s\n' 'stdin-was-not-null' >> "$OP_TEST_LOG"
  exit 88
fi
{{
  printf 'argc=<%s>\n' "$#"
  for argument in "$@"; do
    printf 'arg=<%s>\n' "$argument"
  done
}} >> "$OP_TEST_LOG"
case "$3" in
  'op://Test/Login/TOKEN') printf '%s' 'secret-no-newline' ;;
  'op://Test/Login/FIRST') printf '%s' 'first-secret' ;;
  'op://Test/Login/FAIL') printf '%s' '{STDERR_SECRET}' >&2; exit 9 ;;
  'op://Test/Login/HUGE') dd if=/dev/zero bs=1048576 count=2 2>/dev/null ;;
  'op://Test/Login/HOLD') (sleep 30) & printf '%s' 'held-pipe-secret' ;;
  'op://Test/Login/DETACHED')
    (exec </dev/null >/dev/null 2>&1; sleep 30) &
    printf '%s' "$!" > "$OP_DESCENDANT_PID"
    printf '%s' 'detached-secret'
    ;;
  *) printf '%s' 'injection-secret' ;;
esac
"#
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&op).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&op, permissions).unwrap();
    bin
}

fn import_output(
    cwd: &Path,
    home: &Path,
    fake_bin: &Path,
    log: &Path,
    source: &Path,
    destination: &Path,
    extra: &[&str],
) -> Output {
    let mut path_parts = vec![fake_bin.to_path_buf()];
    if let Some(path) = std::env::var_os("PATH") {
        path_parts.extend(std::env::split_paths(&path));
    }
    let path = std::env::join_paths(path_parts).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    command
        .current_dir(cwd)
        .args(["--json", "vault", "import", "onepassword", "--env-file"])
        .arg(source)
        .args(["--item", "Production", "--out-env"])
        .arg(destination)
        .arg("--home")
        .arg(home)
        .args(extra)
        .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
        .env(
            "JIG_VAULT_NEW_PASSPHRASE",
            "test-only-new-passphrase-must-not-reach-op",
        )
        .env("OP_TEST_LOG", log)
        .env("OP_DESCENDANT_PID", log.with_extension("descendant-pid"))
        .env("PATH", path);
    command.output().unwrap()
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON ({error}); stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn reference(value: &str) -> VaultReference {
    VaultReference::parse(value).unwrap()
}

fn process_disappears(pid: libc::pid_t) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        // SAFETY: signal 0 performs only a liveness/permission check for the
        // numeric pid written by the isolated fake-op child.
        if unsafe { libc::kill(pid, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn imports_literals_and_exact_op_references_and_reruns_convergently() {
    let temp = private_tempdir();
    let (home, vault) = initialized_vault(temp.path());
    let fake_bin = install_fake_op(temp.path());
    let log = temp.path().join("op.log");
    let source = temp.path().join("source.env");
    let destination = temp.path().join("generated.env");
    let injection_marker = temp.path().join("shell-injection-ran");
    std::fs::write(
        &source,
        b"TOKEN=op://Test/Login/TOKEN\nMODE=production\nINJECT='op://Test/Login/value;touch shell-injection-ran'\n",
    )
    .unwrap();

    let dry_run = import_output(
        temp.path(),
        &home,
        &fake_bin,
        &log,
        &source,
        &destination,
        &["--dry-run"],
    );
    assert!(dry_run.status.success(), "{}", combined_output(&dry_run));
    let dry_run_json = json(&dry_run);
    assert_eq!(dry_run_json["dry_run"], true);
    assert_eq!(dry_run_json["fields"][0]["kind"], "concealed");
    assert_eq!(dry_run_json["fields"][1]["kind"], "text");
    assert_eq!(dry_run_json["fields"][0]["action"], "create");
    assert!(!log.exists(), "dry-run unexpectedly invoked op");
    assert!(!destination.exists());

    let imported = import_output(
        temp.path(),
        &home,
        &fake_bin,
        &log,
        &source,
        &destination,
        &[],
    );
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        b"TOKEN=jig://Production/TOKEN\nMODE=jig://Production/MODE\nINJECT=jig://Production/INJECT\n"
    );
    assert_eq!(
        std::fs::metadata(&destination)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(!injection_marker.exists());

    let op_log = std::fs::read_to_string(&log).unwrap();
    assert_eq!(op_log.matches("argc=<3>").count(), 2);
    assert_eq!(op_log.matches("arg=<read>").count(), 2);
    assert_eq!(op_log.matches("arg=<--no-newline>").count(), 2);
    assert!(op_log.contains("arg=<op://Test/Login/TOKEN>"));
    assert!(op_log.contains("arg=<op://Test/Login/value;touch shell-injection-ran>"));
    assert!(!op_log.contains("stdin-was-not-null"));
    assert!(!op_log.contains("reserved-passphrase-env-was-inherited"));

    let passphrase = SecretString::from(PASSPHRASE.to_owned());
    let fields = vault.list_fields(&passphrase).unwrap();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].kind, FieldKind::Concealed);
    assert_eq!(fields[1].kind, FieldKind::Text);
    assert_eq!(fields[2].kind, FieldKind::Concealed);
    let mut token = Vec::new();
    vault
        .read_field_to(&passphrase, reference("jig://Production/TOKEN"), &mut token)
        .unwrap();
    assert_eq!(token, b"secret-no-newline");

    let log_len = std::fs::metadata(&log).unwrap().len();
    let refused = import_output(
        temp.path(),
        &home,
        &fake_bin,
        &log,
        &source,
        &destination,
        &[],
    );
    assert!(!refused.status.success());
    assert_eq!(std::fs::metadata(&log).unwrap().len(), log_len);

    let existing_dry_run = import_output(
        temp.path(),
        &home,
        &fake_bin,
        &log,
        &source,
        &destination,
        &["--dry-run"],
    );
    assert!(existing_dry_run.status.success());
    let existing_json = json(&existing_dry_run);
    assert_eq!(existing_json["requires_replace"], true);
    assert_eq!(existing_json["requires_overwrite"], true);
    assert_eq!(existing_json["fields"][0]["action"], "replace");
    assert_eq!(std::fs::metadata(&log).unwrap().len(), log_len);

    let rerun = import_output(
        temp.path(),
        &home,
        &fake_bin,
        &log,
        &source,
        &destination,
        &["--replace", "--overwrite"],
    );
    assert!(
        rerun.status.success(),
        "{}",
        String::from_utf8_lossy(&rerun.stderr)
    );
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        b"TOKEN=jig://Production/TOKEN\nMODE=jig://Production/MODE\nINJECT=jig://Production/INJECT\n"
    );

    let audit = std::fs::read(vault.root().join("audit.jsonl")).unwrap();
    for forbidden in [
        b"secret-no-newline".as_slice(),
        b"injection-secret".as_slice(),
        b"production".as_slice(),
        b"op://Test".as_slice(),
    ] {
        assert!(!audit.windows(forbidden.len()).any(|part| part == forbidden));
    }
}

#[test]
fn last_resolution_failure_and_oversized_output_leave_vault_and_destination_unchanged() {
    let temp = private_tempdir();
    let (home, vault) = initialized_vault(temp.path());
    let fake_bin = install_fake_op(temp.path());
    let log = temp.path().join("op.log");
    let source = temp.path().join("failure.env");
    let destination = temp.path().join("failure-output.env");
    std::fs::write(
        &source,
        b"FIRST=op://Test/Login/FIRST\nLAST=op://Test/Login/FAIL\n",
    )
    .unwrap();

    let failed = import_output(
        temp.path(),
        &home,
        &fake_bin,
        &log,
        &source,
        &destination,
        &[],
    );
    assert!(!failed.status.success());
    let failure_text = combined_output(&failed);
    assert!(failure_text.contains("variable 'LAST'"));
    assert!(failure_text.contains("exit status 9"));
    assert!(!failure_text.contains(STDERR_SECRET));
    assert!(!destination.exists());
    let passphrase = SecretString::from(PASSPHRASE.to_owned());
    assert!(vault.list_fields(&passphrase).unwrap().is_empty());

    std::fs::write(&source, b"HUGE=op://Test/Login/HUGE\n").unwrap();
    let started = Instant::now();
    let oversized = import_output(
        temp.path(),
        &home,
        &fake_bin,
        &log,
        &source,
        &destination,
        &[],
    );
    // Keep this comfortably below the resolver's 30-second deadline while
    // allowing for the vault KDF on loaded CI hosts.
    assert!(started.elapsed() < Duration::from_secs(20));
    assert!(!oversized.status.success());
    let oversized_text = combined_output(&oversized);
    assert!(oversized_text.contains("variable 'HUGE'"));
    assert!(oversized_text.contains("byte limit") || oversized_text.contains("safety limit"));
    assert!(!destination.exists());
    assert!(vault.list_fields(&passphrase).unwrap().is_empty());

    let audit = std::fs::read(vault.root().join("audit.jsonl")).unwrap();
    assert!(
        !audit
            .windows(STDERR_SECRET.len())
            .any(|part| part == STDERR_SECRET.as_bytes())
    );
    assert!(
        !audit
            .windows(b"first-secret".len())
            .any(|part| part == b"first-secret")
    );
}

#[test]
fn op_descendants_cannot_hold_pipes_or_survive_after_leader_exit() {
    let temp = private_tempdir();
    let (home, vault) = initialized_vault(temp.path());
    let fake_bin = install_fake_op(temp.path());
    let log = temp.path().join("op.log");
    let source = temp.path().join("held-pipe.env");
    let destination = temp.path().join("held-pipe-output.env");
    std::fs::write(
        &source,
        b"HOLD=op://Test/Login/HOLD\nDETACHED=op://Test/Login/DETACHED\n",
    )
    .unwrap();

    let started = Instant::now();
    let imported = import_output(
        temp.path(),
        &home,
        &fake_bin,
        &log,
        &source,
        &destination,
        &[],
    );
    assert!(
        started.elapsed() < Duration::from_secs(25),
        "an op descendant kept the import pipes open"
    );
    assert!(imported.status.success(), "{}", combined_output(&imported));
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        b"HOLD=jig://Production/HOLD\nDETACHED=jig://Production/DETACHED\n"
    );

    let descendant_pid = std::fs::read_to_string(log.with_extension("descendant-pid"))
        .unwrap()
        .parse::<libc::pid_t>()
        .unwrap();
    assert!(
        process_disappears(descendant_pid),
        "an op descendant that closed all stdio survived group cleanup"
    );

    let passphrase = SecretString::from(PASSPHRASE.to_owned());
    let mut value = Vec::new();
    vault
        .read_field_to(&passphrase, reference("jig://Production/HOLD"), &mut value)
        .unwrap();
    assert_eq!(value, b"held-pipe-secret");
    value.clear();
    vault
        .read_field_to(
            &passphrase,
            reference("jig://Production/DETACHED"),
            &mut value,
        )
        .unwrap();
    assert_eq!(value, b"detached-secret");
}

#[test]
fn non_utf8_recovery_paths_fail_before_op_or_import_mutation() {
    let temp = private_tempdir();
    let (home, vault) = initialized_vault(temp.path());
    let fake_bin = install_fake_op(temp.path());
    let log = temp.path().join("op.log");
    let valid_source = temp.path().join("valid.env");
    let valid_destination = temp.path().join("valid-output.env");
    std::fs::write(&valid_source, b"TOKEN=op://Test/Login/TOKEN\n").unwrap();

    let non_utf8_source = temp
        .path()
        .join(std::ffi::OsString::from_vec(b"source-\xff.env".to_vec()));
    std::fs::write(&non_utf8_source, b"TOKEN=op://Test/Login/TOKEN\n").unwrap();
    let source_error = import_output(
        temp.path(),
        &home,
        &fake_bin,
        &log,
        &non_utf8_source,
        &valid_destination,
        &[],
    );
    assert!(!source_error.status.success());
    assert!(combined_output(&source_error).contains("source path is not valid UTF-8"));

    let non_utf8_destination = temp.path().join(std::ffi::OsString::from_vec(
        b"destination-\xff.env".to_vec(),
    ));
    let destination_error = import_output(
        temp.path(),
        &home,
        &fake_bin,
        &log,
        &valid_source,
        &non_utf8_destination,
        &[],
    );
    assert!(!destination_error.status.success());
    assert!(combined_output(&destination_error).contains("destination path is not valid UTF-8"));

    let non_utf8_home = temp
        .path()
        .join(std::ffi::OsString::from_vec(b"vault-\xff-home".to_vec()));
    let non_utf8_vault = Vault::resolve(Some(non_utf8_home.clone())).unwrap();
    non_utf8_vault
        .init(&SecretString::from(PASSPHRASE.to_owned()))
        .unwrap();
    let home_error = import_output(
        temp.path(),
        &non_utf8_home,
        &fake_bin,
        &log,
        &valid_source,
        &valid_destination,
        &[],
    );
    assert!(!home_error.status.success());
    assert!(combined_output(&home_error).contains("vault home path is not valid UTF-8"));

    assert!(
        !log.exists(),
        "invalid recovery metadata unexpectedly invoked op"
    );
    assert!(!valid_destination.exists());
    assert!(!non_utf8_destination.exists());
    let passphrase = SecretString::from(PASSPHRASE.to_owned());
    assert!(vault.list_fields(&passphrase).unwrap().is_empty());
    assert!(non_utf8_vault.list_fields(&passphrase).unwrap().is_empty());
}

#[test]
fn post_prepare_destination_collision_reports_committed_import_and_reruns_safely() {
    let temp = private_tempdir();
    let (home, vault) = initialized_vault(temp.path());
    let fake_bin = install_fake_op(temp.path());
    let log = temp.path().join("op.log");
    let source = temp.path().join("collision.env");
    let destination = temp.path().join("collision-output.env");
    std::fs::write(&source, b"TOKEN=collision-literal\n").unwrap();

    let watched_parent = temp.path().to_path_buf();
    let watched_destination = destination.clone();
    let watcher = std::thread::spawn(move || {
        let file_name = watched_destination.file_name().unwrap().to_string_lossy();
        let prefix = format!(".{file_name}.");
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            let prepared_exists = std::fs::read_dir(&watched_parent)
                .unwrap()
                .filter_map(std::result::Result::ok)
                .any(|entry| {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    name.starts_with(&prefix) && name.ends_with(".jig-vault-output.tmp")
                });
            if prepared_exists {
                let mut sentinel = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&watched_destination)
                    .unwrap();
                sentinel.write_all(b"destination-race-sentinel").unwrap();
                sentinel.sync_all().unwrap();
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    });

    let collided = import_output(
        temp.path(),
        &home,
        &fake_bin,
        &log,
        &source,
        &destination,
        &[],
    );
    assert!(
        watcher.join().unwrap(),
        "never observed the prepared output"
    );
    assert!(!collided.status.success());
    let error = combined_output(&collided);
    assert!(error.contains("vault import succeeded"));
    assert!(error.contains("Safe rerun:"));
    assert!(error.contains("--replace --overwrite"));
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        b"destination-race-sentinel"
    );
    assert!(!log.exists(), "literal-only import unexpectedly invoked op");

    let passphrase = SecretString::from(PASSPHRASE.to_owned());
    let fields = vault.list_fields(&passphrase).unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Text);
    let audit = std::fs::read_to_string(vault.root().join("audit.jsonl")).unwrap();
    assert_eq!(
        audit.matches("\"action\":\"onepassword_import\"").count(),
        1
    );

    let rerun = import_output(
        temp.path(),
        &home,
        &fake_bin,
        &log,
        &source,
        &destination,
        &["--replace", "--overwrite"],
    );
    assert!(rerun.status.success(), "{}", combined_output(&rerun));
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        b"TOKEN=jig://Production/TOKEN\n"
    );
}
