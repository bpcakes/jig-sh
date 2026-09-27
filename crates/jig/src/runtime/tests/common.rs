use super::*;
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use serde_json::json;

use crate::test_env::TestRepoBuilder;

pub(super) fn write_fixture_repo(root: &Path) {
    TestRepoBuilder::new(root)
        .config(
            r#"
[commands]
custom_check_command = "printf 'manifest target ran\n'"

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
"#,
        )
        .required_commands(["custom_check_command"])
        .tool(json!({
            "name": "jig.custom_check",
            "kind": "command",
            "description": "Run configured custom check.",
            "command": "custom_check_command"
        }))
        .write();
    std::fs::write(root.join(".gitignore"), ".agent/.cache/\n.agent/runtime/\n").unwrap();
    write_open_plan(root);
}

pub(super) fn write_command_fixture_repo(root: &Path) {
    TestRepoBuilder::new(root)
        .config(
            r#"
rust_migration_dir = "migrations"
rust_sqlx_metadata_dir = ".sqlx"
schema_dump_command = "printf 'schema dump\n'"
rust_test_command = "printf 'command tool ran\n'"
contract_check_command = "printf 'contract ok\n'"

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": "jig.custom_check",
            "kind": "command",
            "description": "Run configured custom check.",
            "command": "rust_test_command"
        }))
        .write();
    write_open_plan(root);
}

pub(super) fn write_v6_evidence_fixture_repo(root: &Path, gates: &str) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::create_dir_all(root.join("api")).unwrap();
    fs::create_dir_all(root.join("web")).unwrap();
    fs::write(root.join("api/example.go"), "package example\n").unwrap();
    fs::write(
        root.join("web/example.ts"),
        "export const example = true;\n",
    )
    .unwrap();
    fs::write(
        root.join(".jig.toml"),
        format!(
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "ExampleProject"
default_branch = "main"

[commands]
api_test_command = "printf 'api tests passed\n'"
web_test_command = "printf 'web tests passed\n'"

[repository]
default_check_profile = "verify"

[[repository.components]]
id = "api"
root = "api"
adapters = ["go"]

[[repository.components]]
id = "web"
root = "web"
adapters = ["typescript"]

[[repository.actions]]
target = {{ component = "api", action = "test" }}
intent = "check"
effects = ["read_only", "process"]
runner = {{ kind = "command", command = "api_test_command" }}
inputs = ["api/**"]

[[repository.actions]]
target = {{ component = "web", action = "test" }}
intent = "check"
effects = ["read_only", "process"]
runner = {{ kind = "command", command = "web_test_command" }}
inputs = ["web/**"]

[[repository.profiles]]
id = "verify"
targets = [
  {{ component = "api", action = "test" }},
  {{ component = "web", action = "test" }},
]

{gates}
"#
        ),
    )
    .unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 6,
            "tool_namespace": "jig",
            "required_commands": ["api_test_command", "web_test_command"],
            "tools": [],
            "components": [
                {"id": "api", "root": "api", "adapters": ["go"]},
                {"id": "web", "root": "web", "adapters": ["typescript"]}
            ],
            "actions": [
                {
                    "target": {"component": "api", "action": "test"},
                    "intent": "check",
                    "effects": ["read_only", "process"],
                    "runner": {"kind": "command", "command": "api_test_command"},
                    "inputs": ["api/**"]
                },
                {
                    "target": {"component": "web", "action": "test"},
                    "intent": "check",
                    "effects": ["read_only", "process"],
                    "runner": {"kind": "command", "command": "web_test_command"},
                    "inputs": ["web/**"]
                }
            ],
            "profiles": [{
                "id": "verify",
                "targets": [
                    {"component": "api", "action": "test"},
                    {"component": "web", "action": "test"}
                ]
            }],
            "default_check_profile": "verify"
        }))
        .unwrap(),
    )
    .unwrap();
    write_open_plan(root);
}

pub(super) fn write_non_rust_file_budget_fixture_repo(root: &Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::create_dir_all(root.join("web")).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("web/example.ts"),
        "export const example = true;\n",
    )
    .unwrap();
    fs::write(root.join("docs/example.md"), "# Example\n").unwrap();
    fs::write(
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "ExampleProject"
default_branch = "main"

[commands]
web_file_loc_command = "test ! -f web/fail.loc && printf 'web LOC passed\n'"
docs_file_loc_command = "printf 'docs LOC passed\n'"

[repository]
default_check_profile = "quality"

[[repository.components]]
id = "web"
root = "web"
adapters = ["typescript"]

[[repository.components]]
id = "docs"
root = "docs"
adapters = ["markdown"]

[[repository.actions]]
target = { component = "web", action = "file-loc" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "web_file_loc_command" }
inputs = ["web/**"]

[[repository.actions]]
target = { component = "docs", action = "file-loc" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "docs_file_loc_command" }
inputs = ["docs/**"]

[[repository.profiles]]
id = "quality"
targets = [
  { component = "web", action = "file-loc" },
  { component = "docs", action = "file-loc" },
]

[[work.gates]]
id = "web-file-loc"
kind = "evidence"
target = "web:file-loc"
"#,
    )
    .unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 6,
            "tool_namespace": "jig",
            "required_commands": ["web_file_loc_command", "docs_file_loc_command"],
            "tools": [],
            "components": [
                {"id": "web", "root": "web", "adapters": ["typescript"]},
                {"id": "docs", "root": "docs", "adapters": ["markdown"]}
            ],
            "actions": [
                {
                    "target": {"component": "web", "action": "file-loc"},
                    "intent": "check",
                    "effects": ["read_only", "process"],
                    "runner": {"kind": "command", "command": "web_file_loc_command"},
                    "inputs": ["web/**"]
                },
                {
                    "target": {"component": "docs", "action": "file-loc"},
                    "intent": "check",
                    "effects": ["read_only", "process"],
                    "runner": {"kind": "command", "command": "docs_file_loc_command"},
                    "inputs": ["docs/**"]
                }
            ],
            "profiles": [{
                "id": "quality",
                "targets": [
                    {"component": "web", "action": "file-loc"},
                    {"component": "docs", "action": "file-loc"}
                ]
            }],
            "default_check_profile": "quality"
        }))
        .unwrap(),
    )
    .unwrap();
    write_open_plan(root);
}

