#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Output};

const OPERATOR_GUIDANCE_MARKERS: [&str; 4] = [
    "run the exact command in a terminal",
    "automation outside the agent session",
    "must never request, print, store, or choose a vault passphrase",
    "must not set JIG_VAULT_PASSPHRASE or JIG_VAULT_NEW_PASSPHRASE themselves",
];

/// Runs `jig --json vault SUBCOMMAND --home HOME TRAILING...`.
fn jig_without_passphrase(subcommand: &[&str], home: &Path, trailing: &[&str]) -> Output {
    // `output()` gives the child null stdin and piped stderr, so Jig cannot
    // open its hidden terminal prompt.
    Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["--json", "vault"])
        .args(subcommand)
        .arg("--home")
        .arg(home)
        .args(trailing)
        .env_remove("JIG_VAULT_PASSPHRASE")
        .env_remove("JIG_VAULT_NEW_PASSPHRASE")
        .output()
        .unwrap()
}

fn error_message(output: &Output) -> String {
    assert!(!output.status.success());
    let payload: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "invalid JSON ({error}); stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
    assert_eq!(payload["ok"], false);
    payload["error"]["message"].as_str().unwrap().to_owned()
}

fn assert_operator_guidance(message: &str) {
    for marker in OPERATOR_GUIDANCE_MARKERS {
        assert!(message.contains(marker), "{marker}: {message}");
    }
    assert!(!message.contains("export "), "{message}");
}

#[test]
fn vault_run_without_terminal_or_passphrase_returns_operator_guidance() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("vault-home");

    let output =
        jig_without_passphrase(&["run", "--env", "TOKEN=api_token"], &home, &["--", "true"]);

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

    let output = jig_without_passphrase(&["init"], &home, &[]);

    let message = error_message(&output);
    assert!(
        message.contains("operator must choose and enter a new vault passphrase"),
        "{message}"
    );
    assert_operator_guidance(&message);
    assert!(!home.join("vault.json").exists());
}
