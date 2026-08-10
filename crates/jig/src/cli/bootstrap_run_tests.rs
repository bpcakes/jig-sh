use clap::ValueEnum;
use serde_json::json;

use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

use super::*;

#[test]
fn bootstrap_vault_capture_is_deferred_only_for_interactive_prompts() {
    assert!(!should_pre_capture_bootstrap_vault(
        true, false, false, false, true
    ));
    assert!(should_pre_capture_bootstrap_vault(
        true, true, false, false, true
    ));
    assert!(should_pre_capture_bootstrap_vault(
        true, false, true, false, true
    ));
    assert!(should_pre_capture_bootstrap_vault(
        true, false, false, true, true
    ));
    assert!(should_pre_capture_bootstrap_vault(
        true, false, false, false, false
    ));
    assert!(!should_pre_capture_bootstrap_vault(
        false, true, true, true, false
    ));
}

#[test]
fn noninteractive_init_vault_error_names_init_and_its_escape_hatches() {
    reject_missing_bootstrap_vault_passphrase(true, true, true, false, BootstrapVaultCommand::Init)
        .unwrap();
    reject_missing_bootstrap_vault_passphrase(
        false,
        true,
        false,
        false,
        BootstrapVaultCommand::Init,
    )
    .unwrap();
    reject_missing_bootstrap_vault_passphrase(
        true,
        false,
        false,
        true,
        BootstrapVaultCommand::Init,
    )
    .unwrap();

    let error = reject_missing_bootstrap_vault_passphrase(
        true,
        false,
        false,
        false,
        BootstrapVaultCommand::Init,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("JIG_VAULT_PASSPHRASE is required"));
    assert!(error.contains("`jig init`"));
    assert!(error.contains("--no-vault"));
    assert!(error.contains("export JIG_VAULT_PASSPHRASE"));

    let no_input_error = reject_missing_bootstrap_vault_passphrase(
        true,
        true,
        false,
        true,
        BootstrapVaultCommand::Adopt,
    )
    .unwrap_err()
    .to_string();
    assert!(no_input_error.contains("`jig adopt --write`"));
}

#[test]
fn unresolved_no_input_init_fails_before_vault_or_destination_writes() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("repo");

    let error = run_init_command(
        bootstrap::InitOpts {
            path: destination.clone(),
            scaffold: bootstrap::ScaffoldOpts::default(),
            template: None,
            template_mode: None,
            vcs_ref: None,
            force: false,
            defaults: false,
            no_input: true,
            no_vault: false,
            answers: bootstrap::AnswerOpts::default(),
        },
        true,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("--no-input was supplied"));
    assert!(!error.contains("JIG_VAULT_PASSPHRASE"));
    assert!(!destination.exists());
}

#[test]
fn invalid_init_destination_fails_before_shape_or_vault_interaction() {
    let _env = lock_env();
    let _passphrase = EnvVarGuard::remove("JIG_VAULT_PASSPHRASE");
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("repo");
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join("existing.txt"), "project-owned\n").unwrap();

    let error = run_init_command(
        bootstrap::InitOpts {
            path: destination,
            scaffold: bootstrap::ScaffoldOpts::default(),
            template: None,
            template_mode: None,
            vcs_ref: None,
            force: false,
            defaults: false,
            no_input: true,
            no_vault: false,
            answers: bootstrap::AnswerOpts::default(),
        },
        true,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Init destination is not empty"), "{error}");
    assert!(!error.contains("--no-input was supplied"), "{error}");
    assert!(!error.contains("JIG_VAULT_PASSPHRASE"), "{error}");
}

#[test]
fn pre_capture_rejects_short_new_vault_passphrase() {
    let _env = lock_env();
    let _passphrase = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", "short");

    let error = runtime::capture_new_vault_passphrase()
        .unwrap_err()
        .to_string();

    assert!(error.contains("at least 12 bytes"));
}

