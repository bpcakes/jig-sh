use std::path::Path;

use serde_json::json;
use tempfile::tempdir;

use super::runtime::{
    MIN_SUPPORTED_CONTRACT_VERSION, active_contract_versions_label_at,
    is_active_contract_version_at, supported_contract_versions_label,
};
use super::*;

fn write_native_contract_fixture(root: &Path, contract_version: u32) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"

[repository]
default_check_profile = "verify"

[[repository.components]]
id = "repo"
root = "."
adapters = ["jig"]

[[repository.actions]]
target = { component = "repo", action = "contract" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "native", operation = "jig.contract_check" }

[[repository.profiles]]
id = "verify"
targets = [{ component = "repo", action = "contract" }]
"#,
    )
    .unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": contract_version,
            "tool_namespace": "jig",
            "required_commands": [],
            "tools": [],
            "components": [{"id": "repo", "root": ".", "adapters": ["jig"]}],
            "actions": [{
                "target": {"component": "repo", "action": "contract"},
                "intent": "check",
                "effects": ["read_only", "process"],
                "runner": {"kind": "native", "operation": "jig.contract_check"}
            }],
            "profiles": [{
                "id": "verify",
                "targets": [{"component": "repo", "action": "contract"}]
            }],
            "default_check_profile": "verify"
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn tracker_journal_epoch_is_supported_without_activating_or_reusing_reserved_epochs() {
    assert_eq!(CURRENT_CONTRACT_VERSION, 8);
    assert_eq!(TRACKER_JOURNAL_CONTRACT_VERSION, 11);
    assert_eq!(MAX_SUPPORTED_CONTRACT_VERSION, 11);

    for version in MIN_SUPPORTED_CONTRACT_VERSION..=CURRENT_CONTRACT_VERSION {
        assert!(is_supported_contract_version(version));
        assert!(!supports_tracker_journals(version));
    }
    for version in [9, 10] {
        assert!(!is_supported_contract_version(version));
        assert!(!supports_tracker_journals(version));
    }
    assert!(is_supported_contract_version(
        TRACKER_JOURNAL_CONTRACT_VERSION
    ));
    assert!(!is_active_contract_version(
        TRACKER_JOURNAL_CONTRACT_VERSION
    ));
    assert!(supports_tracker_journals(TRACKER_JOURNAL_CONTRACT_VERSION));
    assert_eq!(supported_contract_versions_label(), "2 through 8 and 11");
    assert_eq!(active_contract_versions_label(), "2 through 8");

    assert!(is_active_contract_version_at(
        TRACKER_JOURNAL_CONTRACT_VERSION,
        TRACKER_JOURNAL_CONTRACT_VERSION
    ));
    for version in [9, 10] {
        assert!(!is_active_contract_version_at(
            version,
            TRACKER_JOURNAL_CONTRACT_VERSION
        ));
    }
    assert_eq!(
        active_contract_versions_label_at(TRACKER_JOURNAL_CONTRACT_VERSION),
        "2 through 8 and 11"
    );
}

#[test]
fn repository_loading_accepts_tracker_journal_epoch_and_rejects_reserved_epochs() {
    let supported = tempdir().unwrap();
    write_native_contract_fixture(supported.path(), TRACKER_JOURNAL_CONTRACT_VERSION);
    let context = RepoContext::load_from_root(supported.path().to_path_buf()).unwrap();
    assert_eq!(context.contract_version(), TRACKER_JOURNAL_CONTRACT_VERSION);

    for reserved in [9, 10] {
        let rejected = tempdir().unwrap();
        write_native_contract_fixture(rejected.path(), reserved);
        let error = RepoContext::load_from_root(rejected.path().to_path_buf())
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            format!("Unsupported jig contract version: {reserved}")
        );
    }
}

#[test]
fn legacy_contract_versions_two_through_five_remain_supported() {
    for version in MIN_SUPPORTED_CONTRACT_VERSION..=5 {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".agent")).unwrap();
        fs::write(
            temp.path().join(".jig.toml"),
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
bootstrap_command = "cargo fetch"
"#,
        )
        .unwrap();

        let mut manifest = json!({
            "contract_version": version,
            "tool_namespace": "jig",
            "required_commands": ["bootstrap_command"],
            "tools": [],
        });
        if version <= LAST_VERSION_LOCKED_CONTRACT_VERSION {
            manifest["jig_version"] = json!("0.2.0-beta.1");
            let path = temp.path().join(".jig.toml");
            let config = fs::read_to_string(&path).unwrap();
            fs::write(
                path,
                config.replace(
                    "bootstrap_command = \"cargo fetch\"",
                    "jig_version = \"0.2.0-beta.1\"\nbootstrap_command = \"cargo fetch\"",
                ),
            )
            .unwrap();
        }
        fs::write(
            temp.path().join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        assert_eq!(ctx.contract_version(), version);
    }

    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
