use serde_json::json;
use tempfile::tempdir;

use super::*;

#[test]
fn supported_contract_versions_are_two_through_four() {
    for version in MIN_SUPPORTED_CONTRACT_VERSION..=CURRENT_CONTRACT_VERSION {
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
            "contract_version": 5,
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

    assert!(error.contains("Unsupported jig contract version: 5"));
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
