use super::*;

#[test]
fn initial_notes_cover_review_and_available_checks() {
    let notes = initial_notes(Vec::new(), true, None, false, true);
    for expected in [
        "Review generated .jig.toml",
        "scripts/jig check typescript-lint",
        "scripts/jig check contract",
    ] {
        assert!(notes.iter().any(|note| note.contains(expected)));
    }
    assert!(notes.iter().any(|note| note.contains("file-budget audit")));
    assert!(
        !initial_notes(Vec::new(), false, None, true, true)
            .iter()
            .any(|note| note.contains("file-budget audit"))
    );
}

#[test]
fn initial_notes_cover_review_and_available_checks_for_minimal_init() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("minimal");
    let answers_file = temp.path().join("minimal-answers.toml");
    let mut authored = authored_mixed_repository_config();
    let table = authored.as_table_mut().unwrap();
    table.insert(
        "repo_name".into(),
        toml::Value::String("ExampleProject".into()),
    );
    table.insert(
        "harness_footprint".into(),
        toml::Value::String("minimal".into()),
    );
    table.insert("sqlx_enabled".into(), toml::Value::Boolean(false));
    table.insert("schema_dump_enabled".into(), toml::Value::Boolean(false));
    authored["commands"].as_table_mut().unwrap().insert(
        "budget_check_command".into(),
        toml::Value::String("true".into()),
    );
    let budget_action: toml::Value = toml::from_str(
        r#"[[actions]]
target = { component = "repo", action = "file-budget" }
intent = "check"
effects = ["read_only"]
runner = { kind = "command", command = "budget_check_command" }
"#,
    )
    .unwrap();
    authored["repository"]["actions"]
        .as_array_mut()
        .unwrap()
        .push(budget_action["actions"][0].clone());
    fs::write(&answers_file, toml::to_string_pretty(&authored).unwrap()).unwrap();

    let output = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(!destination.join(".jig/file-budget.toml").exists());
    assert!(!destination.join("scripts/jig").exists());
    assert!(!output["notes"].to_string().contains("file-budget audit"));
}

#[test]
fn initial_notes_cover_review_and_available_checks_when_policy_seed_is_empty() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");
    let answers_file = temp.path().join("answers.toml");
    let mut authored = authored_mixed_repository_config();
    let table = authored.as_table_mut().unwrap();
    table.insert(
        "repo_name".into(),
        toml::Value::String("ExampleProject".into()),
    );
    table.insert("sqlx_enabled".into(), toml::Value::Boolean(false));
    table.insert("schema_dump_enabled".into(), toml::Value::Boolean(false));
    authored["commands"].as_table_mut().unwrap().insert(
        "budget_check_command".into(),
        toml::Value::String("true".into()),
    );
    authored["repository"]["components"]
        .as_array_mut()
        .unwrap()
        .retain(|component| component["id"].as_str() == Some("repo"));
    authored["repository"]["actions"]
        .as_array_mut()
        .unwrap()
        .retain(|action| action["target"]["component"].as_str() == Some("repo"));
    for profile in authored["repository"]["profiles"].as_array_mut().unwrap() {
        profile["targets"]
            .as_array_mut()
            .unwrap()
            .retain(|target| target["component"].as_str() == Some("repo"));
    }
    let budget_action: toml::Value = toml::from_str(
        r#"[[actions]]
target = { component = "repo", action = "file-budget" }
intent = "check"
effects = ["read_only"]
runner = { kind = "command", command = "budget_check_command" }
"#,
    )
    .unwrap();
    authored["repository"]["actions"]
        .as_array_mut()
        .unwrap()
        .push(budget_action["actions"][0].clone());
    fs::write(&answers_file, toml::to_string_pretty(&authored).unwrap()).unwrap();

    let output = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(
        fs::read(destination.join(".jig/file-budget.toml"))
            .unwrap()
            .is_empty()
    );
    assert!(!output["notes"].to_string().contains("file-budget audit"));
    let guide = fs::read_to_string(destination.join("AGENTS.md")).unwrap();
    assert!(!guide.contains("file-budget audit"));
}

