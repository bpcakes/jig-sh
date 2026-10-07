use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use jig_context::test_support::TestRepoBuilder;
use serde_json::{Value, json};
use tempfile::tempdir;

const COMMAND_OUTPUT: &str = "SQLx fixture command ran";

fn jig(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jig"))
        .current_dir(root)
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD")
        .env("NO_COLOR", "1")
        .args(args)
        .output()
        .unwrap()
}

fn write_repository(root: &Path, version: u32, action_name: &str) {
    let command = format!("printf '{COMMAND_OUTPUT}\\n'");
    let tool = json!({
        "name": "jig.sqlx_check", "kind": "command",
        "description": "SQLx fixture check.", "command": "sqlx_check_command"
    });
    let config = format!(
        "sqlx_enabled = true\nmigration_dir = \"database/changes\"\nsqlx_check_command = {command:?}"
    );
    let builder = TestRepoBuilder::new(root)
        .repo_name("ExampleProject")
        .contract_version(version)
        .required_commands(["sqlx_check_command"])
        .tool(tool);
    if version < 6 {
        builder.config(config).write();
    } else {
        builder
            .config(format!(
                r#"{config}
[commands]
sqlx_check_command = {command:?}
[repository]
default_check_profile = "verify"
[[repository.components]]
id = "api"
root = "."
adapters = ["sqlx"]
[[repository.actions]]
target = {{ component = "api", action = "{action_name}" }}
intent = "check"
effects = ["read_only", "process"]
runner = {{ kind = "command", command = "sqlx_check_command" }}
inputs = ["database/changes/**"]
legacy_aliases = ["jig.sqlx_check"]
[[repository.profiles]]
id = "verify"
targets = [{{ component = "api", action = "{action_name}" }}]
"#
            ))
            .write();
        let contract_path = root.join(".agent/jig-contract.json");
        let mut contract: Value =
            serde_json::from_slice(&fs::read(&contract_path).unwrap()).unwrap();
        contract["components"] = json!([{"id": "api", "root": ".", "adapters": ["sqlx"]}]);
        contract["actions"] = json!([{
            "target": {"component": "api", "action": action_name},
            "intent": "check", "effects": ["read_only", "process"],
            "runner": {"kind": "command", "command": "sqlx_check_command"},
            "inputs": ["database/changes/**"], "legacy_aliases": ["jig.sqlx_check"]
        }]);
        contract["profiles"] = json!([{
            "id": "verify", "targets": [{"component": "api", "action": action_name}]
        }]);
        contract["default_check_profile"] = json!("verify");
        fs::write(contract_path, serde_json::to_vec(&contract).unwrap()).unwrap();
    }
    fs::create_dir_all(root.join("database/changes")).unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.email", "fixture@example.invalid"],
        &["config", "user.name", "Fixture"],
        &["add", "."],
        &["commit", "-qm", "baseline"],
    ] {
        assert!(
            Command::new("git")
                .current_dir(root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
}

#[test]
fn sqlx_checks_reject_numeric_duplicates_before_the_configured_command() {
    for version in [3, 6] {
        let repo = tempdir().unwrap();
        write_repository(repo.path(), version, "sqlx");
        fs::write(
            repo.path().join("database/changes/1_first.sql"),
            "SELECT 1;",
        )
        .unwrap();
        fs::write(
            repo.path().join("database/changes/01_second.sql"),
            "SELECT 2;",
        )
        .unwrap();
        let mut invocations = vec![vec!["check", "sqlx"]];
        if version >= 6 {
            invocations.extend([
                vec!["check", "api:sqlx"],
                vec!["check", "--profile", "verify"],
                vec!["run", "api:sqlx"],
            ]);
        }
        for args in invocations {
            let output = jig(repo.path(), &args);
            assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                text.contains("Duplicate SQLx migration version 1"),
                "{args:?}: {text}"
            );
            assert!(text.contains("database/changes/01_second.sql"), "{text}");
            assert!(text.contains("database/changes/1_first.sql"), "{text}");
            assert!(!text.contains(COMMAND_OUTPUT), "{text}");
        }
        let output = jig(repo.path(), &["check", "sqlx", "--json"]);
        assert_eq!(output.status.code(), Some(1));
        let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(payload["ok"], false);
        assert!(
            payload
                .to_string()
                .contains("Duplicate SQLx migration version 1")
        );
        assert!(!payload.to_string().contains(COMMAND_OUTPUT));
    }
}

#[test]
fn reversible_pairs_pass_without_combining_independent_directories() {
    let repo = tempdir().unwrap();
    write_repository(repo.path(), 6, "sqlx");
    for path in [
        "database/changes/1_pair.up.sql",
        "database/changes/01_pair.down.sql",
        "database/changes/child/1_other.sql",
        "database/changes/child/01_other.sql",
        "other/migrations/1_other.sql",
        "other/migrations/01_other.sql",
    ] {
        fs::create_dir_all(repo.path().join(path).parent().unwrap()).unwrap();
        fs::write(repo.path().join(path), "SELECT 1;").unwrap();
    }
    let output = jig(repo.path(), &["check", "sqlx", "--json"]);
    assert!(output.status.success(), "{output:?}");
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["ok"], true);
    assert!(payload.to_string().contains(COMMAND_OUTPUT));
}

