use super::*;
use crate::bootstrap::repository_model::rust_file_loc::RUST_FILE_LOC_COMMAND_KEY;

fn reload_authored_answers(model: &RepositoryRenderModel) -> RenderAnswers {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
        &path,
        format!(
            "repo_name = \"ExampleProject\"\nsqlx_enabled = false\nschema_dump_enabled = false\nrust_crate_roots = [\"crates\"]\n{}\n{}",
            model.authored_toml().unwrap(),
            model.commands_toml().unwrap()
        ),
    )
    .unwrap();
    RenderAnswers::from_answers_file(&path).unwrap()
}

fn reload_authored_model(model: &RepositoryRenderModel) -> RepositoryRenderModel {
    RepositoryRenderModel::from_answers(&reload_authored_answers(model)).unwrap()
}

#[test]
fn same_target_custom_loc_runner_returns_root_authority_to_authored_components() {
    let initial = answers("rust_crate_roots = [\"crates\"]\n");
    let mut authored = RepositoryRenderModel::from_answers(&initial).unwrap();
    let action = authored
        .actions
        .iter_mut()
        .find(|action| action.target.to_string() == "repo:rust-file-loc")
        .unwrap();
    action.runner = ActionRunner::command("custom_loc_command");
    authored
        .required_commands
        .retain(|command| command != "repo_rust_file_loc_command");
    authored.required_commands.push("custom_loc_command".into());
    authored.commands.remove("repo_rust_file_loc_command");
    authored.commands.insert(
        "custom_loc_command".into(),
        "scripts/check-authored-loc.sh".into(),
    );

    let temp = TempDir::new().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
        &path,
        format!(
            "repo_name = \"ExampleProject\"\nsqlx_enabled = false\nschema_dump_enabled = false\nrust_crate_roots = [\"crates\"]\n{}\n{}",
            authored.authored_toml().unwrap(),
            authored.commands_toml().unwrap()
        ),
    )
    .unwrap();

    let reloaded = RenderAnswers::from_answers_file(&path).unwrap();
    assert_eq!(
        serde_json::to_value(&reloaded).unwrap()["rust_crate_roots"],
        serde_json::json!(["."])
    );
    let rerendered = RepositoryRenderModel::from_answers(&reloaded).unwrap();
    let loc = rerendered
        .actions
        .iter()
        .find(|action| action.target.to_string() == "repo:rust-file-loc")
        .unwrap();
    assert_eq!(loc.runner, ActionRunner::command("custom_loc_command"));
    assert_eq!(
        rerendered.commands["custom_loc_command"],
        "scripts/check-authored-loc.sh"
    );
}

#[test]
fn same_key_authored_checker_mode_survives_model_round_trip() {
    let initial = answers("rust_crate_roots = [\"crates\"]\n");
    let mut authored = RepositoryRenderModel::from_answers(&initial).unwrap();
    authored.commands.insert(
        RUST_FILE_LOC_COMMAND_KEY.into(),
        "scripts/check-rust-file-loc.sh --all".into(),
    );

    let reloaded_answers = reload_authored_answers(&authored);
    assert_eq!(reloaded_answers.rust_crate_roots(), ["crates"]);
    let reloaded = RepositoryRenderModel::from_answers(&reloaded_answers).unwrap();

    assert_eq!(
        reloaded.commands[RUST_FILE_LOC_COMMAND_KEY],
        "scripts/check-rust-file-loc.sh --all"
    );
}

#[test]
fn authored_loc_action_alias_and_profile_choices_survive_model_round_trip() {
    let generated =
        RepositoryRenderModel::from_answers(&answers("rust_crate_roots = [\"crates\"]\n")).unwrap();
    let loc_target = target_id("repo", "rust-file-loc").unwrap();

    let mut action_removed = generated.clone();
    action_removed
        .actions
        .retain(|action| action.target != loc_target);
    for profile in &mut action_removed.profiles {
        profile.targets.retain(|target| target != &loc_target);
    }
    action_removed.commands.remove(RUST_FILE_LOC_COMMAND_KEY);
    let action_removed = reload_authored_model(&action_removed);
    assert!(
        action_removed
            .actions
            .iter()
            .all(|action| action.target != loc_target)
    );
    assert!(
        !action_removed
            .commands
            .contains_key(RUST_FILE_LOC_COMMAND_KEY)
    );

    let mut alias_removed = generated.clone();
    alias_removed
        .actions
        .iter_mut()
        .find(|action| action.target == loc_target)
        .unwrap()
        .legacy_aliases
        .clear();
    let alias_removed = reload_authored_model(&alias_removed);
    let loc = alias_removed
        .actions
        .iter()
        .find(|action| action.target == loc_target)
        .unwrap();
    assert!(loc.legacy_aliases.is_empty());
    assert!(
        alias_removed
            .tools
            .iter()
            .all(|tool| tool.name != "jig.rust_file_loc")
    );

    let mut profile_removed = generated;
    for profile in &mut profile_removed.profiles {
        profile.targets.retain(|target| target != &loc_target);
    }
    let profile_removed = reload_authored_model(&profile_removed);
    assert!(
        profile_removed
            .profiles
            .iter()
            .all(|profile| !profile.targets.contains(&loc_target))
    );
    assert!(
        profile_removed
            .actions
            .iter()
            .any(|action| action.target == loc_target)
    );
}

#[test]
fn default_branch_change_does_not_discard_managed_checker_roots() {
    let initial = answers("rust_crate_roots = [\"crates\"]\n");
    let authored = RepositoryRenderModel::from_answers(&initial).unwrap();
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
        &path,
        format!(
            "repo_name = \"ExampleProject\"\ndefault_branch = \"master\"\nsqlx_enabled = false\nschema_dump_enabled = false\nrust_crate_roots = [\"crates\"]\n{}\n{}",
            authored.authored_toml().unwrap(),
            authored.commands_toml().unwrap()
        ),
    )
    .unwrap();

    let reloaded = RenderAnswers::from_answers_file(&path).unwrap();
    assert_eq!(reloaded.default_branch(), "master");
    assert_eq!(reloaded.rust_crate_roots(), ["crates"]);
    let rerendered = RepositoryRenderModel::from_answers(&reloaded).unwrap();
    assert_eq!(
        rerendered.commands[RUST_FILE_LOC_COMMAND_KEY],
        "scripts/check-rust-file-loc.sh master"
    );
}