#[test]
fn ensure_bootstrap_vault_initializes_repo_scope_and_reports_created() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    TestRepoBuilder::new(&repo)
        .config(
            r#"
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []

[vault]
scope = "repo"
scope_id = "scope_123"
allow_global = false
"#,
        )
        .required_commands(["bootstrap_command"])
        .write();
    let _vault_home = EnvVarGuard::set("JIG_VAULT_HOME", temp.path().join("vault-base"));
    let _passphrase = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", "correct horse battery staple");
    let bootstrap = json!({ "destination": repo.display().to_string() });

    let output = ensure_bootstrap_vault(&bootstrap, true, true).unwrap();

    assert_eq!(output["requested"], true);
    assert_eq!(output["initialized"], true);
    assert_eq!(output["created"], true);
    assert_eq!(output["vault_scope"], "repo");
    assert_eq!(output["vault_scope_id"], "scope_123");
}

#[test]
fn ensure_bootstrap_vault_late_passphrase_error_mentions_written_repo_files() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    TestRepoBuilder::new(&repo)
        .config(
            r#"
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []

[vault]
scope = "repo"
scope_id = "scope_123"
allow_global = false
"#,
        )
        .required_commands(["bootstrap_command"])
        .write();
    let _vault_home = EnvVarGuard::set("JIG_VAULT_HOME", temp.path().join("vault-base"));
    let _passphrase = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", "short");
    let bootstrap = json!({ "destination": repo.display().to_string() });

    let error = ensure_bootstrap_vault(&bootstrap, true, true)
        .unwrap_err()
        .to_string();

    assert!(error.contains("repo files were written"));
    assert!(error.contains("rerun `jig vault init`"));
}

#[test]
fn presets_summary_explains_defaults_and_ownership() {
    let output = bootstrap::scaffold_presets_report();
    assert_eq!(
        output["presets"].as_array().unwrap().len(),
        bootstrap::ScaffoldPreset::value_variants().len()
    );

    let summary = format_presets_human_summary(&output);

    assert!(summary.contains("available presets"));
    assert!(summary.contains("rust-react"));
    assert!(summary.contains("Rust crate roots default to apps and crates."));
    assert!(summary.contains("apps/<repo>-api"));
    assert!(summary.contains("web: shadcn Vite React product app in web/"));
    assert!(summary.contains("admin: shadcn Vite React admin app in admin-panel/"));
    assert!(summary.contains("React frontends ship tested shadcn 4 sources and provenance"));
    assert!(summary.contains("jig init ./my-app --preset rust-react"));
    assert!(summary.contains("harness-only"));
    assert!(summary.contains("jig init ./my-repo --preset harness-only --no-input --no-vault"));
    assert!(summary.contains("without starter application code"));
    assert!(summary.contains("does not create Rust crates, databases, or frontend applications"));
    assert!(summary.contains("project-owned after creation"));
    assert!(summary.contains("Presets are starter shapes, not long-term application frameworks."));
}

#[test]
fn init_summary_calls_out_custom_bare_frontend_names() {
    let output = json!({
        "destination": "/tmp/demo",
        "template": "embedded",
        "render_report": {
            "files_created": [],
            "files_modified": [],
            "files_removed": []
        },
        "scaffold": {
            "preset": "rust-react",
            "repo_name": "demo",
            "db": "none",
            "frontends": [{ "name": "dashboard" }],
            "frontend_notices": [
                "'dashboard' isn't a preset shorthand — scaffolding a custom Vite SPA in dashboard/."
            ],
            "files_created": [],
            "files_modified": [],
            "files_unchanged": []
        },
        "git_initialized": true,
        "vault": { "requested": false },
        "notes": [],
        "next_steps": []
    });

    let summary = format_init_human_summary(&output);

    assert!(summary.contains("frontend notes:"));
    assert!(summary.contains("'dashboard' isn't a preset shorthand"));
}

#[test]
fn adopt_human_summary_includes_reviewable_next_steps() {
    let output = serde_json::json!({
        "render_mode": "preview",
        "destination": "/tmp/repo",
        "render_report": {
            "files_created": ["scripts/jig"],
            "files_modified": [],
            "files_removed": [],
            "conflicts": [
                {
                    "path": ".agent/PLANS.md",
                    "detail": "destination differs from the rendered template-managed path"
                }
            ]
        },
        "detection_report": {
            "warnings": ["SQLx metadata directory was not detected"]
        },
        "adoption_review": [
            "stack: Rust workspace, SQLx",
            "SQLx: enabled with migrations at migrations"
        ],
        "next_steps": [
            "Re-run jig adopt . --write after reviewing the preview.",
            "No files were changed by this preview."
        ]
    });

    let summary = format_adopt_human_summary(&output);

    assert!(summary.contains("mode: preview"));
    assert!(summary.contains("managed files: 1 created, 0 modified, 0 removed"));
    assert!(summary.contains("stack: Rust workspace, SQLx"));
    assert!(summary.contains(".agent/PLANS.md"));
    assert!(summary.contains("SQLx metadata directory was not detected"));
    assert!(summary.contains("Re-run jig adopt . --write"));
}

