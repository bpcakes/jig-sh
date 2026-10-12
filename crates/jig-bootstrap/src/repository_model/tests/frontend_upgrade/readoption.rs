//! Re-adoption with a capability change regenerates the harness actions
//! around the saved components, whatever their names and roots.

use super::*;
use crate::adopt_infer::{ComponentSelectionOpts, infer_adopt_answers};
use crate::answers::{AnswerInput, HarnessFootprint};

/// Re-adopts a Rust scaffold saved as `model` and enables SQLx, resolving
/// the answers as `jig adopt . --write --sqlx-enabled true` does.
fn readopt_with_sqlx(model: &RepositoryRenderModel) -> Result<RenderAnswers> {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let path = root.join(".jig.toml");
    fs::write(
        &path,
        format!(
            "repo_name = \"ExampleProject\"\nbackend_language = \"rust\"\nharness_footprint = \"full\"\nsqlx_enabled = false\nschema_dump_enabled = false\n{APP}\n{}\n{}",
            model.authored_toml().unwrap(),
            model.commands_toml().unwrap()
        ),
    )
    .unwrap();
    let mut inference = infer_adopt_answers(root);
    let mut opts = AnswerOpts {
        harness_footprint: Some(HarnessFootprint::Full),
        sqlx_enabled: Some(true),
        rust_migration_dir: Some("migrations".into()),
        ..AnswerOpts::default()
    };
    let mut input = AnswerInput::from_file(&path)?;
    input.prepare_adoption(
        &mut inference,
        root,
        &opts,
        &ComponentSelectionOpts::default(),
    )?;
    let shape = input.shape().clone();
    inference.apply_to_answers(&mut opts, &shape);
    inference.apply_component_decisions(&mut opts);
    Ok(AnswerResolution::from_input(input, &opts, root, true)?
        .into_parts()
        .0)
}

fn has_target(model: &RepositoryRenderModel, target: &str) -> bool {
    model
        .actions
        .iter()
        .any(|action| action.target.to_string() == target)
}

#[test]
fn capability_refresh_keeps_the_contract_checks_beside_a_nested_backend() {
    let mut saved = saved_current_model("rust");
    saved
        .components
        .iter_mut()
        .find(|component| component.id.as_str() == "api")
        .unwrap()
        .root = "backend".into();

    let contract_checks = saved
        .actions
        .iter()
        .map(|action| action.target.to_string())
        .filter(|target| {
            matches!(
                target.as_str(),
                "repo:frontend-contract-drift" | "repo:frontend-public-boundary"
            )
        })
        .collect::<Vec<_>>();
    assert!(contract_checks.contains(&"repo:frontend-contract-drift".into()));

    // Adoption generates backend actions only for a backend at the root, so
    // none can own a regenerated document check, but the saved contract
    // checks are regenerated.
    let answers = readopt_with_sqlx(&saved).unwrap();
    let model = RepositoryRenderModel::from_answers(&answers).unwrap();
    for target in &contract_checks {
        assert!(has_target(&model, target), "{target}");
    }
}

#[test]
fn capability_refresh_keeps_a_renamed_backend_document_check_in_ci() {
    let mut saved = saved_current_model("rust");
    rename_backend(&mut saved, "server");

    let answers = readopt_with_sqlx(&saved).unwrap();
    let model = RepositoryRenderModel::from_answers(&answers).unwrap();
    assert!(has_target(&model, "server:openapi"));
    assert_eq!(
        model.openapi_document_check_targets("rust"),
        ["server:openapi"]
    );
    assert!(model.rust_ci_input_paths().contains(&"openapi/**".into()));
}

/// Only `jig update --add-openapi-check` gives a model from before the
/// document check the check.
#[test]
fn capability_refresh_leaves_an_older_model_without_the_document_check() {
    let answers = readopt_with_sqlx(&legacy_model("rust")).unwrap();
    let model = RepositoryRenderModel::from_answers(&answers).unwrap();

    assert!(
        model
            .components
            .iter()
            .any(|component| component.adapters.iter().any(|adapter| adapter == "sqlx"))
    );
    assert!(!has_target(&model, "api:openapi"));
    assert!(!model.commands.contains_key("api_openapi_command"));
}

#[test]
fn capability_refresh_adds_no_document_check_under_a_taken_command_key() {
    let mut saved = legacy_model("rust");
    saved.commands.insert(
        "api_openapi_command".into(),
        "printf skip-openapi-check".into(),
    );

    let answers = readopt_with_sqlx(&saved).unwrap();
    let model = RepositoryRenderModel::from_answers(&answers).unwrap();

    assert!(!has_target(&model, "api:openapi"));
    assert_eq!(
        model.commands["api_openapi_command"],
        "printf skip-openapi-check"
    );
}