bootstrap_command = "cargo fetch"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["bootstrap_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = RepoContext::load_from_root(temp.path().to_path_buf())
        .unwrap_err()
        .to_string();

    assert!(error.contains("Unsupported jig contract version: 1"));

    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
bootstrap_command = "cargo fetch"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": MAX_SUPPORTED_CONTRACT_VERSION + 1,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["bootstrap_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = RepoContext::load_from_root(temp.path().to_path_buf())
        .unwrap_err()
        .to_string();

    assert!(error.contains(&format!(
        "Unsupported jig contract version: {}",
        MAX_SUPPORTED_CONTRACT_VERSION + 1
    )));
}

fn write_v6_repository_fixture(root: &Path, manifest_root: &str) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"

[commands]
api_test_command = "go test ./..."

[repository]
default_check_profile = "verify"

[[repository.components]]
id = "api"
root = "."
adapters = ["go"]

[[repository.actions]]
target = { component = "api", action = "test" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "api_test_command" }

[[repository.profiles]]
id = "verify"
targets = [{ component = "api", action = "test" }]
"#,
    )
    .unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 6,
            "tool_namespace": "jig",
            "required_commands": ["api_test_command"],
            "tools": [],
            "components": [{"id": "api", "root": manifest_root, "adapters": ["go"]}],
            "actions": [{
                "target": {"component": "api", "action": "test"},
                "intent": "check",
                "effects": ["read_only", "process"],
                "runner": {"kind": "command", "command": "api_test_command"}
            }],
            "profiles": [{
                "id": "verify",
                "targets": [{"component": "api", "action": "test"}]
            }],
            "default_check_profile": "verify"
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn contract_six_loads_adapter_identity_without_backend_language() {
    let temp = tempdir().unwrap();
    write_v6_repository_fixture(temp.path(), ".");

    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    assert_eq!(ctx.contract_version(), 6);
    assert!(ctx.is_go_backend());
    assert!(!ctx.sqlx_enabled());
}

#[test]
fn contract_six_rejects_authored_and_resolved_repository_drift() {
    let temp = tempdir().unwrap();
    write_v6_repository_fixture(temp.path(), "cmd/api");

    let error = RepoContext::load_from_root(temp.path().to_path_buf())
        .unwrap_err()
        .to_string();

    assert!(error.contains("repository components differ"));
    assert!(error.contains("component 'api' (index 0)"), "{error}");
    assert!(error.contains("jig update --recopy"), "{error}");
}

#[test]
fn contract_six_accepts_a_native_only_repository() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"

[repository]
default_check_profile = "verify"

[[repository.components]]
id = "repo"
root = "."
adapters = ["jig"]

[[repository.actions]]
target = { component = "repo", action = "contract" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "native", operation = "jig.contract_check" }

[[repository.profiles]]
id = "verify"
targets = [{ component = "repo", action = "contract" }]
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 6,
            "tool_namespace": "jig",
            "required_commands": [],
            "tools": [],
            "components": [{"id": "repo", "root": ".", "adapters": ["jig"]}],
            "actions": [{
                "target": {"component": "repo", "action": "contract"},
                "intent": "check",
                "effects": ["read_only", "process"],
                "runner": {"kind": "native", "operation": "jig.contract_check"}
            }],
            "profiles": [{
                "id": "verify",
                "targets": [{"component": "repo", "action": "contract"}]
            }],
            "default_check_profile": "verify"
        }))
        .unwrap(),
    )
    .unwrap();

    let context = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    assert!(context.required_commands().is_empty());
    assert_eq!(context.action_specs().len(), 1);
}

#[test]
fn contract_four_ignores_stale_legacy_product_versions() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
bootstrap_command = "cargo fetch"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 4,
            "tool_namespace": "jig",
            "jig_version": "0.2.0",
            "required_commands": ["bootstrap_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    assert_eq!(ctx.contract_version(), 4);
}

#[test]
fn legacy_contracts_require_product_versions_in_both_files() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
bootstrap_command = "cargo fetch"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["bootstrap_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = RepoContext::load_from_root(temp.path().to_path_buf())
        .unwrap_err()
        .to_string();

    assert!(error.contains("requires a non-empty jig_version in .jig.toml"));
}
