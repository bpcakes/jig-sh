use super::*;
use crate::context::RepoContext;
use crate::policy::{PolicyCheckCommand, run_check};

#[test]
fn update_preserves_explicit_rust_guide_roots() {
    assert_preserves_guide_roots(false);
}

#[test]
fn recopy_preserves_explicit_rust_guide_roots() {
    assert_preserves_guide_roots(true);
}

fn assert_preserves_guide_roots(recopy: bool) {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("ExampleProject");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        "[workspace]\nmembers = [\"apps/*\", \"crates/*\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    let valid_guide = "## Purpose\nExample backend.\n## Key entrypoints\n`src/lib.rs`\n## Edit here for X\n## Invariants\n## Common commands\n";
    for path in ["apps/api", "apps/worker", "crates/core"] {
        let root = repo.join(path);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("AGENTS.md"), valid_guide).unwrap();
        fs::write(root.join("src/lib.rs"), "").unwrap();
        fs::write(
            root.join("Cargo.toml"),
            format!(
                "[package]\nname = \"example-{}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                root.file_name().unwrap().to_str().unwrap()
            ),
        )
        .unwrap();
    }
    for path in ["web", "docs"] {
        fs::create_dir_all(repo.join(path)).unwrap();
        fs::write(repo.join(path).join("AGENTS.md"), "Non-Rust guidance.\n").unwrap();
    }
    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    let answers_path = repo.join(".jig.toml");
    let mut answers = read_answers_toml(&answers_path).unwrap();
    answers.insert(
        "repo_name".into(),
        TomlValue::String("ExampleProject".into()),
    );
    answers.insert(
        "rust_crate_roots".into(),
        TomlValue::Array(vec!["apps".into(), "crates".into()]),
    );
    // Model a backend whose execution component owns the root workspace,
    // independently of the directories used to discover backend guides.
    let repo_component = answers["repository"]["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|component| component["id"].as_str() == Some("repo"))
        .unwrap()
        .clone();
    let repo_actions = answers["repository"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|action| action["target"]["component"].as_str() == Some("repo"))
        .cloned()
        .collect::<Vec<_>>();
    answers.insert(
        "repository".into(),
        toml::from_str::<TomlValue>(
            r#"
default_check_profile = "verify"
[[components]]
id = "api"
root = "."
adapters = ["rust"]
[[actions]]
target = { component = "api", action = "test" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "example_check_command" }
[[profiles]]
id = "verify"
targets = [{ component = "api", action = "test" }]
"#,
        )
        .unwrap(),
    );
    answers["repository"]["components"]
        .as_array_mut()
        .unwrap()
        .push(repo_component);
    answers["repository"]["actions"]
        .as_array_mut()
        .unwrap()
        .extend(repo_actions);
    answers["commands"]
        .as_table_mut()
        .unwrap()
        .insert("example_check_command".into(), "true".into());
    write_answers_toml(&answers_path, &answers).unwrap();
    assert_guides(&repo, true);
    for invalid in [false, true] {
        if invalid {
            fs::write(
                repo.join("apps/worker/AGENTS.md"),
                "Invalid backend guide.\n",
            )
            .unwrap();
        }
        run_update(UpdateOpts {
            path: repo.clone(),
            template: None,
            template_mode: None,
            recopy,
            launcher_only: false,
            force: true,
            vcs_ref: None,
            defaults: true,
            no_input: true,
        })
        .unwrap();
        let rendered = read_answers_toml(&answers_path).unwrap();
        assert_eq!(rendered["rust_crate_roots"], answers["rust_crate_roots"]);
        assert_guides(&repo, !invalid);
    }
}

fn assert_guides(repo: &Path, valid: bool) {
    let ctx = RepoContext::load_from(repo).unwrap();
    let result = run_check(&ctx, PolicyCheckCommand::AgentGuides).unwrap();
    assert_eq!(result["guide_count"], 3, "{result}");
    assert_eq!(result["ok"], valid, "{result}");
    if !valid {
        assert_eq!(
            result["missing_entry_ref"],
            serde_json::json!([
                "apps/worker/AGENTS.md: missing src/lib.rs or src/main.rs entrypoint reference"
            ])
        );
    }
}
