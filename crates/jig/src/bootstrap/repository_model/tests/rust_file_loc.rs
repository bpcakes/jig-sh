use super::*;
use crate::bootstrap::AnswerOpts;
use crate::bootstrap::answers::{AnswerInput, AnswerResolution};
use crate::bootstrap::repository_model::rust_file_loc::{
    RUST_FILE_LOC_COMMAND_KEY, generated_legacy_rust_file_loc_action,
};

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

fn reload_managed_model(model: &RepositoryRenderModel) -> RepositoryRenderModel {
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
    let input = AnswerInput::from_file(&path).unwrap();
    let resolution =
        AnswerResolution::from_input(input, &AnswerOpts::default(), temp.path(), false).unwrap();
    let (answers, _) = resolution.into_parts();
    RepositoryRenderModel::from_answers(&answers).unwrap()
}

#[test]
fn generated_freshness_defaults_remain_managed_but_declared_policies_are_preserved() {
    use jig_contract::ActionInputsPolicy;

    let generated =
        RepositoryRenderModel::from_answers(&answers("rust_crate_roots = [\"crates\"]\n")).unwrap();
    let mut rendered = generated.clone();
    rendered.prepare_runner_epoch(9).unwrap();
    rendered.prepare_freshness_epoch(9).unwrap();
    assert_eq!(reload_managed_model(&rendered).actions, generated.actions);

    for policy in [
        ActionInputsPolicy::WholeRepository,
        ActionInputsPolicy::Exhaustive,
    ] {
        let mut authored = rendered.clone();
        let action = authored
            .actions
            .iter_mut()
            .find(|action| action.target.to_string() == "api:clippy")
            .unwrap();
        action.inputs_policy = Some(policy);
        action
            .provenance
            .insert("inputs_policy".into(), FieldProvenance::Declared);
        assert_eq!(reload_managed_model(&authored).actions, authored.actions);
    }
}

#[test]
fn superseded_formatter_inference_stays_managed_and_is_retracted_on_render() {
    use jig_contract::ActionSourceState;

    let generated =
        RepositoryRenderModel::from_answers(&answers("rust_crate_roots = [\"crates\"]\n")).unwrap();
    let mut previous = generated.clone();
    previous.prepare_runner_epoch(8).unwrap();
    previous.prepare_freshness_epoch(8).unwrap();
    let formatter = previous
        .actions
        .iter_mut()
        .find(|action| action.target.to_string() == "api:fmt")
        .unwrap();
    formatter.source_state = Some(ActionSourceState::Worktree);
    assert_eq!(
        formatter.provenance.get("source_state"),
        Some(&FieldProvenance::Inferred)
    );
    // The obsolete inference must not turn an otherwise generated model custom.
    assert_eq!(reload_managed_model(&previous).actions, generated.actions);

    // Also retract when the caller deliberately keeps the authored projection.
    let mut rendered = reload_authored_model(&previous);
    rendered.prepare_freshness_epoch(8).unwrap();
    let formatter = rendered
        .actions
        .iter()
        .find(|action| action.target.to_string() == "api:fmt")
        .unwrap();
    assert_eq!(formatter.source_state, Some(ActionSourceState::Git));
    assert_eq!(
        formatter.provenance.get("source_state"),
        Some(&FieldProvenance::Inferred)
    );
    let once = rendered.actions.clone();
    rendered.prepare_freshness_epoch(8).unwrap();
    assert_eq!(rendered.actions, once);
}

