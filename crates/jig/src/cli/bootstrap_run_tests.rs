use clap::ValueEnum;
use serde_json::json;

use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

use super::*;

fn init_report(value: serde_json::Value) -> bootstrap::InitReport {
    serde_json::from_value(value).unwrap()
}

#[test]
fn bootstrap_vault_capture_is_deferred_only_for_interactive_prompts() {
    use BootstrapInputMode::{Defaults, Interactive, NoInput};
    use BootstrapPassphraseAvailability::{Environment, Prompt, Unavailable};
    use BootstrapVaultIntent::{Disabled, Initialize};
    use BootstrapVaultPlan::{CaptureAfterRender, PreCaptured};

    let resolve = |intent, mode, availability| {
        BootstrapVaultPlan::resolve(intent, mode, availability, BootstrapVaultCommand::Init)
    };

    assert_eq!(BootstrapInputMode::from_flags(true, true), NoInput);
    assert_eq!(
        resolve(Initialize, Interactive, Prompt).unwrap(),
        CaptureAfterRender
    );
    assert_eq!(
        resolve(Initialize, NoInput, Environment).unwrap(),
        PreCaptured
    );
    assert_eq!(resolve(Initialize, Defaults, Prompt).unwrap(), PreCaptured);
    assert_eq!(
        resolve(Initialize, Interactive, Environment).unwrap(),
        PreCaptured
    );
    assert_eq!(
        resolve(Disabled, NoInput, Unavailable).unwrap(),
        BootstrapVaultPlan::Disabled
    );
    assert!(resolve(Initialize, Interactive, Unavailable).is_err());
    assert!(resolve(Initialize, NoInput, Prompt).is_err());
}

