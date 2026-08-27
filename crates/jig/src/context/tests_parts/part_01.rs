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
fn schema_docs_dir_is_repository_relative_normalized_and_literal() {
    for (value, expected) in [
        ("../schema", "stay inside"),
        ("docs//schema", "normalized"),
        (".", "dedicated directory"),
        (".agent/schema", "outside reserved"),
        (".git/schema", "outside reserved"),
        (".Agent/schema", "outside reserved"),
        ("generated/.GIT/schema", "outside reserved"),
        (":(exclude)docs/schema", "unsupported characters"),
    ] {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .config(format!("schema_docs_dir = {value:?}\n"))
            .write();

        let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();
        assert!(error.contains(expected), "unexpected error: {error}");
    }
    let hfs_alias = validate_schema_docs_dir("generated/\u{200c}.git/schema")
        .unwrap_err()
        .to_string();
    assert!(hfs_alias.contains("outside reserved"), "{hfs_alias}");

    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .config("schema_docs_dir = \"artifacts/schema\"\n")
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    assert_eq!(ctx.schema_docs_dir(), "artifacts/schema");
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
        crate::test_env::TestRepoBuilder::new(temp.path())
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

#[test]
fn work_refinements_are_loaded_for_refinement_execution() {
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
skill = "jig-rust:rust-simplify"
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

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let refinements = ctx.work_refinements();
    assert_eq!(refinements.len(), 1);
    assert_eq!(refinements[0].id, "rust-simplify");
    assert_eq!(
        refinements[0].skill.as_deref(),
        Some("jig-rust:rust-simplify")
    );
}

#[test]
fn multiple_work_refinements_are_rejected_until_selection_exists() {
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

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();

    assert!(error.contains("Only one [[work.refinements]] entry is supported"));
}
