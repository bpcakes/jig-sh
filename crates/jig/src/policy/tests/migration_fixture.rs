use super::*;

pub(super) fn write_v6_mixed_migration_policy_repo(root: &Path, owner: &str) {
    TestRepoBuilder::new(root)
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
        .config(format!(
            r#"
migration_dir = "database/migrations"

[repository]
default_check_profile = "operate"

[[repository.components]]
id = "api"
root = "services/api"
adapters = ["go", "go-postgres"]

[[repository.components]]
id = "worker"
root = "services/worker"
adapters = ["rust", "sqlx"]

[[repository.actions]]
target = {{ component = "{owner}", action = "migration-add" }}
intent = "generate"
effects = ["worktree", "process"]
runner = {{ kind = "native", operation = "jig.migration_add" }}
arguments = {{ name = {{ type = "string", required = true, max_bytes = 200 }} }}
inputs = ["database/migrations/**"]
legacy_aliases = ["jig.migration_add"]

[[repository.profiles]]
id = "operate"
targets = [{{ component = "{owner}", action = "migration-add" }}]
"#,
        ))
        .required_commands(std::iter::empty::<&str>())
        .tool(json!({
            "name": tool::MIGRATION_ADD,
            "kind": kind::NATIVE,
            "description": "Create a migration."
        }))
        .write();
    let contract_path = root.join(".agent/jig-contract.json");
    let mut contract: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&contract_path).unwrap()).unwrap();
    contract["components"] = json!([
        {"id": "api", "root": "services/api", "adapters": ["go", "go-postgres"]},
        {"id": "worker", "root": "services/worker", "adapters": ["rust", "sqlx"]}
    ]);
    contract["actions"] = json!([{
        "target": {"component": owner, "action": "migration-add"},
        "intent": "generate",
        "effects": ["worktree", "process"],
        "runner": {"kind": "native", "operation": "jig.migration_add"},
        "arguments": {"name": {"type": "string", "required": true, "max_bytes": 200}},
        "inputs": ["database/migrations/**"],
        "legacy_aliases": ["jig.migration_add"]
    }]);
    contract["profiles"] = json!([{
        "id": "operate",
        "targets": [{"component": owner, "action": "migration-add"}]
    }]);
    contract["default_check_profile"] = json!("operate");
    fs::write(
        contract_path,
        serde_json::to_string_pretty(&contract).unwrap(),
    )
    .unwrap();
}
