//! The backend document check proves that the committed documents match the
//! backend, whatever the general backend test commands run. A scaffold's
//! drift check, beside its client-only contract script, reads no backend; one
//! upgraded from an earlier generator keeps reading it, since its own script
//! still exports from the backend.

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

/// A model generated for a repository upgraded from an earlier generator.
fn generated_model(backend: &str, answers: &str) -> RepositoryRenderModel {
    RepositoryRenderModel::from_answers(&scaffold_answers(&format!(
        "backend_language = {backend:?}\n{answers}{APP}"
    )))
    .unwrap()
}

pub(super) fn find<'a>(model: &'a RepositoryRenderModel, target: &str) -> Option<&'a ActionSpec> {
    model
        .actions
        .iter()
        .find(|item| item.target.to_string() == target)
}

pub(super) fn drift_reads_backend(model: &RepositoryRenderModel) -> bool {
    let drift = &find(model, "repo:frontend-contract-drift").unwrap().inputs;
    BACKEND_CONTRACT_INPUTS
        .iter()
        .all(|input| drift.iter().any(|item| item == input))
}

fn document_check_mut(model: &mut RepositoryRenderModel) -> &mut ActionSpec {
    model
        .actions
        .iter_mut()
        .find(|item| item.target.to_string() == "api:openapi")
        .unwrap()
}

/// Marks `model` custom without touching its contract or backend checks.
pub(super) fn customize_frontend(model: &mut RepositoryRenderModel) {
    model
        .components
        .iter_mut()
        .find(|component| component.id.as_str() == "web")
        .unwrap()
        .description = Some("Authored web frontend.".into());
}

