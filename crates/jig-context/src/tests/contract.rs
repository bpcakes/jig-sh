use super::*;

#[test]
fn contract_version_probe_keeps_manifest_path_and_parse_cause() {
    let temp = tempdir().unwrap();
    fs::create_dir(temp.path().join(".agent")).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    for (body, cause) in [
        (
            r#"{"contract_version":8,"contract_version":7}"#,
            "duplicate JSON object key",
        ),
        (r#"{"contract_version":8"#, "EOF while parsing an object"),
        (r#"{"contract_version":"invalid"}"#, "invalid type"),
    ] {
        fs::write(&manifest_path, body).unwrap();
        let error = RepoContext::declared_contract_version_from_root(temp.path()).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("Failed to parse {}", manifest_path.display())
        );
        assert!(format!("{error:#}").contains(cause), "{error:#}");
    }
}

#[test]
fn contract_nine_execution_authority_has_no_work_section() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let config_path = temp.path().join(".jig.toml");
    let original = fs::read_to_string(&config_path).unwrap();
    let with_work = format!(
        "{}\n[work]\nchecks = [\"jig.contract_check\"]\n",
        original.trim_end()
    );
    let mut manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(temp.path().join(".agent/jig-contract.json")).unwrap(),
    )
    .unwrap();
    let digest = |manifest: &serde_json::Value, source: &str| {
        fs::write(&config_path, source).unwrap();
        contract_source_digest(
            &load_config_snapshot(&config_path).unwrap().config,
            manifest,
        )
        .unwrap()
    };

    manifest["contract_version"] = json!(8);
    assert_ne!(digest(&manifest, &original), digest(&manifest, &with_work));
    manifest["contract_version"] = json!(9);
    assert_eq!(digest(&manifest, &original), digest(&manifest, &with_work));
}

#[test]
fn contract_digest_uses_canonical_execution_authority() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let config_path = temp.path().join(".jig.toml");
    let original_source = fs::read_to_string(&config_path).unwrap();
    let snapshot = load_config_snapshot(&config_path).unwrap();
    let manifest_text = fs::read_to_string(temp.path().join(".agent/jig-contract.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text).unwrap();

    let expected = contract_source_digest(&snapshot.config, &manifest).unwrap();
    fs::write(
        &config_path,
        format!(
            "{}\n[dev]\nproxy_port = 2456\n# local runtime settings and comments are not execution authority\n",
            original_source.trim_end()
        ),
    )
    .unwrap();
    let comment_only = load_config_snapshot(&config_path).unwrap();
    assert_eq!(
        contract_source_digest(&comment_only.config, &manifest).unwrap(),
        expected
    );

    let changed_source = format!(
        "{}\n[commands]\nrust_test_command = \"cargo nextest run\"\n",
        original_source.trim_end()
    );
    fs::write(&config_path, changed_source).unwrap();
    let changed = load_config_snapshot(&config_path).unwrap();
    assert_ne!(
        contract_source_digest(&changed.config, &manifest).unwrap(),
        expected
    );

    for authority_change in [
        "harness_footprint = \"minimal\"\n",
        "[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\n",
        "[work]\nchecks = [\"jig.contract_check\"]\n",
        "[work.tracker]\nkind = \"beads\"\nworkspace_id = \"01ARZ3NDEKTSV4RRFFQ69G5FAV\"\n",
    ] {
        fs::write(
            &config_path,
            format!("{}\n{authority_change}", original_source.trim_end()),
        )
        .unwrap();
        let changed = load_config_snapshot(&config_path).unwrap();
        assert_ne!(
            contract_source_digest(&changed.config, &manifest).unwrap(),
            expected,
            "native contract-check input must participate in execution authority: {authority_change}"
        );
    }
}

#[test]
fn contract_digest_preserves_forward_compatible_manifest_fields() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .tool(json!({
            "name": "jig.future",
            "kind": "native",
            "description": "Future-compatible fixture.",
            "future_policy": {"mode": "original"},
        }))
        .write();
    let path = temp.path().join(".agent/jig-contract.json");
    let original = RepoContext::load_from(temp.path()).unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    manifest["unmodeled_authority"] = json!("must not be dropped from the digest");
    manifest["tools"][0]["future_policy"]["mode"] = json!("changed");
    fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();

    let changed = RepoContext::load_from(temp.path()).unwrap();

    assert_ne!(changed.contract_digest(), original.contract_digest());
}

