
fn write_doctor_fixture(root: &Path) {
    fs::create_dir_all(root.join("scripts")).unwrap();
    TestRepoBuilder::new(root)
        .jig_version(env!("CARGO_PKG_VERSION"))
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
        .config(
            r#"
bootstrap_command = "printf bootstrap"

[repository]
default_check_profile = "verify"

[[repository.components]]
id = "repo"
root = "."
adapters = ["rust"]

[[repository.actions]]
target = { component = "repo", action = "bootstrap" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "bootstrap_command" }
inputs = ["**"]

[[repository.profiles]]
id = "verify"
targets = [{ component = "repo", action = "bootstrap" }]

[agent_tooling.codex]
marketplaces = []
"#,
        )
        .required_commands(["bootstrap_command"])
        .tool(json!({
            "name": tool::CONTRACT_CHECK,
            "kind": "native",
            "description": "Contract check."
        }))
        .tool(json!({
            "name": tool::BOOTSTRAP,
            "kind": "command",
            "description": "Bootstrap.",
            "command": "bootstrap_command"
        }))
        .write();
    let contract_path = root.join(".agent/jig-contract.json");
    let mut contract: Value =
        serde_json::from_str(&fs::read_to_string(&contract_path).unwrap()).unwrap();
    contract["components"] = json!([{
        "id": "repo",
        "root": ".",
        "adapters": ["rust"]
    }]);
    contract["actions"] = json!([{
        "target": {"component": "repo", "action": "bootstrap"},
        "intent": "check",
        "effects": ["read_only", "process"],
        "runner": {"kind": "command", "command": "bootstrap_command"},
        "inputs": ["**"]
    }]);
    contract["profiles"] = json!([{
        "id": "verify",
        "targets": [{"component": "repo", "action": "bootstrap"}]
    }]);
    contract["default_check_profile"] = json!("verify");
    fs::write(
        &contract_path,
        serde_json::to_string_pretty(&contract).unwrap(),
    )
    .unwrap();
    fs::write(root.join(".mcp.json"), "{}").unwrap();
    fs::write(
        root.join("scripts/install-jig.sh"),
        CURRENT_GENERATED_INSTALLER,
    )
    .unwrap();
    fs::write(root.join("scripts/jig"), current_generated_launcher()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.join("scripts/jig"), fs::Permissions::from_mode(0o755)).unwrap();
    }
}
