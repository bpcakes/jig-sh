#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use jig_vault::{FieldKind, FieldMutation, SecretBytes, Vault, VaultReference};
use secrecy::SecretString;

const PASSPHRASE: &str = "correct horse battery staple";
const TOKEN: &[u8] = b"canonical-run-token-value";
const FLAG: &[u8] = b"canonical-run-text-flag";
const OPERATOR_GUIDANCE_MARKERS: [&str; 4] = [
    "run the exact command in a terminal",
    "automation outside the agent session",
    "must never request, print, store, or choose a vault passphrase",
    "must not set JIG_VAULT_PASSPHRASE or JIG_VAULT_NEW_PASSPHRASE themselves",
];

/// Runs `jig --json vault SUBCOMMAND --home HOME TRAILING...`.
fn jig_vault(
    subcommand: &[&str],
    home: &Path,
    trailing: &[&str],
    passphrase: Option<&str>,
) -> Output {
    // `output()` gives the child null stdin and piped stderr, so Jig cannot
    // open its hidden terminal prompt.
    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    command
        .args(["--json", "vault"])
        .args(subcommand)
        .arg("--home")
        .arg(home)
        .args(trailing)
        .env_remove("JIG_VAULT_PASSPHRASE")
        .env_remove("JIG_VAULT_NEW_PASSPHRASE");
    if let Some(passphrase) = passphrase {
        command.env("JIG_VAULT_PASSPHRASE", passphrase);
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

fn error_message(output: &Output) -> String {
    assert!(!output.status.success());
    let payload = output_json(output);
    assert_eq!(payload["ok"], false);
    payload["error"]["message"].as_str().unwrap().to_owned()
}

fn assert_operator_guidance(message: &str) {
    for marker in OPERATOR_GUIDANCE_MARKERS {
        assert!(message.contains(marker), "{marker}: {message}");
    }
    assert!(!message.contains("export "), "{message}");
}

fn initialized_vault() -> (tempfile::TempDir, PathBuf) {
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
                    "jig://Production/TOKEN".parse::<VaultReference>().unwrap(),
                    FieldKind::Concealed,
                    SecretBytes::new(TOKEN.to_vec()),
                ),
                FieldMutation::set(
                    "jig://Production/FLAG".parse::<VaultReference>().unwrap(),
                    FieldKind::Text,
                    SecretBytes::new(FLAG.to_vec()),
                ),
            ],
        )
        .unwrap();
    (temp, home)
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[test]
fn vault_run_without_terminal_or_passphrase_returns_operator_guidance() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("vault-home");

    let output = jig_vault(
        &["run", "--env", "TOKEN=api_token"],
        &home,
        &["--", "true"],
        None,
    );

    let message = error_message(&output);
    assert!(
        message.contains("cannot prompt for the vault passphrase"),
        "{message}"
    );
    assert!(message.contains("stdin and stderr"), "{message}");
    assert_operator_guidance(&message);
}

#[test]
fn vault_init_without_terminal_requires_operator_chosen_passphrase() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("vault-home");

    let output = jig_vault(&["init"], &home, &[], None);

    let message = error_message(&output);
    assert!(
        message.contains("operator must choose and enter a new vault passphrase"),
        "{message}"
    );
    assert_operator_guidance(&message);
    assert!(!home.join("vault.json").exists());
}

#[test]
fn vault_run_env_and_file_accept_canonical_references() {
    let (_temp, home) = initialized_vault();
    let script = "test \"$TOKEN\" = \"$(cat \"$TOKEN_FILE\")\" && printf 'token=%s flag=%s' \"$TOKEN\" \"$FLAG\"";

    let output = jig_vault(
        &[
            "run",
            "--env",
            "TOKEN=jig://Production/TOKEN",
            "--env",
            "FLAG=jig://Production/FLAG",
            "--file",
            "TOKEN_FILE=jig://Production/TOKEN",
        ],
        &home,
        &["--", "sh", "-c", script],
        Some(PASSPHRASE),
    );

    let payload = output_json(&output);
    assert!(output.status.success(), "{payload:#}");
    assert_eq!(payload["ok"], true);
    assert_eq!(payload["env_mappings"], 2);
    assert_eq!(payload["file_mappings"], 1);
    // The compatible broker redacts every injected value of at least 4 bytes,
    // including text fields.
    assert_eq!(
        payload["result"]["stdout"],
        "token=[REDACTED] flag=[REDACTED]"
    );
    let audit = std::fs::read(home.join("audit.jsonl")).unwrap();
    for secret in [TOKEN, FLAG] {
        assert!(!contains_bytes(&output.stdout, secret));
        assert!(!contains_bytes(&output.stderr, secret));
        assert!(!contains_bytes(&audit, secret));
    }
}

#[test]
fn vault_run_rejects_malformed_reference_before_passphrase_capture() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("vault-home");

    let output = jig_vault(
        &["run", "--env", "TOKEN=jig://Production"],
        &home,
        &["--", "true"],
        None,
    );

    let message = error_message(&output);
    assert!(message.contains("invalid vault reference"), "{message}");
    assert!(message.contains("VAR=jig://ITEM/FIELD"), "{message}");
    assert!(!message.contains(OPERATOR_GUIDANCE_MARKERS[0]), "{message}");
    assert!(!home.exists());
}

#[test]
fn vault_run_reports_missing_canonical_reference_in_reference_form() {
    let (_temp, home) = initialized_vault();

    let output = jig_vault(
        &["run", "--env", "TOKEN=jig://Production/MISSING"],
        &home,
        &["--", "true"],
        Some(PASSPHRASE),
    );

    let message = error_message(&output);
    assert!(
        message.contains("vault field 'jig://Production/MISSING' does not exist"),
        "{message}"
    );
}
