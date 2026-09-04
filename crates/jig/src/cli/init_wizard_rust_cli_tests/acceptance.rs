use super::*;

#[test]
fn explicit_rust_cli_is_complete_in_strict_defaults_and_no_terminal_modes() {
    for mode in ["--no-input", "--defaults"] {
        let mut opts = init_opts(&[
            "jig",
            "init",
            "ExampleCli",
            "--preset",
            "rust-cli",
            mode,
            "--no-vault",
        ]);

        prepare(&mut opts).unwrap();

        assert_eq!(opts.scaffold.preset, Some(ScaffoldPreset::RustCli));
        assert_eq!(opts.scaffold.db, None);
        assert!(!opts.scaffold.has_frontends());
        assert_eq!(
            opts.answers.backend_language,
            Some(crate::bootstrap::BackendLanguage::Rust)
        );
        assert_eq!(opts.answers.sqlx_enabled, Some(false));
    }

    let mut opts = init_opts(&[
        "jig",
        "init",
        "ExampleCli",
        "--preset",
        "rust-cli",
        "--no-vault",
    ]);
    let mut prepared = prepare_init_answers(&opts).unwrap();
    prepared.move_effective_to(&mut opts.answers).unwrap();
    prepare_merged_init_interaction(
        &mut opts,
        &mut prepared,
        InitInteractionPolicy::Strict { non_terminal: true },
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap();

    assert_eq!(opts.scaffold.preset, Some(ScaffoldPreset::RustCli));
    assert_eq!(opts.answers.sqlx_enabled, Some(false));
}

#[test]
fn rust_cli_accepts_supported_cli_authority() {
    let mut opts = init_opts(&[
        "jig",
        "init",
        "ExampleCli",
        "--preset",
        "rust-cli",
        "--repo-name",
        "CliProject",
        "--default-branch",
        "trunk",
        "--ci-github-runner",
        "ubuntu-24.04",
        "--template",
        "/tmp/ExampleProject-template",
        "--template-mode",
        "committed",
        "--vcs-ref",
        "main",
        "--template-source-url",
        "https://example.com/ExampleProject.git",
        "--rust-crate-root",
        "crates",
        "--sqlx-enabled",
        "false",
        "--schema-dump-enabled",
        "false",
        "--rust-fmt-check-command",
        "cargo fmt --all -- --check",
        "--rust-clippy-command",
        "cargo clippy --workspace --all-targets --locked -- -D warnings",
        "--rust-test-command",
        "cargo test --workspace",
        "--rust-test-locked-command",
        "cargo test --workspace --locked",
        "--bootstrap-command",
        "cargo fetch",
        "--contract-check-command",
        "scripts/jig check contract",
        "--web-package-manager",
        "npm",
        "--application-contracts-enabled",
        "false",
        "--force",
        "--defaults",
        "--no-vault",
    ]);

    prepare(&mut opts).unwrap();

    assert_eq!(opts.answers.repo_name.as_deref(), Some("CliProject"));
    assert_eq!(opts.answers.default_branch.as_deref(), Some("trunk"));
    assert_eq!(opts.answers.rust_crate_roots, ["crates"]);
    assert_eq!(opts.answers.sqlx_enabled, Some(false));
    assert_eq!(opts.answers.schema_dump_enabled, Some(false));
    assert_eq!(opts.answers.web_package_manager.as_deref(), Some("npm"));
}

#[test]
fn rust_cli_accepts_the_complete_harness_only_answer_family() {
    let temp = tempdir().unwrap();
    let answers = temp.path().join("answers.toml");
    fs::write(
        &answers,
        r#"repo_name = "ExampleCli"
default_branch = "trunk"
ci_github_runner = "ubuntu-24.04"
jig_version = "legacy-compatibility-input"
template_source_url = "https://example.com/ExampleProject.git"
harness_footprint = "full"
backend_language = "rust"
sqlx_enabled = false
rust_crate_roots = ["crates"]
schema_dump_enabled = false
bootstrap_command = "cargo fetch"
contract_check_command = "scripts/jig check contract"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "npm"
application_contracts_enabled = false
frontend_apps = []
frontend_workspace_roots = []

[vault]
scope = "repo"
scope_id = "scope_123"
allow_global = false

[status]
providers = []

[execution]
command_timeout_seconds = 300
command_output_limit_bytes = 1048576

[agent_tooling.codex]
marketplaces = []

[dev]
proxy_port = 2455
https_port = 2443
https = false
http2 = true
lan = false
tld = "localhost"
workspace_discovery = false
"#,
    )
    .unwrap();
    let mut opts = init_opts(&[
        "jig",
        "init",
        "ExampleCli",
        "--preset",
        "rust-cli",
        "--answers-file",
        answers.to_str().unwrap(),
        "--template",
        "/tmp/ExampleProject-template",
        "--template-mode",
        "committed",
        "--vcs-ref",
        "main",
        "--force",
        "--no-input",
        "--no-vault",
    ]);

    prepare(&mut opts).unwrap();

    assert_eq!(opts.answers.repo_name.as_deref(), Some("ExampleCli"));
    assert_eq!(opts.answers.default_branch.as_deref(), Some("trunk"));
    assert_eq!(opts.answers.web_package_manager.as_deref(), Some("npm"));
    assert_eq!(opts.answers.rust_crate_roots, ["crates"]);
}

#[test]
fn rust_cli_applies_cli_precedence_before_policy_and_ignores_inert_package_manager() {
    let temp = tempdir().unwrap();
    let answers = temp.path().join("answers.toml");
    fs::write(
        &answers,
        "sqlx_enabled = true\nweb_package_manager = \"npm\"\n",
    )
    .unwrap();
    let mut opts = init_opts(&[
        "jig",
        "init",
        "ExampleCli",
        "--preset",
        "rust-cli",
        "--answers-file",
        answers.to_str().unwrap(),
        "--sqlx-enabled",
        "false",
        "--no-input",
        "--no-vault",
    ]);

    prepare(&mut opts).unwrap();
    preflight_init_package_manager_with(&opts, |_| false).unwrap();
    assert_eq!(opts.answers.sqlx_enabled, Some(false));
    assert_eq!(opts.answers.web_package_manager.as_deref(), Some("npm"));
}
