use super::*;

#[path = "inference_assertions.rs"]
mod inference_assertions;
use inference_assertions::*;

#[test]
fn adopt_infers_repo_shape_before_resolving_answers() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("crates/api/src")).unwrap();
    fs::create_dir_all(repo.join("migrations")).unwrap();
    fs::create_dir_all(repo.join(".sqlx")).unwrap();
    fs::create_dir_all(repo.join("web")).unwrap();
    fs::create_dir_all(repo.join(".github/workflows")).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/*"]

[workspace.dependencies]
sqlx = "0.8"
"#,
    )
    .unwrap();
    fs::write(
        repo.join("crates/api/Cargo.toml"),
        r#"[package]
name = "api"
version = "0.1.0"
edition = "2024"

[dependencies]
sqlx = { workspace = true }
"#,
    )
    .unwrap();
    fs::write(repo.join("crates/api/src/lib.rs"), "sqlx::migrate!();").unwrap();
    fs::write(repo.join("migrations/0001_init.sql"), "select 1;").unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["web"]}"#,
    )
    .unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    fs::write(
        repo.join("web/package.json"),
        r#"{
  "name": "web",
  "scripts": {
    "dev": "vite --host 127.0.0.1",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}
"#,
    )
    .unwrap();
    fs::write(
        repo.join(".github/workflows/rust.yml"),
        "jobs:\n  test:\n    runs-on: ubuntu-24.04\n",
    )
    .unwrap();
    init_git_repo_for_test(&repo);
    git(
        &repo,
        [
            "remote",
            "add",
            "origin",
            "git@github.com:owner/inferred-demo.git",
        ],
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_inferred_detection(&output);
    assert_inferred_profile(&output);
    assert_inferred_gates(&output);
    assert_inferred_ownership(&output);
    let answers = assert_inferred_config(&repo);
    assert_rendered_work_gates(&output, &answers);
    assert!(!repo.join("crates/api/AGENTS.md").exists());
}

#[test]
fn adopt_reports_rust_crate_topology_and_skips_fixture_guides() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("crates/api/src")).unwrap();
    fs::create_dir_all(repo.join("crates/util/src")).unwrap();
    fs::create_dir_all(repo.join("crates/fixtures/src")).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/*"]
"#,
    )
    .unwrap();
    fs::write(
        repo.join("crates/api/Cargo.toml"),
        r#"[package]
name = "api"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(repo.join("crates/api/src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        repo.join("crates/util/Cargo.toml"),
        r#"[package]
name = "util"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(repo.join("crates/util/src/lib.rs"), "").unwrap();
    fs::write(repo.join("crates/util/AGENTS.md"), "# util guide\n").unwrap();
    fs::write(
        repo.join("crates/fixtures/Cargo.toml"),
        r#"[package]
name = "fixtures"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(repo.join("crates/fixtures/src/lib.rs"), "").unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    let crates = output["adoption_profile"]["repo_topology"]["rust_crates"]
        .as_array()
        .unwrap();
    let api = crates
        .iter()
        .find(|krate| krate["dir"] == "crates/api")
        .unwrap();
    assert_eq!(api["kind"], "binary");
    assert_eq!(api["role"], "app/service");
    assert_eq!(api["guide_action"], "missing_project_owned");
    let util = crates
        .iter()
        .find(|krate| krate["dir"] == "crates/util")
        .unwrap();
    assert_eq!(util["kind"], "library");
    assert_eq!(util["role"], "support");
    assert_eq!(util["guide_action"], "existing");
    assert_eq!(util["owner_guide"], "crates/util/AGENTS.md");
    let fixtures = crates
        .iter()
        .find(|krate| krate["dir"] == "crates/fixtures")
        .unwrap();
    assert_eq!(fixtures["role"], "example/fixture/test");
    assert_eq!(fixtures["guide_action"], "skip_non_production");
    assert!(
        fixtures["guide_action_reason"]
            .as_str()
            .unwrap()
            .contains("non-production")
    );
    assert!(!repo.join("crates/api/AGENTS.md").exists());
    assert!(!repo.join("crates/fixtures/AGENTS.md").exists());
}

#[test]
fn adopt_reports_sources_for_multiple_migration_dirs() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("crates/api/migrations")).unwrap();
    fs::create_dir_all(repo.join("migrations")).unwrap();
    fs::write(
        repo.join("crates/api/migrations/0001_api.sql"),
        "select 1;\n",
    )
    .unwrap();
    fs::write(repo.join("migrations/0001_root.sql"), "select 1;\n").unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(
        output["detection_report"]["rust_migration_dirs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|dir| dir.as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["crates/api/migrations", "migrations"]
    );
    let sources = output["detection_report"]["metadata"]["rust_migration_dirs"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|source| source.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        sources,
        vec![
            "crates/api/migrations/0001_api.sql",
            "migrations/0001_root.sql"
        ]
    );
    assert!(
        output["detection_report"]["metadata"]["rust_migration_dirs"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning
                .as_str()
                .unwrap()
                .contains("multiple migration directories detected"))
    );
}

#[test]
fn adopt_infers_rust_wrapper_commands_and_web_tool_hints() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("crates/api/src")).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/*"]
"#,
    )
    .unwrap();
    fs::write(
        repo.join("crates/api/Cargo.toml"),
        r#"[package]
name = "api"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(repo.join("crates/api/src/lib.rs"), "").unwrap();
    fs::write(
        repo.join("Justfile"),
        r"fmt-check:
    cargo fmt --all -- --check
clippy:
    cargo hack clippy --workspace --all-targets -- -D warnings
test:
    cargo nextest run --workspace
test-locked:
    cargo nextest run --workspace --locked
",
    )
    .unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{
  "private": true,
  "scripts": {
    "lint": "biome check . && eslint .",
    "test": "vitest run && playwright test",
    "build": "turbo run build",
    "graph": "nx graph"
  },
  "devDependencies": {
    "@biomejs/biome": "1.9.0",
    "@playwright/test": "1.0.0",
    "eslint": "9.0.0",
    "nx": "20.0.0",
    "turbo": "2.0.0",
    "vitest": "2.0.0"
  }
}
"#,
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(output["detection_report"]["rust_test_command"], "just test");
    assert_eq!(
        output["detection_report"]["metadata"]["rust_test_command"]["confidence"],
        "high"
    );
    assert!(
        output["adoption_profile"]["command_profile"]["rust"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "cargo-hack")
    );
    let web_tools = output["adoption_profile"]["command_profile"]["web"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    for expected in ["biome", "eslint", "nx", "playwright", "turbo", "vitest"] {
        assert!(web_tools.contains(&expected), "missing {expected}");
    }
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("api_fmt_command = \"just fmt-check\""));
    assert!(answers.contains("api_clippy_command = \"just clippy\""));
    assert!(answers.contains("api_test_command = \"just test\""));
    assert!(answers.contains("api_test_locked_command = \"just test-locked\""));
}

#[test]
fn adopt_merges_rust_wrapper_commands_across_wrapper_files() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(
        repo.join("Justfile"),
        r"clippy:
    cargo clippy --workspace --all-targets -- -D warnings
",
    )
    .unwrap();
    fs::write(
        repo.join("Makefile"),
        r"fmt-check:
	cargo fmt --all -- --check
test:
	cargo test --workspace
test-locked:
	cargo test --workspace --locked
",
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(
        output["detection_report"]["rust_fmt_check_command"],
        "make fmt-check"
    );
    assert_eq!(
        output["detection_report"]["rust_clippy_command"],
        "just clippy"
    );
    assert_eq!(output["detection_report"]["rust_test_command"], "make test");
    assert_eq!(
        output["detection_report"]["rust_test_locked_command"],
        "make test-locked"
    );
    let clippy_policy_warning = "cannot verify that it enforces `clippy::mod_module_files`";
    assert!(
        output["detection_report"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().unwrap().contains(clippy_policy_warning))
    );
    assert!(
        output["detection_report"]["metadata"]["rust_clippy_command"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().unwrap().contains(clippy_policy_warning))
    );
    assert!(
        output["detection_report"]["metadata"]["rust_fmt_check_command"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("multiple files"))
    );
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("api_fmt_command = \"make fmt-check\""));
    assert!(answers.contains("api_clippy_command = \"just clippy\""));
    assert!(answers.contains("api_test_command = \"make test\""));
    assert!(answers.contains("api_test_locked_command = \"make test-locked\""));
}

include!("inference_parts/part_02.rs");