#[test]
fn generation_adds_a_document_check_that_test_commands_cannot_skip() {
    for backend in ["rust", "go"] {
        let command = openapi_document_check_command(backend == "go");
        let customized = custom_test_commands(backend).map(|(_, answer, custom)| {
            scaffolded_model(backend, &format!("{answer} = {custom:?}\n"))
        });
        for model in std::iter::once(scaffolded_model(backend, "")).chain(customized) {
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
        // An upgraded repository's own script exports from the backend.
        let upgraded = generated_model(backend, "");
        assert!(find(&upgraded, "api:openapi").is_some(), "{backend}");
        assert!(drift_reads_backend(&upgraded), "{backend}");
        assert_ne!(
            find(&upgraded, "repo:frontend-contract-drift")
                .unwrap()
                .description,
            find(
                &scaffolded_model(backend, ""),
                "repo:frontend-contract-drift"
            )
            .unwrap()
            .description,
            "{backend}"
        );
    }
}

#[test]
fn managed_update_keeps_a_scaffold_drift_check_client_only() {
    for backend in ["rust", "go"] {
        let saved = saved_current_model(backend);

        let answers = load_saved(&saved, backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_none(), "{backend}");
        assert!(!drift_reads_backend(&upgraded), "{backend}");
        assert_eq!(
            upgraded.authored_toml().unwrap(),
            scaffolded_model(backend, "").authored_toml().unwrap(),
            "{backend}"
        );
    }
}

#[test]
fn managed_update_keeps_a_document_check_removed_after_an_upgrade() {
    for backend in ["rust", "go"] {
        // The owner asks for the check; the drift check keeps reading the
        // backend, so its inputs alone look like those of an older generator.
        let answers = load_saved_adding_check(&legacy_model(backend), backend);
        let mut migrated = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert!(answers.authored_repository().is_none(), "{backend}");
        assert!(find(&migrated, "api:openapi").is_some(), "{backend}");
        assert!(drift_reads_backend(&migrated), "{backend}");
        migrated
            .prepare_runner_epoch(jig_context::CURRENT_CONTRACT_VERSION)
            .unwrap();
        migrated
            .prepare_freshness_epoch(jig_context::CURRENT_CONTRACT_VERSION)
            .unwrap();
        // Its owner then removes the check.
        let check = target_id("api", "openapi").unwrap();
        migrated.actions.retain(|action| action.target != check);
        for profile in &mut migrated.profiles {
            profile.targets.retain(|target| *target != check);
        }
        migrated.commands.remove("api_openapi_command");

        // It stays removed, even when the owner asks for the check again.
        for answers in [
            load_saved(&migrated, backend),
            load_saved_adding_check(&migrated, backend),
        ] {
            let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

            assert!(answers.authored_repository().is_some(), "{backend}");
            assert!(find(&upgraded, "api:openapi").is_none(), "{backend}");
            assert!(drift_reads_backend(&upgraded), "{backend}");
        }
    }
}

#[test]
fn managed_update_adds_no_document_check_to_a_custom_model() {
    for backend in ["rust", "go"] {
        let mut saved = legacy_model(backend);
        customize_frontend(&mut saved);

        let answers = load_saved(&saved, backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_some(), "{backend}");
        assert!(find(&upgraded, "api:openapi").is_none(), "{backend}");
        assert!(drift_reads_backend(&upgraded), "{backend}");
    }
}

#[test]
fn managed_update_adds_the_document_check_beside_customized_test_commands() {
    for backend in ["rust", "go"] {
        for (key, answer, custom) in custom_test_commands(backend) {
            let label = format!("{backend} {key}");
            let mut saved = legacy_model(backend);
            saved.commands.insert(key.into(), custom.into());

            let answers = load_saved_adding_check(&saved, backend);
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

/// Repositories scaffolded by the latest release upgrade in place: they stay
/// generated, gain the document check when their owner asks, and keep their
/// drift check reading the backend that their own contract script exports
/// from.
#[test]
fn managed_update_upgrades_scaffolds_from_the_latest_release() {
    for (backend, fixture) in [
        ("rust", include_str!("fixtures/v0.8.0-rust-react.jig.toml")),
        ("go", include_str!("fixtures/v0.8.0-go-react.jig.toml")),
    ] {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join(".jig.toml");
        fs::write(&path, fixture).unwrap();

        let (answers, _) =
            RenderAnswers::from_managed_answers_file(&path, temp.path(), false).unwrap();
        let unchanged = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert!(answers.authored_repository().is_none(), "{backend}");
        assert!(find(&unchanged, "api:openapi").is_none(), "{backend}");
        assert!(drift_reads_backend(&unchanged), "{backend}");

        let (answers, _) =
            RenderAnswers::from_managed_answers_file(&path, temp.path(), true).unwrap();
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_none(), "{backend}");
        assert!(find(&upgraded, "api:openapi").is_some(), "{backend}");
        assert!(drift_reads_backend(&upgraded), "{backend}");
        assert_eq!(
            find(&upgraded, "repo:frontend-contract-drift")
                .unwrap()
                .description,
            find(
                &generated_model(backend, ""),
                "repo:frontend-contract-drift"
            )
            .unwrap()
            .description,
            "{backend}"
        );
    }
}

#[test]
fn managed_update_keeps_backend_inputs_an_owner_added_to_a_client_only_drift_check() {
    for backend in ["rust", "go"] {
        // The owner made the scaffold's drift-check export again and listed
        // what it reads, which matches the inputs of an earlier generator.
        let mut saved = saved_current_model(backend);
        let drift = saved
            .actions
            .iter_mut()
            .find(|item| item.target.to_string() == "repo:frontend-contract-drift")
            .unwrap();
        drift.inputs.extend(
            [
                "Cargo.toml",
                "**/Cargo.toml",
                "**/*.rs",
                "go.mod",
                "**/go.mod",
                "**/*.go",
            ]
            .map(String::from),
        );
        drift.inputs.sort();
        drift.inputs.dedup();
        let authored = drift.inputs.clone();

        let answers = load_saved(&saved, backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_some(), "{backend}");
        assert_eq!(
            find(&upgraded, "repo:frontend-contract-drift")
                .unwrap()
                .inputs,
            authored,
            "{backend}"
        );
    }
}

#[test]
fn managed_update_keeps_a_model_custom_when_the_check_command_key_is_taken() {
    for backend in ["rust", "go"] {
        let mut saved = legacy_model(backend);
        saved.commands.insert(
            "api_openapi_command".into(),
            "printf skip-openapi-check".into(),
        );

        let answers = load_saved(&saved, backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_some(), "{backend}");
        assert!(find(&upgraded, "api:openapi").is_none(), "{backend}");
        assert_eq!(
            upgraded.commands["api_openapi_command"], "printf skip-openapi-check",
            "{backend}"
        );
        assert!(drift_reads_backend(&upgraded), "{backend}");
    }
}

/// An owner may run the document tests their own way, as a Go backend that
/// needs its workspace modules does. Generation keeps that command, as it
/// keeps the other backend commands, and the model stays generated.
#[test]
fn managed_update_keeps_a_customized_document_check_command() {
    for (backend, custom) in [
        (
            "rust",
            "cargo test --workspace --locked committed_ -- --format pretty",
        ),
        (
            "go",
            "go test -count=1 -run '^TestOpenAPIIsCurrent$' ./internal/httpapi",
        ),
    ] {
        let mut saved = saved_current_model(backend);
        saved
            .commands
            .insert("api_openapi_command".into(), custom.into());

        let answers = load_saved(&saved, backend);
        let mut upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        let mut current = scaffolded_model(backend, "");
        current
            .commands
            .insert("api_openapi_command".into(), custom.into());
        assert!(answers.authored_repository().is_none(), "{backend}");
        assert_eq!(
            upgraded.authored_toml().unwrap(),
            current.authored_toml().unwrap(),
            "{backend}"
        );
        assert_eq!(upgraded.commands, current.commands, "{backend}");

        // The next update reads back the command this one wrote.
        upgraded
            .prepare_runner_epoch(jig_context::CURRENT_CONTRACT_VERSION)
            .unwrap();
        upgraded
            .prepare_freshness_epoch(jig_context::CURRENT_CONTRACT_VERSION)
            .unwrap();
        let answers = load_saved(&upgraded, backend);
        let reloaded = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert!(answers.authored_repository().is_none(), "{backend}");
        assert_eq!(reloaded.commands, current.commands, "{backend}");
    }
}

#[test]
fn managed_update_keeps_a_removed_document_check_removed() {
    for backend in ["rust", "go"] {
        let mut saved = saved_current_model(backend);
        let check = target_id("api", "openapi").unwrap();
        saved.actions.retain(|action| action.target != check);
        for profile in &mut saved.profiles {
            profile.targets.retain(|target| *target != check);
        }
        saved.commands.remove("api_openapi_command");

        let answers = load_saved(&saved, backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_some(), "{backend}");
        assert!(find(&upgraded, "api:openapi").is_none(), "{backend}");
        assert_eq!(
            find(&upgraded, "repo:frontend-contract-drift")
                .unwrap()
                .inputs,
            find(&saved, "repo:frontend-contract-drift").unwrap().inputs,
            "{backend}"
        );
    }
}

#[test]
fn workflows_run_each_read_only_document_check_of_their_backend() {
    for (backend, other) in [("rust", "go"), ("go", "rust")] {
        let mut model = generated_model(backend, "");
        // An authored check may read more than the backend root, such as a
        // schema directory beside a nested backend.
        document_check_mut(&mut model)
            .inputs
            .push("schemas/**".into());
        let ci_inputs = |model: &RepositoryRenderModel| {
            if backend == "go" {
                model.go_ci_input_paths()
            } else {
                model.rust_ci_input_paths()
            }
        };
        assert_eq!(
            model.openapi_document_check_targets(backend),
            ["api:openapi"]
        );
        assert!(model.openapi_document_check_targets(other).is_empty());
        assert!(
            ci_inputs(&model).contains(&"schemas/**".into()),
            "{backend}"
        );

        // Adoption names the backend after the repository workspace, and an
        // owner may add another backend with its own document check.
        let mut renamed = model.clone();
        rename_backend(&mut renamed, "server");
        let mut worker = renamed
            .components
            .iter()
            .find(|component| component.id.as_str() == "server")
            .unwrap()
            .clone();
        worker.id = component_id("worker").unwrap();
        worker.root = "worker".into();
        renamed.components.push(worker);
        let mut worker_check = find(&renamed, "server:openapi").unwrap().clone();
        worker_check.target = target_id("worker", "openapi").unwrap();
        worker_check.inputs = vec!["worker/openapi/**".into()];
        renamed.actions.push(worker_check);
        assert_eq!(
            renamed.openapi_document_check_targets(backend),
            ["server:openapi", "worker:openapi"]
        );
        for input in ["schemas/**", "worker/openapi/**"] {
            assert!(
                ci_inputs(&renamed).contains(&input.into()),
                "{backend} {input}"
            );
        }

        // An authored action of the same name may generate the documents.
        let check = document_check_mut(&mut model);
        check.intent = ActionIntent::Generate;
        check.effects = vec![ActionEffect::Worktree];
        assert!(model.openapi_document_check_targets(backend).is_empty());
        assert!(
            !ci_inputs(&model).contains(&"schemas/**".into()),
            "{backend}"
        );
    }
}
