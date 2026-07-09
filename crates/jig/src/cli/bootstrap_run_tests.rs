use clap::ValueEnum;
use serde_json::json;

use crate::test_env::{EnvVarGuard, lock_env};

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
fn no_input_bootstrap_vault_requires_env_passphrase() {
    reject_missing_no_input_vault_passphrase(true, true, true).unwrap();
    reject_missing_no_input_vault_passphrase(false, true, false).unwrap();
    reject_missing_no_input_vault_passphrase(true, false, false).unwrap();

    let error = reject_missing_no_input_vault_passphrase(true, true, false)
        .unwrap_err()
        .to_string();

    assert!(error.contains("JIG_VAULT_PASSPHRASE is required"));
    assert!(error.contains("--no-vault"));
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
    std::fs::create_dir_all(repo.join(".agent")).unwrap();
    std::fs::write(
        repo.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
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
    .unwrap();
    std::fs::write(
        repo.join(".agent/jig-contract.json"),
        json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["bootstrap_command"],
            "tools": []
        })
        .to_string(),
    )
    .unwrap();
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
    std::fs::create_dir_all(repo.join(".agent")).unwrap();
    std::fs::write(
        repo.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
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
    .unwrap();
    std::fs::write(
        repo.join(".agent/jig-contract.json"),
        json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["bootstrap_command"],
            "tools": []
        })
        .to_string(),
    )
    .unwrap();
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
    assert!(summary.contains("admin: Vite React admin app in admin-panel/"));
    assert!(summary.contains("jig init ./my-app --preset rust-react"));
    assert!(summary.contains("project-owned after creation"));
    assert!(summary.contains("Presets are starter shapes, not long-term application frameworks."));
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
            "scripts/jig doctor"
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
    assert!(summary.contains("scripts/jig doctor"));
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
        }
    }));

    assert!(summary.contains("update summary"));
    assert!(summary.contains("mode: update"));
    assert!(summary.contains("target: /tmp/repo"));
    assert!(summary.contains("answers: .jig.toml"));
    assert!(summary.contains("managed files: 1 created, 2 modified, 0 removed, 1 unchanged"));
    assert!(summary.contains("full report: rerun with --json"));
}