#[test]
fn init_human_summary_includes_scaffold_and_next_steps() {
    let output = serde_json::json!({
        "destination": "/tmp/repo",
        "template": "embedded",
        "git_initialized": true,
        "scaffold": {
            "preset": "rust-react",
            "repo_name": "demo",
            "db": "postgres",
            "frontends": [
                { "name": "web", "dir": "web", "kind": "vite" },
                { "name": "landing", "dir": "landing", "kind": "astro" },
                { "name": "admin-panel", "dir": "admin-panel", "kind": "vite" }
            ],
            "files_created": ["Cargo.toml", "web/package.json"],
            "files_modified": [],
            "files_unchanged": ["landing/package.json"]
        },
        "render_report": {
            "files_created": ["scripts/jig", ".jig.toml"],
            "files_modified": [],
            "files_removed": []
        },
        "notes": [
            "SQLx disabled by default until configured."
        ],
        "next_steps": [
            "cd /tmp/repo",
            "scripts/jig setup"
        ]
    });

    let summary = format_init_human_summary(&output);

    assert!(summary.contains("init summary"));
    assert!(summary.contains("target: /tmp/repo"));
    assert!(summary.contains("template: embedded"));
    assert!(summary.contains("managed files: 2 created, 0 modified, 0 removed"));
    assert!(summary.contains("scaffold: rust-react for demo (db: postgres)"));
    assert!(summary.contains("scaffold files: 2 created, 0 modified, 1 unchanged"));
    assert!(summary.contains("frontends: web, landing, admin-panel"));
    assert!(summary.contains("git: initialized"));
    assert!(summary.contains("SQLx disabled by default"));
    assert!(summary.contains("scripts/jig setup"));
    assert!(summary.contains("full report: rerun with --json"));
}

#[test]
fn adopt_human_summary_includes_notes() {
    let summary = format_adopt_human_summary(&json!({
        "render_mode": "preview",
        "destination": "/tmp/repo",
        "render_report": {
            "files_created": [],
            "files_modified": [],
            "files_removed": [],
            "conflicts": []
        },
        "vault": {
            "requested": false
        },
        "adoption_review": [],
        "notes": [
            "Existing .jig.toml had no [vault] block, so Jig added a new repo-scoped vault scope."
        ],
        "detection_report": {
            "warnings": []
        },
        "next_steps": []
    }));

    assert!(summary.contains("notes:"));
    assert!(summary.contains("repo-scoped vault scope"));
}

#[test]
fn update_human_summary_reports_managed_file_counts() {
    let summary = format_update_human_summary(&json!({
        "render_mode": "update",
        "destination": "/tmp/repo",
        "answers_file": ".jig.toml",
        "render_report": {
            "files_created": ["scripts/new-helper.sh"],
            "files_modified": ["scripts/jig", "scripts/install-jig.sh"],
            "files_removed": [],
            "files_unchanged": [".mcp.json"],
            "conflicts": []
        },
        "warnings": ["Embedded launcher templates will replace source-specific customizations."],
        "next_steps": ["Run `jig adopt /tmp/repo --write --force` before a full update."]
    }));

    assert!(summary.contains("update summary"));
    assert!(summary.contains("mode: update"));
    assert!(summary.contains("target: /tmp/repo"));
    assert!(summary.contains("answers: .jig.toml"));
    assert!(summary.contains("managed files: 1 created, 2 modified, 0 removed, 1 unchanged"));
    assert!(summary.contains("warnings:"));
    assert!(summary.contains("Embedded launcher templates"));
    assert!(summary.contains("next steps:"));
    assert!(summary.contains("jig adopt /tmp/repo --write --force"));
    assert!(summary.contains("full report: rerun with --json"));
}
