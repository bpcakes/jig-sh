//! Re-adoption with a capability change regenerates the harness actions
//! around the saved components, whatever their names and roots.

use super::*;
use crate::adopt_infer::{ComponentSelectionOpts, infer_adopt_answers};
use crate::answers::{AnswerInput, HarnessFootprint};

/// Re-adopts a Rust scaffold saved as `model` and enables SQLx, resolving
/// the answers as `jig adopt . --write --sqlx-enabled true` does.
fn readopt_with_sqlx(model: &RepositoryRenderModel) -> Result<RenderAnswers> {
    let temp = TempDir::new().unwrap();
    write_scaffold_document_tests(temp.path(), "rust");
    readopt_with_sqlx_at(temp.path(), model)
}

/// Re-adopts the repository at `root`, saved as `model`, enabling SQLx.
fn readopt_with_sqlx_at(root: &Path, model: &RepositoryRenderModel) -> Result<RenderAnswers> {
    readopt_apps_with_sqlx_at(root, model, APP)
}

/// Re-adopts the repository at `root`, saved as `model` with the frontend
/// `apps`, enabling SQLx.
fn readopt_apps_with_sqlx_at(
    root: &Path,
    model: &RepositoryRenderModel,
    apps: &str,
) -> Result<RenderAnswers> {
    let path = root.join(".jig.toml");
    fs::write(
        &path,
        format!(
            "repo_name = \"ExampleProject\"\nbackend_language = \"rust\"\nharness_footprint = \"full\"\nsqlx_enabled = false\nschema_dump_enabled = false\n{apps}\n{}\n{}",
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

#[test]
fn capability_refresh_adds_the_document_check_only_beside_its_tests() {
    for tests in [true, false] {
        let temp = TempDir::new().unwrap();
        if tests {
            write_scaffold_document_tests(temp.path(), "rust");
        }

        let answers = readopt_with_sqlx_at(temp.path(), &legacy_model("rust")).unwrap();
        let model = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert_eq!(has_target(&model, "api:openapi"), tests, "{tests}");
        assert_eq!(
            model.commands.contains_key("api_openapi_command"),
            tests,
            "{tests}"
        );
    }
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
    let mut upgraded =
        RepositoryRenderModel::from_answers(&load_saved(&legacy_model("rust"), "rust")).unwrap();
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

#[test]
fn capability_refresh_keeps_a_removed_boundary_check_removed() {
    let apps = format!("{APP}{ADMIN_APP}");
    let mut scaffold = scaffold_answers(&format!("backend_language = \"rust\"\n{apps}"));
    scaffold.enable_client_only_contract_drift();
    let mut saved = RepositoryRenderModel::from_answers(&scaffold).unwrap();
    saved
        .prepare_runner_epoch(jig_context::CURRENT_CONTRACT_VERSION)
        .unwrap();
    saved
        .prepare_freshness_epoch(jig_context::CURRENT_CONTRACT_VERSION)
        .unwrap();
    let boundary = target_id("repo", "frontend-public-boundary").unwrap();
    let readopt = |saved: &RepositoryRenderModel| {
        let temp = TempDir::new().unwrap();
        write_scaffold_document_tests(temp.path(), "rust");
        let answers = readopt_apps_with_sqlx_at(temp.path(), saved, &apps).unwrap();
        RepositoryRenderModel::from_answers(&answers).unwrap()
    };

    // The refresh regenerates the boundary check that the model has.
    assert!(has_target(
        &readopt(&saved),
        "repo:frontend-public-boundary"
    ));

    let command = "repo_frontend_public_boundary_command";
    assert!(saved.commands.contains_key(command));
    saved.actions.retain(|action| action.target != boundary);
    for profile in &mut saved.profiles {
        profile.targets.retain(|target| *target != boundary);
    }
    saved.commands.remove(command);

    let model = readopt(&saved);

    assert!(
        model
            .components
            .iter()
            .any(|component| component.adapters.iter().any(|adapter| adapter == "sqlx"))
    );
    assert!(!has_target(&model, "repo:frontend-public-boundary"));
    assert!(!model.commands.contains_key(command));
    assert!(
        model
            .profiles
            .iter()
            .all(|profile| !profile.targets.contains(&boundary))
    );
}

#[test]
fn capability_refresh_adds_the_document_check_to_a_custom_older_model() {
    // An update kept the customized older model as its owner left it.
    let mut saved = legacy_model("rust");
    super::backend_freshness::customize_frontend(&mut saved);
    let updated = RepositoryRenderModel::from_answers(&load_saved(&saved, "rust")).unwrap();
    assert!(!has_target(&updated, "api:openapi"));

    let answers = readopt_with_sqlx(&updated).unwrap();
    let model = RepositoryRenderModel::from_answers(&answers).unwrap();

    assert!(has_target(&model, "api:openapi"));
}
