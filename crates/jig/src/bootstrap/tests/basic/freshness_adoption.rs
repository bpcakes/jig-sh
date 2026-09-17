use super::*;
use jig_contract::{
    ActionInputsPolicy, ActionRunner, ActionSourceState, ActionSpec, FieldProvenance,
};

fn options(repo: &Path, template: &Path, minimal: bool, force: bool) -> AdoptOpts {
    let mut options = footprint_adopt_opts(repo, template, minimal, force);
    options.answers.repo_name = Some("ExampleProject".into());
    options
}

fn saved_formatter(repo: &Path) -> ActionSpec {
    let ctx = crate::context::RepoContext::load_from(repo).unwrap();
    ctx.authored_action_specs()
        .unwrap()
        .iter()
        .find(|action| {
            action
                .legacy_aliases
                .iter()
                .any(|alias| alias == "jig.fmt_check")
        })
        .unwrap()
        .clone()
}

fn save_formatter(repo: &Path, action: &ActionSpec, command: Option<&str>) {
    let config_path = repo.join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let actions = config["repository"]["actions"].as_array_mut().unwrap();
    let index = actions
        .iter()
        .position(|value| {
            value["target"]["component"].as_str() == Some(action.target.component.as_str())
                && value["target"]["action"].as_str() == Some(action.target.action.as_str())
        })
        .unwrap();
    actions[index] = toml::Value::try_from(action).unwrap();
    if let Some(value) = command {
        let key = match &action.runner {
            ActionRunner::Shell { command, .. } | ActionRunner::Command { command, .. } => command,
            _ => panic!("fixture formatter must retain a command key"),
        };
        config["commands"]
            .as_table_mut()
            .unwrap()
            .insert(key.clone(), toml::Value::String(value.into()));
    }
    fs::write(config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
    let manifest_path = repo.join(".agent/jig-contract.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    let target = serde_json::to_value(&action.target).unwrap();
    let manifest_action = manifest["actions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|value| value["target"] == target)
        .unwrap();
    *manifest_action = serde_json::to_value(action).unwrap();
    fs::write(
        manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    crate::context::RepoContext::load_from(repo).unwrap();
}

#[test]
fn update_and_recopy_preserve_saved_git_policy_for_every_provenance() {
    let _guard = lock_env();
    let template = materialize_template_worktree();
    for provenance in [
        Some(FieldProvenance::Inferred),
        Some(FieldProvenance::Declared),
        Some(FieldProvenance::Overridden),
        None,
    ] {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("ExampleProject");
        fs::create_dir_all(&repo).unwrap();
        run_adopt(options(&repo, template.path(), false, false)).unwrap();
        let mut action = saved_formatter(&repo);
        action.source_state = Some(ActionSourceState::Git);
        if let Some(provenance) = provenance {
            action.provenance.insert("source_state".into(), provenance);
        } else {
            action.provenance.remove("source_state");
        }
        for recopy in [false, true] {
            save_formatter(&repo, &action, None);
            run_update(UpdateOpts {
                path: repo.clone(),
                template: Some(template.path().display().to_string()),
                template_mode: None,
                recopy,
                launcher_only: false,
                // Accept re-rendering the fixture-edited manifest; its authored
                // policies must still survive the requested update.
                force: true,
                vcs_ref: None,
                defaults: true,
                no_input: true,
            })
            .unwrap();
            let refreshed = saved_formatter(&repo);
            assert_eq!(
                refreshed.source_state,
                Some(ActionSourceState::Git),
                "saved source policy was changed during recopy={recopy}"
            );
            assert_eq!(refreshed.inputs_policy, action.inputs_policy);
            assert_eq!(refreshed.inputs, action.inputs);
            assert_eq!(refreshed.runner, action.runner);
        }
    }
}

#[test]
fn footprint_and_capability_refresh_preserve_owned_freshness_and_its_command() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("ExampleProject");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(options(&repo, template.path(), false, false)).unwrap();
    fs::write(
        repo.join("scripts/check-format.sh"),
        "#!/bin/sh\ncargo fmt --all -- --check\n",
    )
    .unwrap();
    let mut action = saved_formatter(&repo);
    action.source_state = Some(ActionSourceState::Worktree);
    action.inputs_policy = Some(ActionInputsPolicy::Exhaustive);
    action.inputs = [
        "**/*.rs",
        "Cargo.toml",
        "rustfmt.toml",
        "scripts/check-format.sh",
    ]
    .map(str::to_owned)
    .into();
    for field in ["source_state", "inputs_policy", "inputs"] {
        action
            .provenance
            .insert(field.into(), FieldProvenance::Declared);
    }
    match &mut action.runner {
        ActionRunner::Shell { environment, .. } | ActionRunner::Command { environment, .. } => {
            environment.insert("FORMAT_PROFILE".into(), "owned".into());
        }
        _ => panic!("generated formatter must use a command key"),
    }
    save_formatter(&repo, &action, Some("scripts/check-format.sh"));
    run_adopt(options(&repo, template.path(), true, true)).unwrap();
    assert_eq!(saved_formatter(&repo), action);

    let mut capability_refresh = options(&repo, template.path(), true, true);
    capability_refresh.answers.sqlx_enabled = Some(true);
    capability_refresh.answers.rust_migration_dir = Some("migrations".into());
    run_adopt(capability_refresh).unwrap();
    assert_eq!(saved_formatter(&repo), action);
    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    let key = match &action.runner {
        ActionRunner::Shell { command, .. } | ActionRunner::Command { command, .. } => command,
        _ => unreachable!(),
    };
    assert_eq!(ctx.command_for_key(key).unwrap(), "scripts/check-format.sh");
}

#[test]
fn inferred_formatter_wrapper_keeps_conservative_freshness_defaults() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("ExampleProject");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"example-project\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(repo.join("src/lib.rs"), "").unwrap();
    fs::write(
        repo.join("Justfile"),
        "fmt-check:\n    cargo fmt --all -- --check\n",
    )
    .unwrap();
    run_adopt(options(&repo, template.path(), false, false)).unwrap();
    let action = saved_formatter(&repo);
    assert_eq!(action.source_state, Some(ActionSourceState::Git));
    assert_eq!(
        action.inputs_policy,
        Some(ActionInputsPolicy::WholeRepository)
    );
    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    let key = match &action.runner {
        ActionRunner::Shell { command, .. } | ActionRunner::Command { command, .. } => command,
        _ => panic!("inferred wrapper must use a command key"),
    };
    assert_eq!(ctx.command_for_key(key).unwrap(), "just fmt-check");
}
