use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

use jig_ui::dashboard::{PLAN_ROOT_FIELDS, RECORDER_ROOT_FIELDS, STATUS_ROOT_FIELDS};

mod support;

fn fixture() -> tempfile::TempDir {
    let root = support::tempdir().unwrap();
    fs::write(
        root.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "ExampleProject"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[commands]
contract_check_command = "true"
"#,
    )
    .unwrap();
    fs::create_dir(root.path().join(".agent")).unwrap();
    fs::write(
        root.path().join(".agent/jig-contract.json"),
        r#"{
  "contract_version": 3,
  "tool_namespace": "jig",
  "jig_version": "0.2.0-beta.1",
  "required_commands": ["contract_check_command"],
  "tools": [{
    "name": "jig.contract_check",
    "kind": "command",
    "description": "Validate fixture contract.",
    "command": "contract_check_command"
  }]
}
"#,
    )
    .unwrap();
    for args in [
        &["init", "-q"][..],
        &["checkout", "-q", "-b", "main"][..],
        &["config", "user.email", "fixture@example.com"][..],
        &["config", "user.name", "Fixture"][..],
        &["add", "."][..],
        &["commit", "-q", "-m", "fixture"][..],
    ] {
        assert!(
            Command::new("git")
                .current_dir(root.path())
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    root
}

fn jig(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jig"))
        .current_dir(root)
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD")
        .args(args)
        .output()
        .unwrap()
}

fn success_json(output: Output) -> Value {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn assert_exact_root_fields(value: &Value, expected: &[&str]) {
    let actual = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected.iter().copied().collect::<BTreeSet<_>>());
}

#[test]
fn recorder_plan_and_status_json_entrypoints_remain_portable_after_deletion() {
    let root = fixture();

    let recorder = success_json(jig(root.path(), &["ui", "--json"]));
    assert_eq!(recorder["command"], "ui");
    assert_eq!(recorder["snapshot_kind"], "recorder");
    assert_exact_root_fields(&recorder, RECORDER_ROOT_FIELDS);

    let started = jig(
        root.path(),
        &[
            "work",
            "start",
            "--title",
            "Example plan",
            "--body",
            "# Example plan",
            "--print-plan-id",
        ],
    );
    assert!(started.status.success());
    let plan_id = String::from_utf8(started.stdout).unwrap();
    let plan = success_json(jig(
        root.path(),
        &["ui", "--plan", plan_id.trim(), "--json"],
    ));
    assert_eq!(plan["command"], "ui");
    assert_eq!(plan["snapshot_kind"], "plan");
    assert_eq!(plan["plan"]["plan_id"], plan_id.trim());
    assert_exact_root_fields(&plan, PLAN_ROOT_FIELDS);

    let status = success_json(jig(root.path(), &["status", "--json"]));
    assert_eq!(status["command"], "status");
    assert_eq!(status["schema_version"], 1);
    assert_eq!(status["outcome"], "complete");
    assert_eq!(status["repository"]["name"], "ExampleProject");
    assert_eq!(status["repository"]["branch"], "main");
    assert!(status["repository"]["head_revision"].as_str().is_some());
    assert_exact_root_fields(&status, STATUS_ROOT_FIELDS);
}