#[test]
fn formatter_inference_migration_preserves_owner_policies_and_changed_runners() {
    use jig_contract::ActionSourceState;

    for (provenance, custom_command) in [
        (Some(FieldProvenance::Declared), false),
        (Some(FieldProvenance::Overridden), false),
        (Some(FieldProvenance::Inherited), false),
        (None, false),
        (Some(FieldProvenance::Inferred), true),
    ] {
        let mut model =
            RepositoryRenderModel::from_answers(&answers("rust_crate_roots = [\"crates\"]\n"))
                .unwrap();
        model.prepare_runner_epoch(8).unwrap();
        model.prepare_freshness_epoch(8).unwrap();
        let action = model
            .actions
            .iter_mut()
            .find(|action| action.target.to_string() == "api:fmt")
            .unwrap();
        action.source_state = Some(ActionSourceState::Worktree);
        if let Some(provenance) = provenance {
            action.provenance.insert("source_state".into(), provenance);
        } else {
            action.provenance.remove("source_state");
        }
        if custom_command {
            let ActionRunner::Shell { command, .. } = &action.runner else {
                panic!("expected shell formatter")
            };
            model
                .commands
                .insert(command.clone(), "scripts/owned-format-check.sh".into());
        }
        let mut rendered = reload_managed_model(&model);
        rendered.prepare_freshness_epoch(8).unwrap();
        let action = rendered
            .actions
            .iter()
            .find(|action| action.target.to_string() == "api:fmt")
            .unwrap();
        assert_eq!(action.source_state, Some(ActionSourceState::Worktree));
        if let Some(provenance) = provenance {
            assert_eq!(action.provenance.get("source_state"), Some(&provenance));
        }
    }
}

#[test]
fn same_target_authored_file_budget_runner_survives_model_round_trip() {
    let initial = answers("rust_crate_roots = [\"crates\"]\n");
    let mut authored = RepositoryRenderModel::from_answers(&initial).unwrap();
    let action = authored
        .actions
        .iter_mut()
        .find(|action| action.target.to_string() == "repo:file-budget")
        .unwrap();
    action.runner = ActionRunner::command("custom_file_budget_command");
    authored
        .required_commands
        .push("custom_file_budget_command".into());
    authored.commands.insert(
        "custom_file_budget_command".into(),
        "scripts/check-authored-budget.sh".into(),
    );

    let rerendered = reload_authored_model(&authored);
    let budget = rerendered
        .actions
        .iter()
        .find(|action| action.target.to_string() == "repo:file-budget")
        .unwrap();
    assert_eq!(
        budget.runner,
        ActionRunner::command("custom_file_budget_command")
    );
    assert_eq!(
        rerendered.commands["custom_file_budget_command"],
        "scripts/check-authored-budget.sh"
    );
}

#[test]
fn authored_file_budget_action_alias_and_profile_choices_survive_model_round_trip() {
    let generated =
        RepositoryRenderModel::from_answers(&answers("rust_crate_roots = [\"crates\"]\n")).unwrap();
    let budget_target = target_id("repo", "file-budget").unwrap();

    let mut action_removed = generated.clone();
    action_removed
        .actions
        .retain(|action| action.target != budget_target);
    for profile in &mut action_removed.profiles {
        profile.targets.retain(|target| target != &budget_target);
    }
    let action_removed = reload_authored_model(&action_removed);
    assert!(
        action_removed
            .actions
            .iter()
            .all(|action| action.target != budget_target)
    );
    assert!(
        action_removed
            .tools
            .iter()
            .all(|tool| tool.name != "jig.file_budget")
    );

    let mut alias_removed = generated.clone();
    alias_removed
        .actions
        .iter_mut()
        .find(|action| action.target == budget_target)
        .unwrap()
        .legacy_aliases
        .clear();
    let alias_removed = reload_authored_model(&alias_removed);
    let budget = alias_removed
        .actions
        .iter()
        .find(|action| action.target == budget_target)
        .unwrap();
    assert!(budget.legacy_aliases.is_empty());
    assert!(
        alias_removed
            .tools
            .iter()
            .all(|tool| tool.name != "jig.file_budget")
    );

    let mut profile_removed = generated;
    for profile in &mut profile_removed.profiles {
        profile.targets.retain(|target| target != &budget_target);
    }
    let profile_removed = reload_authored_model(&profile_removed);
    assert!(
        profile_removed
            .profiles
            .iter()
            .all(|profile| !profile.targets.contains(&budget_target))
    );
    assert!(
        profile_removed
            .actions
            .iter()
            .any(|action| action.target == budget_target)
    );
}

