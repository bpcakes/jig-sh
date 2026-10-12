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

    // Adoption generates backend actions only for a backend at the root, so
    // none can own a regenerated document check.
    let answers = readopt_with_sqlx(&saved).unwrap();
    let model = RepositoryRenderModel::from_answers(&answers).unwrap();
    for target in [
        "repo:frontend-contract-drift",
        "repo:frontend-public-boundary",
    ] {
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
