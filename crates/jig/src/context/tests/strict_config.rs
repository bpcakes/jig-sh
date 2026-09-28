use super::*;

#[test]
fn unknown_dev_app_config_fields_are_rejected() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[dev.apps]]
name = "web"
command = "bun run dev"
commmand = "typo"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["contract_check_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = format!("{:#}", RepoContext::load_from(temp.path()).unwrap_err());
    assert!(error.contains("unknown field"));
    assert!(error.contains("commmand"));
}

#[test]
fn unknown_top_level_config_fields_are_rejected() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
proxy_porrt = 1355
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["contract_check_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = format!("{:#}", RepoContext::load_from(temp.path()).unwrap_err());
    assert!(error.contains("unknown field"));
    assert!(error.contains("proxy_porrt"));
}

#[test]
fn configured_work_tracker_is_available_from_the_repo_context() {
    use crate::context::work_config::WorkTrackerExport;

    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
[work.tracker]
kind = "beads"
workspace_id = "01ARZ3NDEKTSV4RRFFQ69G5FAV"
manual_export_guidance = "Run the ExampleProject export helper."
"#,
        )
        .write();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let tracker = ctx.work_tracker().unwrap();
    assert_eq!(tracker.kind(), "beads");
    assert_eq!(tracker.workspace_id(), "01ARZ3NDEKTSV4RRFFQ69G5FAV");
    assert_eq!(tracker.root(), ".beads");
    assert_eq!(tracker.export(), WorkTrackerExport::Manual);
    assert_eq!(
        tracker.manual_export_guidance(),
        Some("Run the ExampleProject export helper.")
    );
}

#[test]
fn invalid_work_tracker_configuration_is_rejected_during_context_load() {
    for tracker in [
        r#"kind = "beads"
workspace_id = "01arz3ndektsv4rrffq69g5fav""#,
        r#"kind = "beads"
workspace_id = "01ARZ3NDEKTSV4RRFFQ69G5FAV"
manual_export_guidance = "  ""#,
        r#"kind = "other"
workspace_id = "01ARZ3NDEKTSV4RRFFQ69G5FAV""#,
    ] {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .config(format!("[work.tracker]\n{tracker}\n"))
            .write();

        assert!(RepoContext::load_from(temp.path()).is_err(), "{tracker}");
    }
}

#[test]
fn legacy_work_checks_are_merged_with_explicit_gates() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[work]
checks = ["jig.contract_check", "jig.test"]

[[work.gates]]
id = "contract"
kind = "check"
tool = "jig.contract_check"
required = false
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["contract_check_command", "rust_test_command"],
            "tools": [
                {
                    "name": "jig.contract_check",
                    "kind": "command",
                    "description": "Run contract check.",
                    "command": "contract_check_command"
                },
                {
                    "name": "jig.test",
                    "kind": "command",
                    "description": "Run tests.",
                    "command": "rust_test_command"
                }
            ],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let gates = ctx.work_gates();
    assert_eq!(gates.len(), 2);
    let WorkGate::Check(gate) = &gates[0] else {
        panic!("expected check gate");
    };
    assert_eq!(gate.id, "contract");
    assert_eq!(gate.tool, "jig.contract_check");
    assert!(!gate.required);
    let WorkGate::Check(gate) = &gates[1] else {
        panic!("expected check gate");
    };
    assert_eq!(gate.id, "test");
    assert_eq!(gate.tool, "jig.test");
    assert!(gate.required);
}

#[test]
fn retired_work_refinements_still_load_without_validation() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[work.refinements]]
id = "rust-simplify"

[[work.refinements]]
id = "rust-security"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": [],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    RepoContext::load_from(temp.path()).unwrap();
}
