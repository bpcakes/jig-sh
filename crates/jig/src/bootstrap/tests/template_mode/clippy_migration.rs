use super::*;

#[test]
fn update_migrates_generated_clippy_command_and_preserves_customization() {
    assert_update_migrates_generated_clippy_command(false);
}

#[test]
fn update_recopy_migrates_generated_clippy_command_and_preserves_customization() {
    assert_update_migrates_generated_clippy_command(true);
}

fn assert_update_migrates_generated_clippy_command(recopy: bool) {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    let answers_path = repo.join(".jig.toml");
    let mut answers = read_answers_toml(&answers_path).unwrap();
    answers.insert("schema_dump_enabled".into(), TomlValue::Boolean(true));
    for (key, command) in [
        ("bootstrap_command", "cargo fetch"),
        ("rust_fmt_check_command", "cargo fmt --all -- --check"),
        (
            "rust_clippy_command",
            "cargo clippy --workspace --all-targets --locked -- -D warnings",
        ),
        ("rust_test_command", "cargo test --workspace"),
        (
            "rust_test_locked_command",
            "cargo test --workspace --locked",
        ),
    ] {
        answers.insert(key.into(), TomlValue::String(command.into()));
    }
    let clippy_command_key = find_clippy_command_key(&answers);
    let commands = answers["commands"].as_table_mut().unwrap();
    let legacy_clippy_command = commands[&clippy_command_key]
        .as_str()
        .unwrap()
        .replace("--all-features ", "")
        .replace(" -D warnings -D clippy::mod_module_files", " -D warnings");
    commands.insert(clippy_command_key, TomlValue::String(legacy_clippy_command));
    write_answers_toml(&answers_path, &answers).unwrap();

    let output = run_update_with_recopy(repo.clone(), recopy);
    assert_all_features_migration_warning(&output);

    let answers_text = fs::read_to_string(&answers_path).unwrap();
    for expected in [
        "sqlx_enabled = false",
        "schema_dump_enabled = false",
        "No Cargo.toml found; skipping cargo bootstrap.",
        "No Cargo.toml found; skipping cargo fmt.",
        "No Cargo.toml found; skipping cargo clippy.",
        "--all-targets --all-features --locked",
        "-D warnings -D clippy::mod_module_files",
        "No Cargo.toml found; skipping cargo test.",
        "No Cargo.toml found; skipping cargo test-locked.",
    ] {
        assert!(answers_text.contains(expected), "{answers_text}");
    }
    assert!(!answers_text.contains("tool = \"jig.schema_check\""));

    let mut answers = toml::from_str::<TomlValue>(&answers_text).unwrap();
    let clippy_command_key = find_clippy_command_key(answers.as_table().unwrap());
    let generated = answers["commands"][&clippy_command_key].as_str().unwrap();
    assert!(generated.contains("-D warnings -D clippy::mod_module_files"));

    // Removing only --all-features is the documented policy-preserving opt-out.
    downgrade_clippy_commands(answers.as_table_mut().unwrap(), "--all-features ", "");
    write_answers_toml(&answers_path, answers.as_table().unwrap()).unwrap();
    let output = run_update_with_recopy(repo.clone(), recopy);
    let mut answers = read_answers_toml(&answers_path).unwrap();
    assert_policy_opt_out_is_preserved(&answers, &clippy_command_key);
    assert_no_all_features_migration_warning(&output);

    let generated = answers["commands"][&clippy_command_key].as_str().unwrap();
    let custom_clippy_command = generated.replace("; else", " --custom; else");
    answers["commands"].as_table_mut().unwrap().insert(
        clippy_command_key.clone(),
        TomlValue::String(custom_clippy_command.clone()),
    );
    write_answers_toml(&answers_path, &answers).unwrap();

    let output = run_update_with_recopy(repo, recopy);
    assert_no_unverified_clippy_policy_warning(&output);

    let answers = read_answers_toml(&answers_path).unwrap();
    assert_eq!(
        answers["commands"][&clippy_command_key].as_str(),
        Some(custom_clippy_command.as_str())
    );
}

#[test]
fn update_warns_without_rewriting_custom_clippy_runner_missing_policy_lint() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);

    let answers_path = repo.join(".jig.toml");
    let mut answers = read_answers_toml(&answers_path).unwrap();
    let clippy_command_key = find_clippy_command_key(&answers);
    let custom_command = "scripts/check-clippy";
    answers["commands"]
        .as_table_mut()
        .unwrap()
        .insert(clippy_command_key.clone(), custom_command.into());
    write_answers_toml(&answers_path, &answers).unwrap();

    for _ in 0..2 {
        let output = run_update_with_recopy(repo.clone(), false);
        assert_unverified_clippy_policy_warning(&output, &clippy_command_key);
        let answers = read_answers_toml(&answers_path).unwrap();
        assert_eq!(
            answers["commands"][&clippy_command_key].as_str(),
            Some(custom_command)
        );
    }
}

#[test]
fn update_migrates_renamed_clippy_action_through_its_capability_alias() {
    assert_update_migrates_renamed_clippy_action(false);
}