#[test]
fn capability_refresh_keeps_a_removed_document_check_removed() {
    // A scaffold's own check, and one that an update gave an older model.
    let mut upgraded = RepositoryRenderModel::from_answers(&load_saved_adding_check(
        &legacy_model("rust"),
        "rust",
    ))
    .unwrap();
    upgraded
        .prepare_runner_epoch(jig_context::CURRENT_CONTRACT_VERSION)
        .unwrap();
    upgraded
        .prepare_freshness_epoch(jig_context::CURRENT_CONTRACT_VERSION)
        .unwrap();
    assert!(has_target(&upgraded, "api:openapi"));
    for (label, mut saved) in [
        ("scaffold", saved_current_model("rust")),
        ("upgraded", upgraded),
    ] {
        let check = target_id("api", "openapi").unwrap();
        saved.actions.retain(|action| action.target != check);
        for profile in &mut saved.profiles {
            profile.targets.retain(|target| *target != check);
        }
        saved.commands.remove("api_openapi_command");

        let answers = readopt_with_sqlx(&saved).unwrap();
        let model = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(
            model
                .components
                .iter()
                .any(|component| component.adapters.iter().any(|adapter| adapter == "sqlx")),
            "{label}"
        );
        assert!(!has_target(&model, "api:openapi"), "{label}");
        assert!(
            !model.commands.contains_key("api_openapi_command"),
            "{label}"
        );
        assert!(
            model
                .profiles
                .iter()
                .all(|profile| !profile.targets.contains(&check)),
            "{label}"
        );
    }
}

#[test]
fn capability_refresh_keeps_a_customized_document_check_command() {
    let custom = "cargo test --workspace --locked committed_ -- --format pretty";
    let mut saved = saved_current_model("rust");
    saved
        .commands
        .insert("api_openapi_command".into(), custom.into());

    let answers = readopt_with_sqlx(&saved).unwrap();
    let model = RepositoryRenderModel::from_answers(&answers).unwrap();

    assert!(has_target(&model, "api:openapi"));
    assert_eq!(model.commands["api_openapi_command"], custom);
}

/// A dependency that an owner declared on a generated check survives the
/// refresh that regenerates the check.
#[test]
fn capability_refresh_keeps_declared_dependencies_of_a_generated_check() {
    let mut saved = saved_current_model("rust");
    let guard_target = target_id("repo", "release-guard").unwrap();
    let mut guard = ActionSpec::new(
        guard_target.clone(),
        ActionIntent::Check,
        ActionRunner::command("repo_release_guard_command"),
    );
    guard.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
    guard.provenance = provenance(&[
        ("target", FieldProvenance::Declared),
        ("runner", FieldProvenance::Declared),
    ]);
    saved.actions.push(guard);
    saved.actions.sort_by(|a, b| a.target.cmp(&b.target));
    saved
        .commands
        .insert("repo_release_guard_command".into(), "true".into());
    let check = saved
        .actions
        .iter_mut()
        .find(|action| action.target.to_string() == "api:openapi")
        .unwrap();
    check.depends_on = vec![guard_target.clone()];
    check
        .provenance
        .insert("depends_on".into(), FieldProvenance::Declared);

    let answers = readopt_with_sqlx(&saved).unwrap();
    let model = RepositoryRenderModel::from_answers(&answers).unwrap();

    assert!(
        model
            .components
            .iter()
            .any(|component| component.adapters.iter().any(|adapter| adapter == "sqlx"))
    );
    let check = model
        .actions
        .iter()
        .find(|action| action.target.to_string() == "api:openapi")
        .unwrap();
    assert_eq!(check.depends_on, [guard_target]);
    assert_eq!(
        check.provenance.get("depends_on"),
        Some(&FieldProvenance::Declared)
    );
}

/// Inputs that an owner declared on a generated check survive the refresh,
/// so a change to them still selects the check and runs its workflow.
#[test]
fn capability_refresh_keeps_inputs_an_owner_declared_on_a_generated_check() {
    let mut saved = saved_current_model("rust");
    let check = saved
        .actions
        .iter_mut()
        .find(|action| action.target.to_string() == "api:openapi")
        .unwrap();
    check.inputs.push("schemas/**".into());
    check.inputs.sort();
    check
        .provenance
        .insert("inputs".into(), FieldProvenance::Declared);

    let answers = readopt_with_sqlx(&saved).unwrap();
    let model = RepositoryRenderModel::from_answers(&answers).unwrap();

    let check = model
        .actions
        .iter()
        .find(|action| action.target.to_string() == "api:openapi")
        .unwrap();
    assert!(check.inputs.contains(&"schemas/**".into()));
    assert_eq!(
        check.provenance.get("inputs"),
        Some(&FieldProvenance::Declared)
    );
    assert!(model.rust_ci_input_paths().contains(&"schemas/**".into()));
}

