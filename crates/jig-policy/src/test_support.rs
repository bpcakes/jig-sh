//! Repository fixtures for tests of policy checks, shared with dependents'
//! tests behind the `test-support` feature.

use std::fs;
use std::path::Path;

use jig_commands::tool_defs::{kind, tool};
use jig_context::test_support::TestRepoBuilder;
use serde_json::json;

pub fn write_v6_schema_policy_repo(root: &Path, legacy_command: &str, action_command: &str) {
    fs::create_dir_all(root.join("api")).unwrap();
    fs::write(root.join("api/.keep"), "").unwrap();
    TestRepoBuilder::new(root)
        .contract_version(6)
        .config(format!(
            r#"
schema_dump_enabled = true
schema_dump_command = {legacy_command:?}

[commands]
api_schema_dump_command = {action_command:?}

[repository]
default_check_profile = "verify"

[[repository.components]]
id = "api"
root = "."
adapters = ["sqlx"]

[[repository.actions]]
target = {{ component = "api", action = "schema" }}
intent = "check"
effects = ["read_only", "process"]
runner = {{ kind = "native", operation = "jig.schema_check" }}
legacy_aliases = ["jig.schema_check"]

[[repository.actions]]
target = {{ component = "api", action = "schema-dump" }}
intent = "generate"
effects = ["worktree", "process"]
runner = {{ kind = "command", command = "api_schema_dump_command", working_directory = "api", environment = {{ SCHEMA_VALUE = "changed" }} }}
legacy_aliases = ["jig.schema_dump"]

[[repository.profiles]]
id = "verify"
targets = [{{ component = "api", action = "schema" }}]
"#,
        ))
        .required_commands(["api_schema_dump_command"])
        .tool(json!({
            "name": tool::SCHEMA_CHECK,
            "kind": kind::NATIVE,
            "description": "Check committed schema output for drift."
        }))
        .tool(json!({
            "name": tool::SCHEMA_DUMP,
            "kind": kind::COMMAND,
            "description": "Refresh committed schema output.",
            "command": "api_schema_dump_command"
        }))
        .write();

    let contract_path = root.join(".agent/jig-contract.json");
    let mut contract: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&contract_path).unwrap()).unwrap();
    contract["components"] = json!([{
        "id": "api",
        "root": ".",
        "adapters": ["sqlx"]
    }]);
    contract["actions"] = json!([
        {
            "target": {"component": "api", "action": "schema"},
            "intent": "check",
            "effects": ["read_only", "process"],
            "runner": {"kind": "native", "operation": tool::SCHEMA_CHECK},
            "legacy_aliases": [tool::SCHEMA_CHECK]
        },
        {
            "target": {"component": "api", "action": "schema-dump"},
            "intent": "generate",
            "effects": ["worktree", "process"],
            "runner": {
                "kind": "command",
                "command": "api_schema_dump_command",
                "working_directory": "api",
                "environment": {"SCHEMA_VALUE": "changed"}
            },
            "legacy_aliases": [tool::SCHEMA_DUMP]
        }
    ]);
    contract["profiles"] = json!([{
        "id": "verify",
        "targets": [{"component": "api", "action": "schema"}]
    }]);
    contract["default_check_profile"] = json!("verify");
    fs::write(
        contract_path,
        serde_json::to_string_pretty(&contract).unwrap(),
    )
    .unwrap();
}
