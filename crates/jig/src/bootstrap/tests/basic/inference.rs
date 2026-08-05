use super::*;

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

    assert_eq!(output["detection_report"]["repo_name"], "inferred-demo");
    assert_eq!(output["detection_report"]["rust_crate_roots"][0], "crates");
    assert_eq!(output["detection_report"]["sqlx_enabled"], true);
    assert_eq!(
        output["detection_report"]["rust_migration_dir"],
        "migrations"
    );
    assert_eq!(output["detection_report"]["web_package_manager"], "pnpm");
    assert_eq!(output["detection_report"]["frontend_apps"][0]["dir"], "web");
    assert_eq!(
        output["detection_report"]["metadata"]["sqlx_enabled"]["confidence"],
        "high"
    );
    assert!(
        output["detection_report"]["metadata"]["sqlx_enabled"]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source.as_str().unwrap().contains("workspace.dependencies"))
    );
    assert!(
        output["detection_report"]["metadata"]["sqlx_enabled"]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source.as_str() == Some("migrations/0001_init.sql"))
    );
    assert_eq!(
        output["adoption_profile"]["detected_stack"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["Rust workspace", "SQLx", "pnpm", "Vite", "GitHub Actions"]
    );
    assert_eq!(
        output["adoption_profile"]["ci_shape"]["workflow_files"][0],
        ".github/workflows/rust.yml"
    );
    assert_eq!(
        output["adoption_profile"]["ci_shape"]["generated_jig_checks_role"],
        "supplement_existing_ci"
    );
    assert!(
        !output["adoption_review"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str().unwrap().contains("overrides:"))
    );
    assert!(
        output["adoption_profile"]["generated_gates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gate| gate == "scripts/jig check sqlx")
    );
    assert!(
        !output["adoption_profile"]["generated_gates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gate| gate == "scripts/jig check schema")
    );
    assert!(
        output["adoption_profile"]["generated_gates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gate| gate == "scripts/jig check typescript-coverage")
    );
    assert!(
        output["adoption_profile"]["generated_gates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|gate| gate.as_str().unwrap().starts_with("scripts/jig "))
    );
    assert!(
        output["render_report"]["commands_detected_or_skipped"]
            .as_array()
            .unwrap()
            .iter()
            .all(|command| command.as_str().unwrap().contains("scripts/jig"))
    );
    assert!(
        output["adoption_profile"]["managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == ".jig.toml")
    );
    assert!(
        !output["adoption_profile"]["managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "scripts/check-agent-guides.sh")
    );
    assert!(
        !output["adoption_profile"]["retired_managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "scripts/check-agent-guides.sh")
    );
    assert!(
        !output["adoption_profile"]["retired_managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == ".jig.toml")
    );
    assert!(
        output["adoption_profile"]["assumptions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|assumption| assumption
                .as_str()
                .unwrap()
                .contains("online cargo sqlx prepare"))
    );
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"inferred-demo\""));
    assert!(answers.contains("default_branch = \"main\""));
    assert!(answers.contains("ci_github_runner = \"ubuntu-24.04\""));
    assert!(answers.contains("sqlx_enabled = true"));
    assert!(answers.contains("rust_crate_roots = [\"crates\"]"));
    assert!(answers.contains("rust_migration_dir = \"migrations\""));
    assert!(answers.contains("rust_sqlx_metadata_dir = \".sqlx\""));
    assert!(answers.contains("schema_dump_enabled = false"));
    assert!(!answers.contains("schema_dump_command"));
    assert!(answers.contains("sqlx_check_command = "));
    assert!(answers.contains("cargo sqlx prepare --check"));
    assert!(answers.contains("web_package_manager = \"pnpm\""));
    assert!(answers.contains("[[frontend_apps]]"));
    assert!(answers.contains("name = \"web\""));
    assert!(answers.contains("dir = \"web\""));
    assert!(answers.contains("argv = [\"pnpm\", \"run\", \"dev\"]"));
    let generated_gates = output["adoption_profile"]["generated_gates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|gate| gate.as_str().unwrap())
        .collect::<Vec<_>>();
    let rendered_work_gate_tools = answers
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("tool = \"")
                .and_then(|value| value.strip_suffix('"'))
        })
        .collect::<Vec<_>>();
    for tool in rendered_work_gate_tools {
        let expected = match tool {
            "jig.contract_check" => "scripts/jig check contract",
            "jig.test" => "scripts/jig check test",
            "jig.typescript_lint" => "scripts/jig check typescript-lint",
            "jig.typescript_typecheck" => "scripts/jig check typescript-typecheck",
            "jig.typescript_build" => "scripts/jig check typescript-build",
            "jig.typescript_coverage" => "scripts/jig check typescript-coverage",
            "jig.sqlx_check" => "scripts/jig check sqlx",
            "jig.schema_check" => "scripts/jig check schema",
            "jig.schema_dump" => "scripts/jig sqlx schema dump",
            other => panic!("unmapped rendered work gate tool {other}"),
        };
        assert!(
            generated_gates.contains(&expected),
            "generated_gates missing rendered work gate command {expected}"
        );
    }
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
    assert!(answers.contains("rust_fmt_check_command = \"just fmt-check\""));
    assert!(answers.contains("rust_clippy_command = \"just clippy\""));
    assert!(answers.contains("rust_test_command = \"just test\""));
    assert!(answers.contains("rust_test_locked_command = \"just test-locked\""));
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
    assert!(
        output["detection_report"]["metadata"]["rust_fmt_check_command"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("multiple files"))
    );
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("rust_fmt_check_command = \"make fmt-check\""));
    assert!(answers.contains("rust_clippy_command = \"just clippy\""));
    assert!(answers.contains("rust_test_command = \"make test\""));
    assert!(answers.contains("rust_test_locked_command = \"make test-locked\""));
}

#[test]
fn adopt_infers_just_recipes_with_default_arguments() {
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
        r#"test target="all":
    cargo test --workspace {{target}}
"#,
    )
    .unwrap();

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

    assert_eq!(output["detection_report"]["rust_test_command"], "just test");
}

#[test]
fn adopt_warns_when_wrapper_test_pairs_with_nextest_locked_command() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".config")).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(repo.join(".config/nextest.toml"), "[profile.default]\n").unwrap();
    fs::write(
        repo.join("Justfile"),
        r"test:
    cargo test --workspace
",
    )
    .unwrap();

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

    assert_eq!(output["detection_report"]["rust_test_command"], "just test");
    assert_eq!(
        output["detection_report"]["rust_test_locked_command"],
        "cargo nextest run --workspace --locked"
    );
    assert!(
        output["detection_report"]["metadata"]["rust_test_locked_command"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("different runners"))
    );
}