/// A contract check whose inputs its owner changed stays as its owner left
/// it, as on update; an unmodified one is regenerated.
#[test]
fn capability_refresh_keeps_contract_checks_whose_inputs_their_owner_changed() {
    for (target, input) in [
        ("repo:frontend-contract-drift", "schemas/**"),
        ("repo:frontend-public-boundary", "docs/privileged/**"),
    ] {
        let mut saved = saved_current_model("rust");
        let check = saved
            .actions
            .iter_mut()
            .find(|action| action.target.to_string() == target)
            .unwrap();
        check.inputs.push(input.into());
        check.inputs.sort();

        let answers = readopt_with_sqlx(&saved).unwrap();
        let model = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(
            model
                .components
                .iter()
                .any(|component| component.adapters.iter().any(|adapter| adapter == "sqlx")),
            "{target}"
        );
        let refreshed = model
            .actions
            .iter()
            .find(|action| action.target.to_string() == target)
            .unwrap();
        assert!(refreshed.inputs.contains(&input.into()), "{target}");
    }
}

/// Contract checks with the inputs that earlier generators wrote end up with
/// those that generation writes now.
#[test]
fn capability_refresh_regenerates_the_contract_checks_of_an_older_model() {
    let answers = readopt_with_sqlx(&legacy_model("rust")).unwrap();
    let model = RepositoryRenderModel::from_answers(&answers).unwrap();

    for target in [
        "repo:frontend-contract-drift",
        "repo:frontend-public-boundary",
    ] {
        let check = model
            .actions
            .iter()
            .find(|action| action.target.to_string() == target)
            .unwrap();
        assert!(check.inputs.contains(&"Cargo.lock".into()), "{target}");
    }
}

/// A document check that its owner removed stays removed, even while
/// another component keeps a document check of its own.
#[test]
fn capability_refresh_keeps_a_removed_document_check_removed_beside_another_one() {
    let mut saved = saved_current_model("rust");
    let mut worker = saved
        .components
        .iter()
        .find(|component| component.id.as_str() == "api")
        .unwrap()
        .clone();
    worker.id = component_id("worker").unwrap();
    worker.root = "worker".into();
    saved.components.push(worker);
    saved.components.sort_by(|a, b| a.id.cmp(&b.id));
    let check = target_id("api", "openapi").unwrap();
    let mut worker_check = saved
        .actions
        .iter()
        .find(|action| action.target == check)
        .unwrap()
        .clone();
    worker_check.target = target_id("worker", "openapi").unwrap();
    worker_check.runner = ActionRunner::command("worker_openapi_command");
    worker_check.inputs = vec!["worker/**".into()];
    worker_check.provenance = provenance(&[
        ("target", FieldProvenance::Declared),
        ("runner", FieldProvenance::Declared),
        ("inputs", FieldProvenance::Declared),
    ]);
    saved.actions.push(worker_check);
    saved.commands.insert(
        "worker_openapi_command".into(),
        "cd worker && cargo test --locked openapi_is_current".into(),
    );
    // The owner removes the backend's check.
    saved.actions.retain(|action| action.target != check);
    saved.actions.sort_by(|a, b| a.target.cmp(&b.target));
    for profile in &mut saved.profiles {
        profile.targets.retain(|target| *target != check);
    }
    saved.commands.remove("api_openapi_command");

    let answers = readopt_with_sqlx(&saved).unwrap();
    let model = RepositoryRenderModel::from_answers(&answers).unwrap();

    assert!(
        model
            .components
            .iter()
            .any(|component| component.adapters.iter().any(|adapter| adapter == "sqlx"))
    );
    assert!(!has_target(&model, "api:openapi"));
    assert!(!model.commands.contains_key("api_openapi_command"));
    assert!(
        model
            .profiles
            .iter()
            .all(|profile| !profile.targets.contains(&check))
    );
    assert!(has_target(&model, "worker:openapi"));
}

/// A command that the owner set for the check before asking for it keeps the
/// model one from before the check through an update and then a refresh,
/// which adds no check.
#[test]
fn capability_refresh_keeps_an_older_model_with_an_owners_command_eligible() {
    let custom = "cargo test --workspace --locked public_document -- --format pretty";
    let mut saved = legacy_model("rust");
    saved
        .commands
        .insert("api_openapi_command".into(), custom.into());
    // An update first brings the contract checks' inputs up to date.
    let mut updated = RepositoryRenderModel::from_answers(&load_saved(&saved, "rust")).unwrap();
    updated
        .prepare_runner_epoch(jig_context::CURRENT_CONTRACT_VERSION)
        .unwrap();
    updated
        .prepare_freshness_epoch(jig_context::CURRENT_CONTRACT_VERSION)
        .unwrap();

    let answers = readopt_with_sqlx(&updated).unwrap();
    let model = RepositoryRenderModel::from_answers(&answers).unwrap();

    assert!(
        model
            .components
            .iter()
            .any(|component| component.adapters.iter().any(|adapter| adapter == "sqlx"))
    );
    assert!(!has_target(&model, "api:openapi"));
    assert_eq!(model.commands["api_openapi_command"], custom);
    let drift = model
        .actions
        .iter()
        .find(|action| action.target.to_string() == "repo:frontend-contract-drift")
        .unwrap();
    assert_eq!(
        drift.description.as_deref(),
        Some("Check repository-wide OpenAPI and generated-client drift.")
    );
}
