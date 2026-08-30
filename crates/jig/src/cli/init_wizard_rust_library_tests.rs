use std::fs;
use std::io::Cursor;

use clap::Parser;
use tempfile::tempdir;

use super::*;
use crate::cli::{Cli, CommandKind};
use crate::test_env::lock_env;

fn init_opts(args: &[&str]) -> InitOpts {
    match Cli::try_parse_from(args).unwrap().command {
        CommandKind::Init(opts) => opts,
        other => panic!("expected init command, got {other:?}"),
    }
}

fn prepare(opts: &mut InitOpts) -> Result<bootstrap::PreparedInitAnswers> {
    prepare_init_interaction_with_io(opts, &mut Cursor::new(Vec::<u8>::new()), &mut Vec::new())
}

fn copy_tree(source: &std::path::Path, destination: &std::path::Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn explicit_rust_library_is_complete_in_strict_and_defaults_modes() {
    for mode in ["--no-input", "--defaults"] {
        let mut opts = init_opts(&[
            "jig",
            "init",
            "ExampleLibrary",
            "--preset",
            "rust-library",
            mode,
            "--no-vault",
        ]);

        prepare(&mut opts).unwrap();

        assert_eq!(opts.scaffold.preset, Some(ScaffoldPreset::RustLibrary));
        assert_eq!(opts.scaffold.db, None);
        assert!(!opts.scaffold.has_frontends());
        assert_eq!(
            opts.answers.backend_language,
            Some(crate::bootstrap::BackendLanguage::Rust)
        );
        assert_eq!(opts.answers.sqlx_enabled, Some(false));
    }
}

#[test]
fn explicit_rust_library_is_complete_when_no_terminal_is_available() {
    let mut opts = init_opts(&[
        "jig",
        "init",
        "ExampleLibrary",
        "--preset",
        "rust-library",
        "--no-vault",
    ]);
    let mut prepared = prepare_init_answers(&opts).unwrap();
    prepared.copy_effective_to(&mut opts.answers);

    prepare_merged_init_interaction(
        &mut opts,
        &mut prepared,
        InitInteractionPolicy::Strict { non_terminal: true },
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap();

    assert_eq!(opts.scaffold.preset, Some(ScaffoldPreset::RustLibrary));
    assert_eq!(opts.answers.sqlx_enabled, Some(false));
}

#[test]
fn rust_library_accepts_supported_cli_authority() {
    let mut opts = init_opts(&[
        "jig",
        "init",
        "ExampleLibrary",
        "--preset",
        "rust-library",
        "--repo-name",
        "CliLibrary",
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

    assert_eq!(opts.answers.repo_name.as_deref(), Some("CliLibrary"));
    assert_eq!(opts.answers.default_branch.as_deref(), Some("trunk"));
    assert_eq!(opts.answers.rust_crate_roots, ["crates"]);
    assert_eq!(opts.answers.sqlx_enabled, Some(false));
    assert_eq!(opts.answers.schema_dump_enabled, Some(false));
    assert_eq!(opts.answers.web_package_manager.as_deref(), Some("npm"));
}

#[test]
fn rust_library_accepts_the_complete_harness_only_answer_family() {
    let temp = tempdir().unwrap();
    let answers = temp.path().join("answers.toml");
    fs::write(
        &answers,
        r#"repo_name = "ExampleLibrary"
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
        "ExampleLibrary",
        "--preset",
        "rust-library",
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

    assert_eq!(opts.answers.repo_name.as_deref(), Some("ExampleLibrary"));
    assert_eq!(opts.answers.default_branch.as_deref(), Some("trunk"));
    assert_eq!(opts.answers.web_package_manager.as_deref(), Some("npm"));
    assert_eq!(opts.answers.rust_crate_roots, ["crates"]);
}

#[test]
fn rust_library_rejects_every_incompatible_cli_family() {
    let cases: &[(&[&str], &str)] = &[
        (&["--db", "none"], "--db"),
        (&["--frontend", "web"], "--frontend"),
        (&["--frontends", "web"], "--frontends"),
        (&["--frontend-app", "web:web:80"], "frontend_apps"),
        (&["--go-module", "example.com/ExampleProject"], "go_module"),
        (&["--rust-crate-root", "libs"], "rust_crate_roots"),
        (&["--sqlx-enabled", "true"], "sqlx_enabled"),
        (&["--schema-dump-enabled", "true"], "schema_dump_enabled"),
        (
            &["--rust-migration-dir", "migrations"],
            "rust_migration_dir",
        ),
        (
            &["--rust-migration-layout", "flat_migrations"],
            "rust_migration_layout",
        ),
        (
            &["--rust-sqlx-metadata-dir", ".sqlx"],
            "rust_sqlx_metadata_dir",
        ),
        (&["--schema-dump-command", "dump"], "schema_dump_command"),
        (&["--schema-docs-dir", "docs/schema"], "schema_docs_dir"),
        (&["--schema-check-command", "check"], "schema_check_command"),
        (&["--sqlx-check-command", "check"], "sqlx_check_command"),
        (&["--migration-add-command", "add"], "migration_add_command"),
        (&["--dev-command", "serve"], "dev_command"),
        (
            &["--application-contracts-enabled", "true"],
            "application_contracts_enabled",
        ),
    ];

    for (extra, expected) in cases {
        let mut args = vec![
            "jig",
            "init",
            "ExampleLibrary",
            "--preset",
            "rust-library",
            "--no-input",
            "--no-vault",
        ];
        args.extend_from_slice(extra);
        let mut opts = init_opts(&args);

        let error = prepare(&mut opts).unwrap_err().to_string();

        assert!(error.contains("rust-library"), "{extra:?}: {error}");
        assert!(error.contains(expected), "{extra:?}: {error}");
    }
}

#[test]
fn rust_library_rejects_every_incompatible_answer_file_family() {
    let cases = [
        ("harness_footprint = \"minimal\"\n", "harness_footprint"),
        ("backend_language = \"go\"\n", "backend_language"),
        ("go_module = \"example.com/ExampleProject\"\n", "go_module"),
        ("go_database = \"none\"\n", "go_database"),
        ("rust_crate_roots = []\n", "rust_crate_roots"),
        ("rust_crate_roots = [\"libs\"]\n", "rust_crate_roots"),
        ("sqlx_enabled = true\n", "sqlx_enabled"),
        ("schema_dump_enabled = true\n", "schema_dump_enabled"),
        (
            "rust_migration_dir = \"migrations\"\n",
            "rust_migration_dir",
        ),
        ("migration_dir = \"migrations\"\n", "migration_dir"),
        (
            "rust_migration_layout = \"flat_migrations\"\n",
            "rust_migration_layout",
        ),
        (
            "rust_sqlx_metadata_dir = \".sqlx\"\n",
            "rust_sqlx_metadata_dir",
        ),
        ("schema_dump_command = \"dump\"\n", "schema_dump_command"),
        ("schema_docs_dir = \"docs/schema\"\n", "schema_docs_dir"),
        ("schema_check_command = \"check\"\n", "schema_check_command"),
        ("sqlx_check_command = \"check\"\n", "sqlx_check_command"),
        ("migration_add_command = \"add\"\n", "migration_add_command"),
        ("go_fmt_check_command = \"fmt\"\n", "go_fmt_check_command"),
        ("go_lint_command = \"lint\"\n", "go_lint_command"),
        ("go_test_command = \"test\"\n", "go_test_command"),
        (
            "go_test_locked_command = \"test\"\n",
            "go_test_locked_command",
        ),
        ("sqlc_check_command = \"sqlc\"\n", "sqlc_check_command"),
        (
            "typescript_lint_command = \"lint\"\n",
            "typescript_lint_command",
        ),
        (
            "typescript_typecheck_command = \"typecheck\"\n",
            "typescript_typecheck_command",
        ),
        (
            "typescript_build_command = \"build\"\n",
            "typescript_build_command",
        ),
        (
            "typescript_coverage_command = \"coverage\"\n",
            "typescript_coverage_command",
        ),
        ("dev_command = \"serve\"\n", "dev_command"),
        (
            "application_contracts_enabled = true\n",
            "application_contracts_enabled",
        ),
        (
            "[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
            "frontend_apps",
        ),
        (
            "frontend_workspace_roots = [\"packages\"]\n",
            "frontend_workspace_roots",
        ),
        (
            "[[dev.apps]]\nname = \"worker\"\nargv = [\"cargo\", \"run\"]\n",
            "dev.apps",
        ),
        (
            "[repository]\ndefault_check_profile = \"verify\"\n",
            "repository",
        ),
        (
            "[commands]\nrepo_bootstrap_command = \"cargo fetch\"\n",
            "commands",
        ),
        ("[work]\nrequired_gates = []\n", "work"),
        ("[loop]\nworkflows = []\n", "loop"),
        (
            "unexpected_shape_authority = true\n",
            "unexpected_shape_authority",
        ),
    ];
    let temp = tempdir().unwrap();

    for (index, (body, expected)) in cases.into_iter().enumerate() {
        let answers = temp.path().join(format!("answers-{index}.toml"));
        fs::write(&answers, body).unwrap();
        let mut opts = init_opts(&[
            "jig",
            "init",
            "ExampleLibrary",
            "--preset",
            "rust-library",
            "--answers-file",
            answers.to_str().unwrap(),
            "--no-input",
            "--no-vault",
        ]);

        let error = prepare(&mut opts).unwrap_err().to_string();

        assert!(error.contains("rust-library"), "{body}: {error}");
        assert!(error.contains(expected), "{body}: {error}");
    }
}

#[test]
fn rust_library_treats_normalized_empty_optional_strings_as_absent() {
    let temp = tempdir().unwrap();
    let answers = temp.path().join("answers.toml");
    fs::write(
        &answers,
        r#"rust_migration_dir = ""
migration_dir = ""
rust_sqlx_metadata_dir = ""
schema_dump_command = ""
schema_check_command = ""
sqlx_check_command = ""
migration_add_command = ""
go_module = ""
go_fmt_check_command = ""
go_lint_command = ""
go_test_command = ""
go_test_locked_command = ""
sqlc_check_command = ""
typescript_lint_command = ""
typescript_typecheck_command = ""
typescript_build_command = ""
typescript_coverage_command = ""
dev_command = ""
"#,
    )
    .unwrap();
    let mut opts = init_opts(&[
        "jig",
        "init",
        "ExampleLibrary",
        "--preset",
        "rust-library",
        "--answers-file",
        answers.to_str().unwrap(),
        "--no-input",
        "--no-vault",
    ]);

    prepare(&mut opts).unwrap();
}

#[test]
fn rust_library_ignores_inert_web_package_manager_preflight() {
    let mut opts = init_opts(&[
        "jig",
        "init",
        "ExampleLibrary",
        "--preset",
        "rust-library",
        "--web-package-manager",
        "npm",
        "--no-input",
        "--no-vault",
    ]);
    prepare(&mut opts).unwrap();

    preflight_init_package_manager_with(&opts, |_| false).unwrap();
}

#[test]
fn rust_library_renders_from_the_frozen_answer_input_after_the_file_changes() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let answers = temp.path().join("answers.toml");
    fs::write(
        &answers,
        "repo_name = \"FrozenLibrary\"\njig_version = \"legacy-input\"\n",
    )
    .unwrap();
    let destination = temp.path().join("repo");
    let template = temp.path().join("template");
    copy_tree(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("templates/project"),
        &template.join("templates/project"),
    );
    let mut opts = init_opts(&[
        "jig",
        "init",
        destination.to_str().unwrap(),
        "--preset",
        "rust-library",
        "--answers-file",
        answers.to_str().unwrap(),
        "--no-input",
        "--no-vault",
    ]);
    opts.template = Some(template.display().to_string());
    let prepared = prepare(&mut opts).unwrap();
    fs::write(
        &answers,
        "repo_name = \"ChangedLibrary\"\nunexpected_shape_authority = true\n",
    )
    .unwrap();

    bootstrap::run_prepared_init(opts, prepared).unwrap();

    let rendered = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(rendered.contains("repo_name = \"frozenlibrary\""));
    assert!(!rendered.contains("ChangedLibrary"));
    assert!(!rendered.contains("jig_version"));
    assert!(
        destination
            .join("crates/frozenlibrary/src/lib.rs")
            .is_file()
    );
}
