//! The drift check drops its backend inputs only while the backend tests
//! prove that the committed documents match the backend.

use super::*;

/// Backend test commands that skip the committed-document tests, as
/// `(command key, answer key, command)`.
fn custom_test_commands(backend: &str) -> [(&'static str, &'static str, &'static str); 2] {
    if backend == "rust" {
        [
            (
                "api_test_command",
                "rust_test_command",
                "cargo test --workspace -- --skip committed_",
            ),
            (
                "api_test_locked_command",
                "rust_test_locked_command",
                "cargo test --workspace --locked -- --skip committed_",
            ),
        ]
    } else {
        [
            (
                "api_test_command",
                "go_test_command",
                "go test -skip TestOpenAPIIsCurrent ./...",
            ),
            (
                "api_test_locked_command",
                "go_test_locked_command",
                "go mod verify && go test -mod=readonly -skip TestOpenAPIIsCurrent ./...",
            ),
        ]
    }
}

fn generated_model(backend: &str, answers: &str) -> RepositoryRenderModel {
    RepositoryRenderModel::from_answers(&scaffold_answers(&format!(
        "backend_language = {backend:?}\n{answers}{APP}"
    )))
    .unwrap()
}

fn inputs<'a>(model: &'a RepositoryRenderModel, target: &str) -> &'a [String] {
    &model
        .actions
        .iter()
        .find(|item| item.target.to_string() == target)
        .unwrap()
        .inputs
}

fn drift_reads_backend(model: &RepositoryRenderModel) -> bool {
    let drift = inputs(model, "repo:frontend-contract-drift");
    BACKEND_CONTRACT_INPUTS
        .iter()
        .all(|input| drift.iter().any(|item| item == input))
}

/// Loads `saved` and asserts that the drift check keeps its saved inputs.
fn assert_saved_drift_inputs_kept(
    saved: &RepositoryRenderModel,
    backend: &str,
    label: &str,
) -> RepositoryRenderModel {
    let answers = load_saved(saved, backend);
    let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();
    assert!(answers.authored_repository().is_some(), "{label}");
    assert_eq!(
        inputs(&upgraded, "repo:frontend-contract-drift"),
        inputs(saved, "repo:frontend-contract-drift"),
        "{label}"
    );
    upgraded
}

#[test]
fn generation_narrows_the_drift_check_only_behind_generated_backend_test_commands() {
    for backend in ["rust", "go"] {
        let generated = generated_model(backend, "");
        assert!(
            inputs(&generated, "repo:frontend-contract-drift")
                .iter()
                .all(|input| !BACKEND_CONTRACT_INPUTS.contains(&input.as_str())),
            "{backend}"
        );
        for (_, answer, custom) in custom_test_commands(backend) {
            let generated = generated_model(backend, &format!("{answer} = {custom:?}\n"));
            assert!(drift_reads_backend(&generated), "{backend} {answer}");
        }
    }
}

#[test]
fn managed_update_keeps_answer_configured_test_commands_generated() {
    for backend in ["rust", "go"] {
        for (key, answer, custom) in custom_test_commands(backend) {
            let mut narrowed = generated_model(backend, "");
            narrowed
                .prepare_runner_epoch(jig_context::CURRENT_CONTRACT_VERSION)
                .unwrap();
            narrowed
                .prepare_freshness_epoch(jig_context::CURRENT_CONTRACT_VERSION)
                .unwrap();
            let current = generated_model(backend, &format!("{answer} = {custom:?}\n"));
            for (shape, mut saved) in [("legacy", legacy_model(backend)), ("narrowed", narrowed)] {
                let label = format!("{backend} {key} {shape}");
                saved.commands.insert(key.into(), custom.into());

                let answers = load_saved(&saved, backend);
                let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

                assert!(answers.authored_repository().is_none(), "{label}");
                assert_eq!(
                    upgraded.authored_toml().unwrap(),
                    current.authored_toml().unwrap(),
                    "{label}"
                );
                assert_eq!(upgraded.commands[key], custom, "{label}");
                assert!(drift_reads_backend(&upgraded), "{label}");
            }
        }
    }
}

#[test]
fn managed_update_keeps_drift_backend_inputs_while_backend_tests_skip_generated_inputs() {
    for backend in ["rust", "go"] {
        // Authored inputs without the documents, and with only the documents.
        for documents_only in [false, true] {
            let mut saved = legacy_model(backend);
            let test = saved
                .actions
                .iter_mut()
                .find(|item| item.target.to_string() == "api:test")
                .unwrap();
            if documents_only {
                test.inputs = vec!["openapi/**".into()];
            }
            test.provenance
                .insert("inputs".into(), FieldProvenance::Declared);
            let authored = test.inputs.clone();
            let label = format!("{backend} documents only: {documents_only}");

            let upgraded = assert_saved_drift_inputs_kept(&saved, backend, &label);

            assert_eq!(inputs(&upgraded, "api:test"), authored, "{label}");
        }
    }
}

#[test]
fn managed_update_keeps_drift_backend_inputs_while_backend_tests_differ_from_generation() {
    // The answers replace this hand-written command with the generated
    // default, but a custom model keeps running its own command.
    let mut saved = legacy_model("rust");
    saved
        .commands
        .insert("api_test_command".into(), "cargo test --workspace".into());
    saved
        .components
        .iter_mut()
        .find(|component| component.id.as_str() == "web")
        .unwrap()
        .description = Some("Authored web frontend.".into());
    assert_saved_drift_inputs_kept(&saved, "rust", "hand-written cargo test");

    // The generated Go command skips the document test outside the module root.
    let mut saved = legacy_model("go");
    let test = saved
        .actions
        .iter_mut()
        .find(|item| item.target.to_string() == "api:test-locked")
        .unwrap();
    let (ActionRunner::Command {
        working_directory, ..
    }
    | ActionRunner::Shell {
        working_directory, ..
    }) = &mut test.runner
    else {
        panic!("generated backend tests run a command");
    };
    *working_directory = Some("cmd".into());
    assert_saved_drift_inputs_kept(&saved, "go", "go working directory");
}
