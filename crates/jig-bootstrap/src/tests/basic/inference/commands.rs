use super::*;

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
        components: Default::default(),
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
        components: Default::default(),
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
        components: Default::default(),
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
        components: Default::default(),
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
        components: Default::default(),
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
        components: Default::default(),
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
    assert!(answers.contains("api_test_command = \"cargo nextest run --workspace\""));
}
