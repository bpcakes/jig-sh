//! The backend document check proves that the committed documents match the
//! backend, whatever the general backend test commands run; without it, the
//! drift check keeps reading the backend.

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

fn find<'a>(model: &'a RepositoryRenderModel, target: &str) -> Option<&'a ActionSpec> {
    model
        .actions
        .iter()
        .find(|item| item.target.to_string() == target)
}

fn drift_reads_backend(model: &RepositoryRenderModel) -> bool {
    let drift = &find(model, "repo:frontend-contract-drift").unwrap().inputs;
    BACKEND_CONTRACT_INPUTS
        .iter()
        .all(|input| drift.iter().any(|item| item == input))
}

/// A legacy model whose owner already added the generated document check.
fn legacy_model_with_document_check(backend: &str) -> RepositoryRenderModel {
    let mut current = generated_model(backend, "");
    current
        .prepare_runner_epoch(jig_context::CURRENT_CONTRACT_VERSION)
        .unwrap();
    current
        .prepare_freshness_epoch(jig_context::CURRENT_CONTRACT_VERSION)
        .unwrap();
    let mut saved = legacy_model(backend);
    let check = find(&current, "api:openapi").unwrap().clone();
    let index = saved
        .actions
        .iter()
        .position(|action| action.target > check.target)
        .unwrap();
    saved.actions.insert(index, check);
    saved.profiles.clone_from(&current.profiles);
    saved.commands.insert(
        "api_openapi_command".into(),
        current.commands["api_openapi_command"].clone(),
    );
    saved
}

/// Marks `model` custom without touching its contract or backend checks.
fn customize_frontend(model: &mut RepositoryRenderModel) {
    model
        .components
        .iter_mut()
        .find(|component| component.id.as_str() == "web")
        .unwrap()
        .description = Some("Authored web frontend.".into());
}

#[test]
fn generation_adds_a_document_check_that_test_commands_cannot_skip() {
    for (backend, command) in [
        ("rust", "cargo test --workspace openapi_is_current"),
        (
            "go",
            "go test -count=1 -run '^TestOpenAPIIsCurrent$' ./internal/httpapi",
        ),
    ] {
        let customized = custom_test_commands(backend).map(|(_, answer, custom)| {
            generated_model(backend, &format!("{answer} = {custom:?}\n"))
        });
        for model in std::iter::once(generated_model(backend, "")).chain(customized) {
            let check = find(&model, "api:openapi").unwrap();
            assert_eq!(model.commands["api_openapi_command"], command, "{backend}");
            let mut inputs = find(&model, "api:test").unwrap().inputs.clone();
            inputs.push("openapi/**".into());
            inputs.sort();
            assert_eq!(check.inputs, inputs, "{backend}");
            assert!(
                model.profiles[0].targets.contains(&check.target),
                "{backend}"
            );
            for test in ["api:test", "api:test-locked"] {
                assert!(
                    !find(&model, test)
                        .unwrap()
                        .inputs
                        .contains(&"openapi/**".into()),
                    "{backend} {test}"
                );
            }
            assert!(!drift_reads_backend(&model), "{backend}");
        }
    }
}

#[test]
fn managed_update_adds_the_document_check_beside_customized_test_commands() {
    for backend in ["rust", "go"] {
        for (key, answer, custom) in custom_test_commands(backend) {
            let label = format!("{backend} {key}");
            let mut saved = legacy_model(backend);
            saved.commands.insert(key.into(), custom.into());

            let answers = load_saved(&saved, backend);
            let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

            let current = generated_model(backend, &format!("{answer} = {custom:?}\n"));
            assert!(answers.authored_repository().is_none(), "{label}");
            assert_eq!(
                upgraded.authored_toml().unwrap(),
                current.authored_toml().unwrap(),
                "{label}"
            );
            assert_eq!(upgraded.commands, current.commands, "{label}");
            assert!(find(&upgraded, "api:openapi").is_some(), "{label}");
        }
    }
}

#[test]
fn managed_update_narrows_the_drift_check_of_a_custom_model_behind_its_document_check() {
    for backend in ["rust", "go"] {
        let mut saved = legacy_model_with_document_check(backend);
        customize_frontend(&mut saved);

        let answers = load_saved(&saved, backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_some(), "{backend}");
        assert_eq!(
            find(&upgraded, "repo:frontend-contract-drift")
                .unwrap()
                .inputs,
            find(
                &generated_model(backend, ""),
                "repo:frontend-contract-drift"
            )
            .unwrap()
            .inputs,
            "{backend}"
        );
    }
}

#[test]
fn managed_update_keeps_drift_backend_inputs_without_the_generated_document_check() {
    type Customize = fn(&mut ActionSpec, &mut BTreeMap<String, String>);
    let cases: [(&str, Customize); 3] = [
        ("documents only", |check, _| {
            check.inputs = vec!["openapi/**".into()];
            check
                .provenance
                .insert("inputs".into(), FieldProvenance::Declared);
        }),
        ("custom command", |_, commands| {
            commands.insert(
                "api_openapi_command".into(),
                "scripts/check-openapi.sh".into(),
            );
        }),
        ("elsewhere", |check, _| {
            let (ActionRunner::Command {
                working_directory, ..
            }
            | ActionRunner::Shell {
                working_directory, ..
            }) = &mut check.runner
            else {
                panic!("the generated document check runs a command");
            };
            *working_directory = Some("cmd".into());
        }),
    ];
    for backend in ["rust", "go"] {
        // A custom model without the check does not gain one.
        let mut saved = legacy_model(backend);
        customize_frontend(&mut saved);
        let answers = load_saved(&saved, backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert!(answers.authored_repository().is_some(), "{backend}");
        assert!(find(&upgraded, "api:openapi").is_none(), "{backend}");
        assert!(drift_reads_backend(&upgraded), "{backend}");

        for (label, customize) in cases {
            let label = format!("{backend} {label}");
            let mut saved = legacy_model_with_document_check(backend);
            let mut check = find(&saved, "api:openapi").unwrap().clone();
            customize(&mut check, &mut saved.commands);
            let authored = check.inputs.clone();
            let index = saved
                .actions
                .iter()
                .position(|action| action.target == check.target)
                .unwrap();
            saved.actions[index] = check;

            let answers = load_saved(&saved, backend);
            let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

            assert!(answers.authored_repository().is_some(), "{label}");
            assert!(drift_reads_backend(&upgraded), "{label}");
            assert_eq!(
                find(&upgraded, "api:openapi").unwrap().inputs,
                authored,
                "{label}"
            );
        }
    }
}