pub(super) fn write_wide_v6_evidence_fixture_repo(root: &Path, commands: &[String]) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    let mut config = String::from(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "ExampleProject"
default_branch = "main"

[commands]
"#,
    );
    for (index, command) in commands.iter().enumerate() {
        writeln!(
            config,
            "example_{index}_test_command = {}",
            serde_json::to_string(command).unwrap()
        )
        .unwrap();
    }
    config.push_str("\n[repository]\ndefault_check_profile = \"verify\"\n");

    let mut components = Vec::new();
    let mut actions = Vec::new();
    let mut targets = Vec::new();
    let mut required_commands = Vec::new();
    for index in 0..commands.len() {
        let component = format!("example{index}");
        let command = format!("example_{index}_test_command");
        fs::create_dir_all(root.join(&component)).unwrap();
        fs::write(root.join(&component).join("example.txt"), "example\n").unwrap();
        writeln!(
            config,
            "\n[[repository.components]]\nid = \"{component}\"\nroot = \"{component}\""
        )
        .unwrap();
        writeln!(
            config,
            "\n[[repository.actions]]\ntarget = {{ component = \"{component}\", action = \"test\" }}\nintent = \"check\"\neffects = [\"read_only\", \"process\"]\nrunner = {{ kind = \"command\", command = \"{command}\" }}\ninputs = [\"{component}/**\"]"
        )
        .unwrap();
        components.push(json!({"id": component, "root": component}));
        actions.push(json!({
            "target": {"component": component, "action": "test"},
            "intent": "check",
            "effects": ["read_only", "process"],
            "runner": {"kind": "command", "command": command},
            "inputs": [format!("{component}/**")]
        }));
        targets.push(json!({"component": component, "action": "test"}));
        required_commands.push(command);
    }
    config.push_str("\n[[repository.profiles]]\nid = \"verify\"\ntargets = [\n");
    for target in &targets {
        writeln!(
            config,
            "  {{ component = {:?}, action = \"test\" }},",
            target["component"].as_str().unwrap()
        )
        .unwrap();
    }
    config.push_str("]\n");
    fs::write(root.join(".jig.toml"), config).unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 6,
            "tool_namespace": "jig",
            "required_commands": required_commands,
            "tools": [],
            "components": components,
            "actions": actions,
            "profiles": [{"id": "verify", "targets": targets}],
            "default_check_profile": "verify"
        }))
        .unwrap(),
    )
    .unwrap();
    write_open_plan(root);
}

pub(super) fn add_v6_effectful_evidence_actions(root: &Path) {
    let config_path = root.join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "[[repository.profiles]]",
        r#"[[repository.actions]]
target = { component = "api", action = "generate" }
intent = "generate"
effects = ["worktree", "process"]
runner = { kind = "command", command = "api_test_command" }
inputs = ["api/**"]

[[repository.actions]]
target = { component = "api", action = "verify-generated" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "api_test_command" }
inputs = ["api/**"]
depends_on = [{ component = "api", action = "generate" }]

[[repository.profiles]]"#,
    );
    fs::write(config_path, config).unwrap();

    let manifest_path = root.join(".agent/jig-contract.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["actions"].as_array_mut().unwrap().extend([
        json!({
            "target": {"component": "api", "action": "generate"},
            "intent": "generate",
            "effects": ["worktree", "process"],
            "runner": {"kind": "command", "command": "api_test_command"},
            "inputs": ["api/**"]
        }),
        json!({
            "target": {"component": "api", "action": "verify-generated"},
            "intent": "check",
            "effects": ["read_only", "process"],
            "runner": {"kind": "command", "command": "api_test_command"},
            "inputs": ["api/**"],
            "depends_on": [{"component": "api", "action": "generate"}]
        }),
    ]);
    fs::write(
        manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

pub(super) fn write_open_plan(root: &Path) {
    let ctx = RepoContext::load_from(root).unwrap();
    crate::state::seed_open_plan_for_test(&ctx, "plan_1", "Test plan", "# Test plan\n").unwrap();
}

pub(super) fn init_git_repo(root: &Path) {
    run_git(root, &["init"]);
    run_git(root, &["config", "user.email", "fixture@example.com"]);
    run_git(root, &["config", "user.name", "Fixture"]);
    run_git(root, &["add", "."]);
    run_git(root, &["commit", "-m", "initial fixture"]);
}

pub(super) fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

pub(super) fn write_codex_stub(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// Newest-first receipt records for one tool, read directly from the journal.
pub(crate) fn tool_receipts(
    ctx: &RepoContext,
    tool_name: &str,
    failed_only: bool,
) -> Vec<serde_json::Value> {
    let path = ctx.state_file("receipts.jsonl");
    let mut receipts = std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|receipt| receipt["tool_name"] == tool_name)
        .filter(|receipt| !failed_only || receipt["exit_status"] != 0)
        .collect::<Vec<_>>();
    receipts.reverse();
    receipts
}