#[test]
fn migration_policy_ci_check_reports_added_duplicates_as_json_violations() {
    let repo = tempdir().unwrap();
    write_repository(repo.path(), 6, "sqlx");
    for name in ["1_first.sql", "01_second.sql"] {
        fs::write(repo.path().join("database/changes").join(name), "SELECT 1;").unwrap();
    }
    let output = jig(
        repo.path(),
        &[
            "check",
            "migration-immutability",
            "--changed-against",
            "HEAD",
            "--json",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["ok"], false);
    assert_eq!(payload["violations"].as_array().unwrap().len(), 1);
    assert!(
        payload["violations"][0]
            .as_str()
            .unwrap()
            .contains("Duplicate SQLx migration version 1")
    );
}

#[test]
fn custom_action_ids_with_the_sqlx_alias_cannot_bypass_the_preflight() {
    let repo = tempdir().unwrap();
    write_repository(repo.path(), 6, "verify-metadata");
    for name in ["1_first.sql", "01_second.sql"] {
        fs::write(repo.path().join("database/changes").join(name), "SELECT 1;").unwrap();
    }
    let output = jig(repo.path(), &["check", "api:verify-metadata", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["ok"], false);
    assert_eq!(payload["run"]["targets"][0]["conclusion"], "failure");
    assert_eq!(
        payload["run"]["targets"][0]["findings"][0]["source"],
        "sqlx.migration_versions"
    );
    assert!(!payload.to_string().contains(COMMAND_OUTPUT));
}

#[test]
fn literal_argv_sqlx_targets_share_the_migration_preflight() {
    let repo = tempdir().unwrap();
    write_repository(repo.path(), 6, "sqlx");
    let config_path = repo.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "runner = { kind = \"command\", command = \"sqlx_check_command\" }",
        "runner = { kind = \"argv\", program = \"printf\", args = [\"SQLx fixture command ran\\n\"] }",
    );
    fs::write(config_path, config).unwrap();
    let contract_path = repo.path().join(".agent/jig-contract.json");
    let mut contract: Value = serde_json::from_slice(&fs::read(&contract_path).unwrap()).unwrap();
    contract["contract_version"] = json!(8);
    contract["actions"][0]["runner"] = json!({
        "kind": "argv", "program": "printf", "args": ["SQLx fixture command ran\n"]
    });
    fs::write(contract_path, serde_json::to_vec(&contract).unwrap()).unwrap();

    let clean = jig(repo.path(), &["check", "api:sqlx", "--json"]);
    assert!(clean.status.success(), "{clean:?}");
    let payload: Value = serde_json::from_slice(&clean.stdout).unwrap();
    assert_eq!(
        payload["results"][0]["response"]["result"]["stdout"],
        format!("{COMMAND_OUTPUT}\n")
    );

    for name in ["1_first.sql", "01_second.sql"] {
        fs::write(repo.path().join("database/changes").join(name), "SELECT 1;").unwrap();
    }
    let duplicate = jig(repo.path(), &["check", "api:sqlx", "--json"]);
    assert_eq!(duplicate.status.code(), Some(1));
    let payload: Value = serde_json::from_slice(&duplicate.stdout).unwrap();
    assert_eq!(payload["ok"], false);
    assert!(
        payload
            .to_string()
            .contains("Duplicate SQLx migration version 1")
    );
    assert_eq!(payload["results"][0]["response"]["result"]["stdout"], "");
}