#[test]
fn update_recopy_migrates_renamed_clippy_action_through_its_capability_alias() {
    assert_update_migrates_renamed_clippy_action(true);
}

fn assert_update_migrates_renamed_clippy_action(recopy: bool) {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);

    let answers_path = repo.join(".jig.toml");
    let mut answers = read_answers_toml(&answers_path).unwrap();
    let original_key = find_clippy_command_key(&answers);
    let action = answers["repository"]["actions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|action| action_exposes_clippy(action))
        .unwrap();
    let component = action["target"]["component"].as_str().unwrap().to_owned();
    action["runner"]["command"] = "custom_clippy_runner_command".into();
    rename_repository_target_action(&mut answers, &component, "clippy", "lint-rust");
    let commands = answers["commands"].as_table_mut().unwrap();
    commands.remove(&original_key);
    commands.insert(
        "custom_clippy_runner_command".into(),
        "cargo clippy --workspace --all-targets --locked -- -D warnings".into(),
    );
    write_answers_toml(&answers_path, &answers).unwrap();

    let output = run_update_with_recopy(repo, recopy);
    assert_all_features_migration_warning(&output);
    let answers = read_answers_toml(&answers_path).unwrap();
    assert_eq!(
        find_clippy_command_key(&answers),
        "custom_clippy_runner_command"
    );
    assert_eq!(
        answers["commands"]["custom_clippy_runner_command"].as_str(),
        Some(crate::bootstrap::clippy_policy::DEFAULT_RUST_CLIPPY_COMMAND)
    );
    assert!(
        answers["repository"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| {
                action_exposes_clippy(action)
                    && action["target"]["action"].as_str() == Some("lint-rust")
            })
    );
}

#[test]
fn readoption_with_missing_managed_manifest_migrates_generated_clippy_command() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);

    let answers_path = repo.join(".jig.toml");
    let mut answers = read_answers_toml(&answers_path).unwrap();
    let clippy_command_key = find_clippy_command_key(&answers);
    answers["commands"].as_table_mut().unwrap().insert(
        clippy_command_key.clone(),
        TomlValue::String("cargo clippy --workspace --all-targets --locked -- -D warnings".into()),
    );
    write_answers_toml(&answers_path, &answers).unwrap();
    fs::remove_file(repo.join(managed_paths::MANIFEST_PATH)).unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let warning = output["notes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(serde_json::Value::as_str)
        .find(|note| note.contains("Updated exact generated Clippy input"))
        .unwrap();
    assert!(warning.contains("[commands]"), "{warning}");
    assert!(warning.contains("exposing `jig.clippy`"), "{warning}");
    let answers = read_answers_toml(&answers_path).unwrap();
    assert_generated_commands_are_current(&answers, &clippy_command_key);
    assert!(answers.get("rust_clippy_command").is_none());
}

#[test]
fn update_migrates_generated_nested_manifest_clippy_command_and_preserves_customization() {
    assert_update_migrates_generated_nested_manifest_clippy_command(false);
}

#[test]
fn update_recopy_migrates_generated_nested_manifest_clippy_command_and_preserves_customization() {
    assert_update_migrates_generated_nested_manifest_clippy_command(true);
}

fn assert_update_migrates_generated_nested_manifest_clippy_command(recopy: bool) {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("api/src")).unwrap();
    fs::write(
        repo.join("api/Cargo.toml"),
        "[package]\nname = \"example-api\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(repo.join("api/src/lib.rs"), "pub fn example() {}\n").unwrap();
    init_git_repo_for_test(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    let answers_path = repo.join(".jig.toml");
    let mut answers = read_answers_toml(&answers_path).unwrap();
    let clippy_command_key = find_clippy_command_key(&answers);
    let nested = answers["commands"][&clippy_command_key]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        nested.contains("--manifest-path \"$jig_manifest\""),
        "{nested}"
    );
    answers.insert("rust_clippy_command".into(), TomlValue::String(nested));
    assert_generated_commands_are_current(&answers, &clippy_command_key);

    downgrade_clippy_commands(
        &mut answers,
        "--all-features -- -D warnings -D clippy::mod_module_files",
        "-- -D warnings",
    );
    write_answers_toml(&answers_path, &answers).unwrap();
    let output = run_update_with_recopy(repo.clone(), recopy);
    assert_all_features_migration_warning(&output);

    let mut answers = read_answers_toml(&answers_path).unwrap();
    assert_generated_commands_are_current(&answers, &clippy_command_key);

    downgrade_clippy_commands(&mut answers, "--all-features ", "");
    write_answers_toml(&answers_path, &answers).unwrap();
    let output = run_update_with_recopy(repo.clone(), recopy);
    let mut answers = read_answers_toml(&answers_path).unwrap();
    assert_policy_opt_out_is_preserved(&answers, &clippy_command_key);
    assert_no_all_features_migration_warning(&output);

    let customized = answers["commands"][&clippy_command_key]
        .as_str()
        .unwrap()
        .replace(" || rc=$?", " --custom || rc=$?");
    answers["commands"].as_table_mut().unwrap().insert(
        clippy_command_key.clone(),
        TomlValue::String(customized.clone()),
    );
    write_answers_toml(&answers_path, &answers).unwrap();
    run_update_with_recopy(repo, recopy);

    let answers = read_answers_toml(&answers_path).unwrap();
    assert_eq!(
        answers["commands"][&clippy_command_key].as_str(),
        Some(customized.as_str())
    );
}