#[test]
fn exact_generated_legacy_action_upgrades_to_native_file_budget() {
    for persisted in [false, true] {
        let initial = answers("rust_crate_roots = [\"crates\"]\n");
        let mut legacy = RepositoryRenderModel::from_answers(&initial).unwrap();
        // Historical generated repositories had only the final verify profile.
        legacy
            .profiles
            .retain(|profile| profile.id.as_str() == "verify");
        let budget_target = target_id("repo", "file-budget").unwrap();
        let legacy_target = target_id("repo", "rust-file-loc").unwrap();
        legacy
            .actions
            .retain(|action| action.target != budget_target);
        legacy
            .actions
            .push(generated_legacy_rust_file_loc_action().unwrap());
        legacy
            .actions
            .sort_by(|left, right| left.target.cmp(&right.target));
        for profile in &mut legacy.profiles {
            profile.targets.retain(|target| target != &budget_target);
            profile.targets.push(legacy_target.clone());
            profile.targets.sort();
            profile.targets.dedup();
        }
        legacy.commands.insert(
            RUST_FILE_LOC_COMMAND_KEY.into(),
            "scripts/check-rust-file-loc.sh main".into(),
        );

        if persisted {
            for action in &mut legacy.actions {
                action.source_state = Some(jig_contract::ActionSourceState::Git);
                action
                    .provenance
                    .insert("source_state".into(), FieldProvenance::Inferred);
            }
            legacy.prepare_runner_epoch(8).unwrap();
            legacy.prepare_freshness_epoch(8).unwrap();
        }
        let upgraded = reload_managed_model(&legacy);
        if persisted {
            // Conservative saved defaults remain managed. Compare their final
            // rendered policies, not the pre-epoch generated model.
            let mut rendered = upgraded.clone();
            rendered.prepare_runner_epoch(8).unwrap();
            rendered.prepare_freshness_epoch(8).unwrap();
            for saved in legacy
                .actions
                .iter()
                .filter(|action| action.target != legacy_target)
            {
                assert_eq!(
                    rendered
                        .actions
                        .iter()
                        .find(|action| action.target == saved.target),
                    Some(saved)
                );
            }
        }
        assert!(
            upgraded
                .actions
                .iter()
                .any(|action| action == &generated_file_budget_action().unwrap())
        );
        assert!(
            upgraded
                .actions
                .iter()
                .all(|action| action.target != legacy_target)
        );
        assert!(!upgraded.commands.contains_key(RUST_FILE_LOC_COMMAND_KEY));
    }
}

#[test]
fn customized_legacy_checker_is_preserved_as_authored_authority() {
    let initial = answers("rust_crate_roots = [\"crates\"]\n");
    let mut authored = RepositoryRenderModel::from_answers(&initial).unwrap();
    let budget_target = target_id("repo", "file-budget").unwrap();
    let mut legacy = generated_legacy_rust_file_loc_action().unwrap();
    legacy.runner = ActionRunner::command(RUST_FILE_LOC_COMMAND_KEY);
    authored
        .actions
        .retain(|action| action.target != budget_target);
    authored.actions.push(legacy.clone());
    authored
        .actions
        .sort_by(|left, right| left.target.cmp(&right.target));
    for profile in &mut authored.profiles {
        profile.targets.retain(|target| target != &budget_target);
        profile.targets.push(legacy.target.clone());
        profile.targets.sort();
        profile.targets.dedup();
    }
    authored.commands.insert(
        RUST_FILE_LOC_COMMAND_KEY.into(),
        "scripts/check-rust-file-loc.sh --all".into(),
    );

    let rerendered = reload_authored_model(&authored);
    assert!(rerendered.actions.iter().any(|action| action == &legacy));
    assert_eq!(
        rerendered.commands[RUST_FILE_LOC_COMMAND_KEY],
        "scripts/check-rust-file-loc.sh --all"
    );
}
