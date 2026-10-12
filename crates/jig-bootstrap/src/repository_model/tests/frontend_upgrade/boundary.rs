//! The public boundary check guards a Rust admin API. Updates narrow its
//! inputs, retire an unmodified Go one with its command, and otherwise keep
//! what its owner did with it.

use super::*;

#[test]
fn managed_update_retires_the_go_boundary_check_inside_a_custom_model() {
    let mut saved = legacy_model("go");
    saved
        .components
        .iter_mut()
        .find(|item| item.id.as_str() == "web")
        .unwrap()
        .description = Some("Custom frontend description".into());

    let answers = load_saved(&saved, "go");
    let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

    assert!(answers.authored_repository().is_some());
    let boundary = target_id("repo", "frontend-public-boundary").unwrap();
    assert!(!upgraded.actions.iter().any(|item| item.target == boundary));
    assert!(!upgraded.profiles[0].targets.contains(&boundary));
    assert!(
        !upgraded
            .commands
            .contains_key("repo_frontend_public_boundary_command")
    );
    assert!(upgraded.frontend_contracts_enabled());
}

#[test]
fn managed_update_keeps_a_customized_boundary_check() {
    let mut saved = legacy_model("rust");
    let boundary = saved
        .actions
        .iter_mut()
        .find(|item| item.target.to_string() == "repo:frontend-public-boundary")
        .unwrap();
    boundary.inputs.push("docs/privileged/**".into());
    let expected = boundary.clone();

    let answers = load_saved(&saved, "rust");
    let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

    assert!(answers.authored_repository().is_some());
    assert_eq!(
        upgraded
            .actions
            .iter()
            .find(|item| item.target == expected.target)
            .unwrap()
            .inputs,
        expected.inputs
    );
    assert!(upgraded.profiles[0].targets.contains(&expected.target));
    assert!(
        upgraded
            .commands
            .contains_key("repo_frontend_public_boundary_command")
    );
}

#[test]
fn managed_update_narrows_admin_boundary_inputs_and_keeps_models_generated() {
    let apps = format!("{APP}{ADMIN_APP}");
    let current = RepositoryRenderModel::from_answers(&scaffold_answers(&format!(
        "backend_language = \"rust\"\n{apps}"
    )))
    .unwrap();
    for backend_inputs in [LEGACY_BACKEND_INPUTS, LOCKFILE_BACKEND_INPUTS] {
        let saved = saved_model_with("rust", &apps, backend_inputs);

        let answers = load_saved_with(&saved, "rust", &apps);

        assert!(answers.authored_repository().is_none());
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert_eq!(
            upgraded.authored_toml().unwrap(),
            current.authored_toml().unwrap()
        );
    }
}

/// An update drops the command of a boundary check that it retires, but
/// keeps the command of one that its owner removed.
#[test]
fn managed_update_keeps_the_command_of_a_boundary_check_its_owner_removed() {
    let apps = format!("{APP}{ADMIN_APP}");
    let mut saved = saved_model_with("rust", &apps, LOCKFILE_BACKEND_INPUTS);
    let boundary = target_id("repo", "frontend-public-boundary").unwrap();
    saved.actions.retain(|item| item.target != boundary);
    for profile in &mut saved.profiles {
        profile.targets.retain(|target| *target != boundary);
    }
    let command = saved.commands["repo_frontend_public_boundary_command"].clone();

    let answers = load_saved_with(&saved, "rust", &apps);
    let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

    assert!(answers.authored_repository().is_some());
    assert!(!upgraded.actions.iter().any(|item| item.target == boundary));
    assert_eq!(
        upgraded.commands["repo_frontend_public_boundary_command"],
        command
    );
}
