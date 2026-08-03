use std::process::Command;

use serde_json::Value;
use tempfile::tempdir;

fn jig() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    command
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD")
        .env("NO_COLOR", "1");
    command
}

#[test]
fn json_mode_wraps_usage_and_pre_output_command_errors() {
    let usage = jig().args(["work", "check", "--json"]).output().unwrap();
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stderr.is_empty());
    let usage: Value = serde_json::from_slice(&usage.stdout).unwrap();
    assert_eq!(usage["ok"], false);
    assert_eq!(usage["error"]["kind"], "usage");
    assert_eq!(usage["exit_status"], 2);

    let repo = tempdir().unwrap();
    let command = jig()
        .current_dir(repo.path())
        .args(["info", "--json"])
        .output()
        .unwrap();
    assert_eq!(command.status.code(), Some(1));
    assert!(command.stderr.is_empty());
    let command: Value = serde_json::from_slice(&command.stdout).unwrap();
    assert_eq!(command["ok"], false);
    assert_eq!(command["error"]["kind"], "command_failed");
    assert_eq!(command["exit_status"], 1);
}

#[test]
fn json_mode_classifies_output_mode_conflicts_as_usage_errors() {
    for args in [
        vec!["--json", "status", "--tui"],
        vec!["status", "--tui", "--json"],
        vec![
            "--json",
            "work",
            "start",
            "--title",
            "test",
            "--print-plan-id",
        ],
        vec![
            "work",
            "--json",
            "start",
            "--title",
            "test",
            "--print-plan-id",
        ],
        vec![
            "work",
            "start",
            "--json",
            "--title",
            "test",
            "--print-plan-id",
        ],
        vec![
            "work",
            "start",
            "--title",
            "test",
            "--print-plan-id",
            "--json",
        ],
    ] {
        let output = jig().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stderr.is_empty());
        let output: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output["ok"], false);
        assert_eq!(output["error"]["kind"], "usage");
        assert_eq!(output["exit_status"], 2);
    }
}

#[test]
fn mcp_parse_errors_keep_stdout_reserved_for_protocol_frames() {
    let output = jig().args(["mcp", "--json", "--bogus"]).output().unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument '--bogus'"));
}

#[test]
fn prompt_get_honors_json_mode() {
    let home = tempdir().unwrap();
    let repo = tempdir().unwrap();
    let added = jig()
        .current_dir(repo.path())
        .env("JIG_PROMPT_HOME", home.path())
        .args(["prompt", "add", "json-test", "Hello {{ name }}"])
        .output()
        .unwrap();
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );

    let output = jig()
        .current_dir(repo.path())
        .env("JIG_PROMPT_HOME", home.path())
        .args([
            "prompt",
            "get",
            "json-test",
            "--var",
            "name=world",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output["ok"], true);
    assert_eq!(output["command"], "prompt get");
    assert_eq!(output["body"], "Hello world");
}