#[test]
fn noninteractive_init_vault_error_names_init_and_its_escape_hatches() {
    use BootstrapInputMode::{Interactive, NoInput};
    use BootstrapPassphraseAvailability::{Environment, Prompt, Unavailable};
    use BootstrapVaultIntent::{Disabled, Initialize};

    BootstrapVaultPlan::resolve(
        Initialize,
        NoInput,
        Environment,
        BootstrapVaultCommand::Init,
    )
    .unwrap();
    BootstrapVaultPlan::resolve(Disabled, NoInput, Unavailable, BootstrapVaultCommand::Init)
        .unwrap();
    BootstrapVaultPlan::resolve(Initialize, Interactive, Prompt, BootstrapVaultCommand::Init)
        .unwrap();

    let error = BootstrapVaultPlan::resolve(
        Initialize,
        Interactive,
        Unavailable,
        BootstrapVaultCommand::Init,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("JIG_VAULT_PASSPHRASE is required"));
    assert!(error.contains("`jig init`"));
    assert!(error.contains("--no-vault"));
    assert!(error.contains("export JIG_VAULT_PASSPHRASE"));

    let no_input_error =
        BootstrapVaultPlan::resolve(Initialize, NoInput, Prompt, BootstrapVaultCommand::Adopt)
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
fn invalid_go_module_fails_before_vault_capture_or_destination_writes() {
    let _env = lock_env();
    let _passphrase = EnvVarGuard::remove("JIG_VAULT_PASSPHRASE");
    let temp = tempfile::tempdir().unwrap();

    for (destination_name, module) in [
        ("ExampleProjectDot", "example.com/ExampleProject."),
        ("ExampleProjectReserved", "example.com/con"),
        (
            "ExampleProjectShortName",
            "example.com/vault-consumer-fixture/foo~1",
        ),
    ] {
        let destination = temp.path().join(destination_name);
        let error = run_init_command(
            bootstrap::InitOpts {
                path: destination.clone(),
                scaffold: bootstrap::ScaffoldOpts {
                    preset: Some(bootstrap::ScaffoldPreset::GoReact),
                    db: Some(bootstrap::ScaffoldDb::None),
                    frontends: vec![bootstrap::parse_scaffold_frontend("web").unwrap()],
                    frontend_list: Vec::new(),
                },
                template: None,
                template_mode: None,
                vcs_ref: None,
                force: false,
                defaults: true,
                no_input: false,
                no_vault: false,
                answers: bootstrap::AnswerOpts {
                    go_module: Some(module.into()),
                    ..bootstrap::AnswerOpts::default()
                },
            },
            true,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("Invalid --go-module"), "{module}: {error}");
        assert!(!error.contains("JIG_VAULT_PASSPHRASE"));
        assert!(!destination.exists());
    }
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
    let output = ensure_bootstrap_vault(
        repo.to_str().unwrap(),
        BootstrapVaultPlan::CaptureAfterRender,
    )
    .unwrap();
    let output = serde_json::to_value(output).unwrap();

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
    let error = ensure_bootstrap_vault(
        repo.to_str().unwrap(),
        BootstrapVaultPlan::CaptureAfterRender,
    )
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
    assert_eq!(
        output["presets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|preset| preset["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "rust-react",
            "go-react",
            "harness-only",
            "rust-library",
            "rust-cli",
        ]
    );
    assert_eq!(
        output["presets"][4],
        json!({
            "name": "rust-cli",
            "summary": "Expandable Rust workspace with one command-line binary crate.",
            "defaults": [
                "The virtual workspace uses crates/<repo> as its only initial member.",
                "Rust 2024 uses the top-level Jig workspace Rust baseline.",
                "The starter binary uses only std and prints its package name and version.",
                "SQLx, schema dumps, application contracts, frontends, and dev apps are disabled."
            ],
            "layout": [
                "Cargo.toml virtual workspace",
                "crates/<repo> command-line binary crate"
            ],
            "frontend_shorthands": [],
            "examples": [
                "jig init ./example-cli --preset rust-cli --no-input --no-vault",
                "cargo run -p example-cli"
            ],
            "ownership": "The generated Cargo manifests, Rust source, crate guide, and README are project-owned after creation; jig update keeps only the Jig harness current.",
            "non_goals": [
                "The rust-cli preset does not create a database, frontend, API, dev app, release workflow, library target, or additional crate layers.",
                "The scaffold does not select a license, enable package publication, or choose an argument parser or logging framework."
            ]
        })
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
    assert!(summary.contains("rust-library"));
    assert!(summary.contains("Expandable Rust workspace with one library crate."));
    assert!(summary.contains("Cargo.toml virtual workspace"));
    assert!(summary.contains("crates/<repo> library crate"));
    assert!(
        summary.contains("jig init ./example-library --preset rust-library --no-input --no-vault")
    );
    assert!(summary.contains("does not create a database, frontend, API, dev app"));
    assert!(summary.contains("does not select a license or enable package publication"));
    assert!(summary.contains("rust-cli"));
    assert!(summary.contains("Expandable Rust workspace with one command-line binary crate."));
    assert!(summary.contains("crates/<repo> command-line binary crate"));
    assert!(summary.contains("jig init ./example-cli --preset rust-cli --no-input --no-vault"));
    assert!(summary.contains("cargo run -p example-cli"));
    assert!(summary.contains("does not create a database, frontend, API, dev app"));
    assert!(summary.contains("does not select a license, enable package publication"));
    assert!(summary.contains("argument parser or logging framework"));
}

#[test]
fn init_summary_calls_out_custom_bare_frontend_names() {
    let output = init_report(json!({
        "ok": true,
        "command": "init",
        "render_mode": "copy",
        "destination": "/tmp/demo",
        "template": "embedded",
        "answers_file": ".jig.toml",
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
        "vault": {
            "requested": false,
            "initialized": false,
            "created": false,
            "skipped_reason": "disabled"
        },
        "notes": [],
        "next_steps": []
    }));

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
    let output = init_report(serde_json::json!({
        "ok": true,
        "command": "init",
        "render_mode": "copy",
        "destination": "/tmp/repo",
        "template": "embedded",
        "answers_file": ".jig.toml",
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
    }));

    let summary = format_init_human_summary(&output);

    assert_eq!(
        summary,
        concat!(
            "init summary\n",
            "  target: /tmp/repo\n",
            "  template: embedded\n",
            "  managed files: 2 created, 0 modified, 0 removed\n",
            "  scaffold: rust-react for demo (db: postgres)\n",
            "  scaffold files: 2 created, 0 modified, 1 unchanged\n",
            "  frontends: web, landing, admin-panel\n",
            "  git: initialized\n",
            "  notes:\n",
            "    - SQLx disabled by default until configured.\n",
            "  next steps:\n",
            "    - cd /tmp/repo\n",
            "    - scripts/jig setup\n",
            "  full report: rerun with --json\n",
        )
    );
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