#[test]
fn adopt_ignores_make_assignments_that_look_like_rust_recipes() {
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
        repo.join("Makefile"),
        r"test := cargo test --workspace
fmt-check:
	cargo fmt --all -- --check
",
    )
    .unwrap();

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
        output["detection_report"]["rust_fmt_check_command"],
        "make fmt-check"
    );
    assert!(output["detection_report"]["rust_test_command"].is_null());
}

#[test]
fn adopt_infers_nextest_when_no_project_wrapper_exists() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".config")).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(repo.join(".config/nextest.toml"), "[profile.default]\n").unwrap();

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
        output["detection_report"]["rust_test_command"],
        "cargo nextest run --workspace"
    );
    assert_eq!(
        output["detection_report"]["rust_test_locked_command"],
        "cargo nextest run --workspace --locked"
    );
    assert_eq!(
        output["detection_report"]["metadata"]["rust_test_command"]["sources"][0],
        ".config/nextest.toml"
    );
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("rust_test_command = \"cargo nextest run --workspace\""));
}

#[test]
fn adopt_keeps_explicit_answers_ahead_of_inference() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("web")).unwrap();
    fs::write(repo.join("package-lock.json"), "{}").unwrap();
    fs::write(
        repo.join("web/package.json"),
        r#"{
  "name": "web",
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}
"#,
    )
    .unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
sqlx_enabled = false
web_package_manager = "yarn"
rust_test_command = "cargo test --workspace"
frontend_apps = []
"#,
    )
    .unwrap();
    fs::write(
        repo.join("Justfile"),
        r"test:
    cargo nextest run --workspace
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
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            repo_name: Some("from-cli".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(
        output["adoption_profile"]["overrides"]
            .as_array()
            .unwrap()
            .iter()
            .any(|override_note| override_note
                .as_str()
                .unwrap()
                .contains("web_package_manager: inferred npm ignored"))
    );
    assert!(
        output["adoption_profile"]["overrides"]
            .as_array()
            .unwrap()
            .iter()
            .any(|override_note| override_note
                .as_str()
                .unwrap()
                .contains("frontend_apps: inferred web ignored"))
    );
    assert!(
        output["adoption_profile"]["overrides"]
            .as_array()
            .unwrap()
            .iter()
            .any(|override_note| override_note
                .as_str()
                .unwrap()
                .contains("rust_test_command: inferred just test ignored"))
    );

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"from-cli\""));
    assert!(answers.contains("web_package_manager = \"yarn\""));
    assert!(answers.contains("rust_test_command = \"cargo test --workspace\""));
    assert!(answers.contains("frontend_apps = []"));
    assert!(!answers.contains("[[frontend_apps]]"));
}