fn downgrade_clippy_commands(answers: &mut toml::Table, from: &str, to: &str) {
    let clippy_command_key = find_clippy_command_key(answers);
    let mapped_command = answers["commands"][&clippy_command_key]
        .as_str()
        .unwrap()
        .to_owned();
    let scalar_command = answers
        .get("rust_clippy_command")
        .and_then(TomlValue::as_str)
        .unwrap_or(&mapped_command)
        .to_owned();
    assert!(scalar_command.contains(from), "{scalar_command}");
    assert!(mapped_command.contains(from), "{mapped_command}");
    let scalar = scalar_command.replace(from, to);
    let mapped = mapped_command.replace(from, to);
    answers.insert("rust_clippy_command".into(), TomlValue::String(scalar));
    answers["commands"]
        .as_table_mut()
        .unwrap()
        .insert(clippy_command_key, TomlValue::String(mapped));
}

fn assert_generated_commands_are_current(answers: &toml::Table, clippy_command_key: &str) {
    let mut commands = vec![answers["commands"][clippy_command_key].as_str().unwrap()];
    if let Some(command) = answers
        .get("rust_clippy_command")
        .and_then(TomlValue::as_str)
    {
        commands.push(command);
    }
    for command in commands {
        assert!(
            command.contains("--all-targets --all-features"),
            "{command}"
        );
        assert!(
            command.contains("-D warnings -D clippy::mod_module_files"),
            "{command}"
        );
    }
}

fn assert_policy_opt_out_is_preserved(answers: &toml::Table, clippy_command_key: &str) {
    let mut commands = vec![answers["commands"][clippy_command_key].as_str().unwrap()];
    if let Some(command) = answers
        .get("rust_clippy_command")
        .and_then(TomlValue::as_str)
    {
        commands.push(command);
    }
    for command in commands {
        assert!(!command.contains("--all-features"), "{command}");
        assert!(
            command.contains("-D warnings -D clippy::mod_module_files"),
            "{command}"
        );
    }
}

fn assert_all_features_migration_warning(output: &serde_json::Value) {
    assert!(
        output["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(serde_json::Value::as_str)
            .any(
                |warning| warning.contains("Updated exact generated Clippy input")
                    && warning.contains("--all-features")
                    && warning.contains("commands.")
                    && warning.contains("review every command")
            ),
        "{output}"
    );
}

fn assert_no_all_features_migration_warning(output: &serde_json::Value) {
    assert!(
        output["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(serde_json::Value::as_str)
            .all(|warning| !warning.contains("Updated exact generated Clippy input")),
        "{output}"
    );
}

fn assert_unverified_clippy_policy_warning(output: &serde_json::Value, command_key: &str) {
    assert!(
        output["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(serde_json::Value::as_str)
            .any(|warning| {
                warning.contains("Could not verify `clippy::mod_module_files`")
                    && warning.contains(&format!("commands.{command_key}"))
                    && warning.contains("were preserved")
            }),
        "{output}"
    );
}

fn assert_no_unverified_clippy_policy_warning(output: &serde_json::Value) {
    assert!(
        output["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(serde_json::Value::as_str)
            .all(|warning| !warning.contains("Could not verify `clippy::mod_module_files`")),
        "{output}"
    );
}

fn find_clippy_command_key(answers: &toml::Table) -> String {
    answers["repository"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action_exposes_clippy(action))
        .unwrap()["runner"]["command"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn action_exposes_clippy(action: &TomlValue) -> bool {
    action["legacy_aliases"].as_array().is_some_and(|aliases| {
        aliases
            .iter()
            .any(|alias| alias.as_str() == Some(jig_contract::tool::CLIPPY))
    })
}

fn rename_repository_target_action(table: &mut toml::Table, component: &str, from: &str, to: &str) {
    for (_, value) in table.iter_mut() {
        rename_target_action_value(value, component, from, to);
    }
}

fn rename_target_action_value(value: &mut TomlValue, component: &str, from: &str, to: &str) {
    match value {
        TomlValue::Table(table) => {
            if table.get("component").and_then(TomlValue::as_str) == Some(component)
                && table.get("action").and_then(TomlValue::as_str) == Some(from)
            {
                table.insert("action".into(), to.into());
            }
            for (_, value) in table.iter_mut() {
                rename_target_action_value(value, component, from, to);
            }
        }
        TomlValue::Array(values) => {
            for value in values {
                rename_target_action_value(value, component, from, to);
            }
        }
        _ => {}
    }
}

fn run_update_with_recopy(repo: PathBuf, recopy: bool) -> serde_json::Value {
    run_update(UpdateOpts {
        path: repo,
        template: None,
        template_mode: None,
        recopy,
        launcher_only: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap()
}
