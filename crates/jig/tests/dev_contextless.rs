#![cfg(feature = "dev-proxy")]

mod support;

use std::fs;
use std::process::Command;

use serde_json::Value;

#[test]
fn contextless_commands_ignore_unrelated_malformed_repository() {
    let temp = support::tempdir().unwrap();
    fs::write(temp.path().join(".jig.toml"), "this is not valid TOML [").unwrap();
    let state_dir = temp.path().join("isolated-proxy-state");
    let state = state_dir.to_str().unwrap();

    for (args, expected_command) in [
        (vec!["status", "--all", "--state-dir", state], "dev status"),
        (
            vec![
                "status",
                "--session",
                "dev_example_missing",
                "--state-dir",
                state,
            ],
            "dev status",
        ),
        (
            vec![
                "recover",
                "--session",
                "dev_example_missing",
                "--state-dir",
                state,
            ],
            "dev recover",
        ),
        (
            vec![
                "stop",
                "--session",
                "dev_example_missing",
                "--state-dir",
                state,
            ],
            "dev stop",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_jig"))
            .current_dir(temp.path())
            .env_remove("JIG_REPO_ROOT")
            .env_remove("JIG_PROXY_STATE_DIR")
            .args(["--json", "dev"])
            .args(&args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["command"], expected_command);
        assert_eq!(value["ok"], true);
        assert_eq!(value["state_dir"], state);
    }
    assert!(
        !state_dir.exists(),
        "read-only missing-state commands must not create a registry"
    );
}