#[test]
fn adopt_answer_file_migration_dir_keeps_sqlx_enabled_when_inference_finds_no_sqlx() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
rust_migration_dir = "migrations"
"#,
    )
    .unwrap();

    run_adopt(AdoptOpts {
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
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = true"));
    assert!(answers.contains("rust_migration_dir = \"migrations\""));
    assert!(answers.contains("schema_dump_enabled = false"));
    assert!(!answers.contains("schema_dump_command"));
}

#[test]
fn adopt_answer_file_sqlx_disabled_suppresses_inferred_migration_defaults() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("migrations")).unwrap();
    fs::write(repo.join("migrations/0001_init.sql"), "select 1;").unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
sqlx_enabled = false
"#,
    )
    .unwrap();

    run_adopt(AdoptOpts {
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
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
    assert!(!answers.contains("rust_migration_dir ="));
}

#[test]
fn adopt_answer_file_schema_dump_disabled_still_uses_inferred_no_sqlx_profile() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
schema_dump_enabled = false
"#,
    )
    .unwrap();

    run_adopt(AdoptOpts {
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
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
    assert!(answers.contains("schema_dump_enabled = false"));
}

#[test]
fn adopt_answer_file_schema_dump_enabled_blocks_inferred_no_sqlx_profile() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
schema_dump_enabled = true
"#,
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
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
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Missing required answer when sqlx_enabled is true"));
    assert!(error.contains("schema_dump_enabled implies SQLx"));
    assert!(error.contains("--rust-migration-dir <dir>"));
}

#[test]
fn adopt_cli_sqlx_metadata_dir_blocks_inferred_no_sqlx_profile() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let error = run_adopt(AdoptOpts {
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
        answers: AnswerOpts {
            rust_sqlx_metadata_dir: Some(".sqlx".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Missing required answer when sqlx_enabled is true"));
    assert!(error.contains("--rust-migration-dir <dir>"));
}

#[test]
fn adopt_infers_root_frontend_app() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("root-web");
    fs::create_dir_all(&repo).unwrap();
    fs::write(repo.join("package-lock.json"), "{}").unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{
  "name": "root-web",
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}
"#,
    )
    .unwrap();

    run_adopt(AdoptOpts {
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

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
    assert!(answers.contains("web_package_manager = \"npm\""));
    assert!(answers.contains("name = \"root-web\""));
    assert!(answers.contains("dir = \".\""));
    assert!(answers.contains("kind = \"vite\""));
    assert!(answers.contains(
        "argv = [\"npm\", \"--prefix=.\", \"--workspace=.\", \"--workspaces=true\", \"--include-workspace-root=true\", \"--global=false\", \"--location=project\", \"--if-present=false\", \"--include=dev\", \"--include=optional\", \"--include=peer\", \"run\", \"dev\"]"
    ));
}

#[test]
fn adopt_defaults_with_migration_dir_keeps_sqlx_enabled() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            rust_migration_dir: Some("migrations".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = true"));
    assert!(answers.contains("rust_migration_dir = \"migrations\""));
    assert!(answers.contains("schema_dump_enabled = false"));
    assert!(!answers.contains("schema_dump_command"));
}

#[test]
fn adopt_schema_dump_command_opts_into_schema_dumps() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            rust_migration_dir: Some("migrations".into()),
            schema_dump_command: Some("scripts/custom-dump-schema.sh".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = true"));
    assert!(answers.contains("schema_dump_enabled = true"));
    assert!(answers.contains("schema_dump_command = \"scripts/custom-dump-schema.sh\""));
}

#[test]
fn adopt_defaults_with_schema_dump_enabled_still_requires_sqlx_migration_answer() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            schema_dump_enabled: Some(true),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Missing required answer when sqlx_enabled is true"));
    assert!(error.contains("--rust-migration-dir <dir>"));
}

#[test]
fn adopt_no_input_without_defaults_uses_inferred_no_sqlx_profile() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(AdoptOpts {
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

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
}