#[test]
fn initial_notes_cover_review_and_available_checks_in_scaffold_readmes() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let scaffold = |name: &str, preset: ScaffoldPreset, answers: AnswerOpts| {
        let destination = temp.path().join(name);
        run_init(InitOpts {
            path: destination.clone(),
            scaffold: ScaffoldOpts {
                preset: Some(preset),
                ..ScaffoldOpts::default()
            },
            template: Some(template.path().display().to_string()),
            template_mode: None,
            vcs_ref: None,
            force: false,
            defaults: false,
            no_input: true,
            no_vault: true,
            answers,
        })
        .unwrap();
        destination
    };
    let with_policy = scaffold(
        "with-policy",
        ScaffoldPreset::RustReact,
        AnswerOpts {
            repo_name: Some("ExampleProject".into()),
            ..AnswerOpts::default()
        },
    );
    let answers_file = temp.path().join("no-budget-answers.toml");
    let mut authored: toml::Value =
        toml::from_str(&fs::read_to_string(with_policy.join(".jig.toml")).unwrap()).unwrap();
    authored["repository"]["actions"]
        .as_array_mut()
        .unwrap()
        .retain(|action| {
            action["target"]["component"].as_str() != Some("repo")
                || action["target"]["action"].as_str() != Some("file-budget")
        });
    for profile in authored["repository"]["profiles"].as_array_mut().unwrap() {
        profile["targets"].as_array_mut().unwrap().retain(|target| {
            target["component"].as_str() != Some("repo")
                || target["action"].as_str() != Some("file-budget")
        });
    }
    fs::write(&answers_file, toml::to_string_pretty(&authored).unwrap()).unwrap();
    let no_policy = scaffold(
        "no-policy",
        ScaffoldPreset::RustReact,
        AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    );
    let rust_only = scaffold(
        "rust-only",
        ScaffoldPreset::RustLibrary,
        AnswerOpts {
            repo_name: Some("ExampleProject".into()),
            ..AnswerOpts::default()
        },
    );

    for (name, destination) in [
        ("no-policy", no_policy),
        ("with-policy", with_policy),
        ("rust-only", rust_only),
    ] {
        let policy = destination.join(".jig/file-budget.toml").exists();
        let readme = fs::read_to_string(destination.join("README.md")).unwrap();
        assert_eq!(
            readme.contains("scripts/jig file-budget audit"),
            policy,
            "{name}"
        );
        let guide = fs::read_to_string(destination.join("AGENTS.md")).unwrap();
        assert_eq!(
            guide.contains("scripts/jig file-budget audit"),
            policy,
            "{name}"
        );
        assert_eq!(policy, name != "no-policy", "{name}");
    }
}

#[test]
fn initial_notes_cover_review_and_available_checks_for_custom_template_without_usable_policy() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let expired_policy = r#"version=1
[[rules]]
id="source"
include=["**/*.rs"]
max_lines=10
[[waivers]]
id="legacy"
rule="source"
path="src/legacy.rs"
ceiling_lines=20
reason="tracked"
expires=2001-01-01
"#;
    for (name, policy_contents) in [
        ("missing-policy", None),
        ("empty-policy", Some("")),
        ("expired-policy", Some(expired_policy)),
    ] {
        let template = materialize_template_worktree();
        let policy_template = template
            .path()
            .join("templates/project/.jig/file-budget.toml.jinja");
        match policy_contents {
            Some(contents) => fs::write(&policy_template, contents).unwrap(),
            None => fs::remove_file(&policy_template).unwrap(),
        }
        let destination = temp.path().join(name);
        let output = run_init(InitOpts {
            path: destination.clone(),
            scaffold: ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                ..ScaffoldOpts::default()
            },
            template: Some(template.path().display().to_string()),
            template_mode: None,
            vcs_ref: None,
            force: false,
            defaults: false,
            no_input: true,
            no_vault: true,
            answers: AnswerOpts {
                repo_name: Some("ExampleProject".into()),
                ..AnswerOpts::default()
            },
        })
        .unwrap();
        let policy = destination.join(".jig/file-budget.toml");
        if name == "expired-policy" {
            let rendered = fs::read(&policy).unwrap();
            let historical_date = jig_file_budget::PolicyDateV1::new(2000, 1, 1).unwrap();
            assert!(jig_file_budget::parse_policy_v1(&rendered, historical_date).is_ok());
        } else {
            assert!(!policy.exists() || fs::read(&policy).unwrap().is_empty());
        }
        assert!(!output["notes"].to_string().contains("file-budget audit"));
        for path in ["AGENTS.md", "README.md"] {
            let content = fs::read_to_string(destination.join(path)).unwrap();
            assert!(
                !content.contains("scripts/jig file-budget audit"),
                "{name}: {path}"
            );
        }
    }
}