#[test]
fn v3_contracts_use_required_commands() {
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
rust_fmt_check_command = "cargo fmt --check"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["bootstrap_command", "rust_fmt_check_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();

    assert_eq!(ctx.contract_version(), 3);
    assert_eq!(
        ctx.required_commands(),
        ["bootstrap_command", "rust_fmt_check_command"]
    );
    assert_eq!(
        ctx.command_for_key("bootstrap_command").unwrap(),
        "cargo fetch"
    );
}

#[test]
fn missing_legacy_contract_check_command_stays_empty() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
rust_fmt_check_command = "cargo fmt --check"
rust_clippy_command = "cargo clippy"
rust_test_command = "cargo test"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 2,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["contract_check_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = ctx.command_for_key("contract_check_command").unwrap_err();

    assert!(
        error
            .to_string()
            .contains("contract_check_command is empty")
    );
}

#[test]
fn legacy_work_checks_become_required_check_gates() {
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
checks = ["jig.contract_check"]
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
            "tools": [
                {
                    "name": "jig.contract_check",
                    "kind": "command",
                    "description": "Run contract check.",
                    "command": "contract_check_command"
                }
            ],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let gates = ctx.work_gates();
    assert_eq!(gates.len(), 1);
    let WorkGate::Check(gate) = &gates[0] else {
        panic!("expected check gate");
    };
    assert_eq!(gate.id, "contract-check");
    assert_eq!(gate.tool, "jig.contract_check");
    assert!(gate.required);
}

#[test]
fn check_gate_path_policy_loads_and_retains_reuse_configuration() {
    let config: WorkConfig = toml::from_str(
        r#"
[[gates]]
id = "rust-tests"
kind = "check"
tool = "jig.test"
paths = ["crates/**", "Cargo.toml"]
paths_ignore = ["crates/generated/**"]
reuse = true
"#,
    )
    .unwrap();
    config.validate().unwrap();

    let WorkGate::Check(gate) = &config.gates()[0] else {
        panic!("expected check gate");
    };
    assert_eq!(gate.paths.as_deref().unwrap(), ["crates/**", "Cargo.toml"]);
    assert_eq!(gate.paths_ignore, ["crates/generated/**"]);
    assert!(gate.reuse);
}

#[test]
fn check_gate_path_policy_rejects_unsafe_and_ambiguous_patterns() {
    for (field, value, expected) in [
        ("paths", "[]", "at least one"),
        ("paths", "[\"../private/**\"]", "unsafe paths"),
        ("paths", "[\".agent/**\"]", "outside .agent"),
        (
            "paths",
            "[\"crates/{api,cli}/**\"]",
            "without brace expansion",
        ),
        ("paths", "[\"crates/api**/src\"]", "complete path component"),
    ] {
        let source = format!(
            r#"
[[gates]]
id = "rust-tests"
kind = "check"
tool = "jig.test"
{field} = {value}
"#
        );
        let config: WorkConfig = toml::from_str(&source).unwrap();
        let error = config.validate().unwrap_err().to_string();
        assert!(error.contains(expected), "unexpected error: {error}");
    }

    let config: WorkConfig = toml::from_str(
        r#"
[[gates]]
id = "rust-tests"
kind = "check"
tool = "jig.test"
paths_ignore = ["docs/**"]
"#,
    )
    .unwrap();
    assert!(
        config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("paths_ignore without paths")
    );
}

#[test]
fn legacy_contract_rejects_v5_only_gate_policy() {
    for policy in [
        "paths = [\"crates/**\"]",
        "paths = [\"crates/**\"]\npaths_ignore = [\"crates/generated/**\"]",
        "paths = [\"crates/**\"]\npaths_ignore = []",
        "reuse = true",
        "reuse = false",
    ] {
        let temp = tempdir().unwrap();
        crate::test_support::TestRepoBuilder::new(temp.path())
            .contract_version(4)
            .config(format!(
                r#"
[commands]
rust_test_command = "cargo test"

[[work.gates]]
id = "rust-tests"
kind = "check"
tool = "jig.test"
{policy}
"#
            ))
            .required_commands(["rust_test_command"])
            .tool(json!({
                "name": "jig.test",
                "kind": "command",
                "description": "Run tests.",
                "command": "rust_test_command"
            }))
            .write();

        let error = RepoContext::load_from_root(temp.path().to_path_buf())
            .unwrap_err()
            .to_string();
        assert!(error.contains("require contract version 5"), "{error}");
    }
}
