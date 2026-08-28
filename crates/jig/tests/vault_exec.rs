#![cfg(unix)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use jig_vault::{FieldKind, FieldMutation, SecretBytes, Vault, VaultReference};
use secrecy::SecretString;

const PASSPHRASE: &str = "correct horse battery staple";
const SECRET: &[u8] = b"transparent-secret-value";

fn reference(value: &str) -> VaultReference {
    value.parse().unwrap()
}

fn initialized_vault() -> (tempfile::TempDir, PathBuf, Vault) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("vault-home");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    let passphrase = SecretString::from(PASSPHRASE.to_owned());
    vault.init(&passphrase).unwrap();
    vault
        .apply_field_batch(
            &passphrase,
            vec![
                FieldMutation::set(
                    reference("jig://Production/TOKEN"),
                    FieldKind::Concealed,
                    SecretBytes::new(SECRET.to_vec()),
                ),
                FieldMutation::set(
                    reference("jig://Production/FLAG"),
                    FieldKind::Text,
                    SecretBytes::new(b"false".to_vec()),
                ),
            ],
        )
        .unwrap();
    (temp, home, vault)
}

fn write_env_file(path: &Path) {
    std::fs::write(
        path,
        b"TOKEN=jig://Production/TOKEN\nFLAG=jig://Production/FLAG\nLITERAL='literal value'\nOVERRIDE=dotenv\n",
    )
    .unwrap();
}

fn exec_output(home: &Path, env_file: &Path, script: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["vault", "exec", "--env-file"])
        .arg(env_file)
        .arg("--home")
        .arg(home)
        .args(["--", "sh", "-c", script])
        .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
        .output()
        .unwrap()
}

#[test]
fn exec_inherits_and_overrides_environment_streams_redacted_output_and_passes_stdin() {
    let (temp, home, vault) = initialized_vault();
    let env_file = temp.path().join("exec.env");
    write_env_file(&env_file);
    let script = "read input; printf 'argv-audit-marker token=%s flag=%s literal=%s ordinary=%s override=%s current=%s new=%s stdin=%s\\n' \"$TOKEN\" \"$FLAG\" \"$LITERAL\" \"$ORDINARY_PARENT\" \"$OVERRIDE\" \"${JIG_VAULT_PASSPHRASE-unset}\" \"${JIG_VAULT_NEW_PASSPHRASE-unset}\" \"$input\"; printf 'err=%s\\n' \"$TOKEN\" >&2";
    let mut child = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["vault", "exec", "--env-file"])
        .arg(&env_file)
        .arg("--home")
        .arg(&home)
        .args(["--", "sh", "-c", script])
        .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
        .env("JIG_VAULT_NEW_PASSPHRASE", "must-not-reach-child")
        .env("ORDINARY_PARENT", "parent")
        .env("OVERRIDE", "parent-value")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"stdin-value\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"argv-audit-marker token=[REDACTED] flag=false literal=literal value ordinary=parent override=dotenv current=unset new=unset stdin=stdin-value\n"
    );
    assert_eq!(output.stderr, b"err=[REDACTED]\n");
    assert!(
        !output
            .stdout
            .windows(SECRET.len())
            .any(|part| part == SECRET)
    );
    assert!(
        !output
            .stderr
            .windows(SECRET.len())
            .any(|part| part == SECRET)
    );

    let audit = std::fs::read(vault.root().join("audit.jsonl")).unwrap();
    assert!(!audit.windows(SECRET.len()).any(|part| part == SECRET));
    assert!(
        !audit
            .windows(b"argv-audit-marker".len())
            .any(|part| part == b"argv-audit-marker")
    );
}

#[test]
fn exec_mirrors_nonzero_and_signal_status_without_a_second_jig_error() {
    let (temp, home, _vault) = initialized_vault();
    let env_file = temp.path().join("exec.env");
    write_env_file(&env_file);

    let failed = exec_output(
        &home,
        &env_file,
        "printf child-out; printf child-err >&2; exit 37",
    );
    assert_eq!(failed.status.code(), Some(37));
    assert_eq!(failed.stdout, b"child-out");
    assert_eq!(failed.stderr, b"child-err");

    let signalled = exec_output(&home, &env_file, "kill -TERM $$");
    assert_eq!(signalled.status.code(), Some(143));
    assert!(signalled.stdout.is_empty());
    assert!(signalled.stderr.is_empty());
}
