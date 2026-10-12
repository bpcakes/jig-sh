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
