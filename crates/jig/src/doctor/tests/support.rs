use super::*;

#[cfg(unix)]
use std::ffi::OsString;
use std::path::Path;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::time::Duration;
use std::{env, fs};

use jig_commands::tool_defs::tool;
use serde_json::{Value, json};

#[cfg(unix)]
use crate::doctor::check::DoctorCheck;
#[cfg(unix)]
use crate::doctor::environment::DoctorEnvironment;
use crate::test_env::TestRepoBuilder;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::test_process::{TestProcessIdentity, publish_test_process_identity};

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) const OWNED_PROCESS_DESCENDANT_MARKER_ENV: &str =
    "JIG_DOCTOR_OWNED_PROCESS_DESCENDANT_MARKER";
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn shell_quote_test_path(path: &Path) -> String {
    let path = path
        .to_str()
        .expect("test helper paths must be representable in shell fixtures");
    format!("'{}'", path.replace('\'', "'\"'\"'"))
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn owned_test_descendant_script(marker: &Path, tail: &str) -> String {
    format!(
        "#!/bin/sh\n{marker_env}={marker} {test_exe} --exact doctor::tests::support::owned_process_descendant_helper --nocapture &\nwhile [ ! -f {marker} ]; do :; done\n{tail}\n",
        marker_env = OWNED_PROCESS_DESCENDANT_MARKER_ENV,
        marker = shell_quote_test_path(marker),
        test_exe = shell_quote_test_path(&std::env::current_exe().unwrap()),
    )
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn owned_process_descendant_helper() {
    let Some(marker) = std::env::var_os(OWNED_PROCESS_DESCENDANT_MARKER_ENV) else {
        return;
    };
    let identity = TestProcessIdentity::capture_current().expect("capture test helper identity");
    publish_test_process_identity(Path::new(&marker), &identity);
    std::thread::sleep(Duration::from_secs(30));
}
#[cfg(unix)]
pub(super) fn cargo_sqlx_program(check: &DoctorCheck) -> &Value {
    check.data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|tool| tool["programs"].as_array().unwrap())
        .find(|program| program.get("driver_probe").is_some())
        .unwrap()
}
#[cfg(unix)]
pub(super) fn doctor_environment(bin: &Path, database_url: Option<&str>) -> DoctorEnvironment {
    let bin = fs::canonicalize(bin).unwrap_or_else(|_| bin.to_path_buf());
    DoctorEnvironment {
        search_path: Some(bin.into_os_string()),
        database_url: database_url.map(OsString::from),
        cargo_alias_sqlx: None,
        cargo_home: None,
        home: None,
        probe_environment: Vec::new(),
        shell_environment_issue: None,
    }
}
#[cfg(unix)]
pub(super) fn write_test_executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
#[cfg(unix)]
pub(super) fn write_workspace_version_manifest(root: &Path, rust: &str, sqlx: Option<&str>) {
    let sqlx = sqlx
        .map(|version| format!("\n[workspace.dependencies]\nsqlx = {{ version = {version:?} }}\n"))
        .unwrap_or_default();
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[workspace]\nmembers = []\n\n[workspace.package]\nrust-version = {rust:?}\n{sqlx}"
        ),
    )
    .unwrap();
}
pub(super) fn write_sqlx_doctor_fixture_with_command(root: &Path, command: &str) {
    write_doctor_fixture(root);
    let config_path = root.join(".jig.toml");
    let sqlx_config = format!(
        "sqlx_enabled = true\nrust_crate_roots = [\"crates\"]\nrust_migration_dir = \"migrations\"\nrust_sqlx_metadata_dir = \".sqlx\"\nschema_dump_enabled = false\nsqlx_check_command = {command:?}\n\n[repository]"
    );
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace("adapters = [\"rust\"]", "adapters = [\"rust\", \"sqlx\"]")
        .replace("[repository]", &sqlx_config);
    fs::write(config_path, config).unwrap();
    fs::create_dir(root.join("migrations")).unwrap();

    let contract_path = root.join(".agent/jig-contract.json");
    let mut contract: Value =
        serde_json::from_str(&fs::read_to_string(&contract_path).unwrap()).unwrap();
    contract["components"][0]["adapters"] = json!(["rust", "sqlx"]);
    contract["required_commands"]
        .as_array_mut()
        .unwrap()
        .push(json!("sqlx_check_command"));
    let tools = contract["tools"].as_array_mut().unwrap();
    tools.push(json!({
        "name": tool::SQLX_CHECK,
        "kind": "command",
        "description": "Run the configured SQLx check command.",
        "command": "sqlx_check_command",
    }));
    tools.push(json!({
        "name": tool::MIGRATION_ADD,
        "kind": "native",
        "description": "Add timestamped SQL migration stubs.",
    }));
    fs::write(
        contract_path,
        serde_json::to_string_pretty(&contract).unwrap(),
    )
    .unwrap();
}
pub(super) fn write_doctor_fixture_with_bootstrap_command(root: &Path, command: &str) {
    write_doctor_fixture(root);
    let config_path = root.join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "bootstrap_command = \"printf bootstrap\"",
        &format!("bootstrap_command = {command:?}"),
    );
    fs::write(config_path, config).unwrap();
}
pub(super) fn write_doctor_fixture(root: &Path) {
    fs::create_dir_all(root.join("scripts")).unwrap();
    TestRepoBuilder::new(root)
        .jig_version(env!("CARGO_PKG_VERSION"))
        .contract_version(jig_context::CURRENT_CONTRACT_VERSION)
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
runner = { kind = "shell", command = "bootstrap_command" }
inputs = ["**"]
legacy_aliases = ["jig.bootstrap"]

[[repository.actions]]
target = { component = "repo", action = "contract" }
intent = "check"
effects = ["read_only"]
runner = { kind = "native", operation = "jig.contract_check" }
inputs = [".jig.toml"]
legacy_aliases = ["jig.contract_check"]

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
    contract["actions"] = json!([
        {
            "target": {"component": "repo", "action": "bootstrap"},
            "intent": "check",
            "effects": ["read_only", "process"],
            "runner": {"kind": "shell", "command": "bootstrap_command"},
            "inputs": ["**"],
            "legacy_aliases": ["jig.bootstrap"]
        },
        {
            "target": {"component": "repo", "action": "contract"},
            "intent": "check",
            "effects": ["read_only"],
            "runner": {"kind": "native", "operation": "jig.contract_check"},
            "inputs": [".jig.toml"],
            "legacy_aliases": ["jig.contract_check"]
        }
    ]);
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

pub(super) fn check_by_id<'a>(output: &'a Value, id: &str) -> &'a Value {
    output["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == id)
        .unwrap()
}
