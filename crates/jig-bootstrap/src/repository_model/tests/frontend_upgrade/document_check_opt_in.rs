//! A repository from before the document check gains it only when its owner
//! asks, with `jig update --add-openapi-check`: the check runs the scaffold's
//! document tests by name, and those belong to the application.

use super::backend_freshness::{customize_frontend, drift_reads_backend, find};
use super::*;

fn prepared(mut model: RepositoryRenderModel) -> RepositoryRenderModel {
    model
        .prepare_runner_epoch(jig_context::CURRENT_CONTRACT_VERSION)
        .unwrap();
    model
        .prepare_freshness_epoch(jig_context::CURRENT_CONTRACT_VERSION)
        .unwrap();
    model
}

#[test]
fn managed_update_leaves_an_older_model_without_the_document_check() {
    for backend in ["rust", "go"] {
        let (answers, notes) = update_answers(&legacy_model(backend), backend, false);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_none(), "{backend}");
        assert!(find(&upgraded, "api:openapi").is_none(), "{backend}");
        assert!(
            !upgraded.commands.contains_key("api_openapi_command"),
            "{backend}"
        );
        assert!(drift_reads_backend(&upgraded), "{backend}");
        assert!(
            notes
                .iter()
                .any(|note| note.contains("jig update --add-openapi-check")),
            "{backend}: {notes:?}"
        );

        // The next update finds the model as this one left it.
        let (answers, notes) = update_answers(&prepared(upgraded), backend, false);
        let reloaded = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert!(answers.authored_repository().is_none(), "{backend}");
        assert!(find(&reloaded, "api:openapi").is_none(), "{backend}");
        assert!(
            notes
                .iter()
                .any(|note| note.contains("jig update --add-openapi-check")),
            "{backend}: {notes:?}"
        );
    }
}

/// An owner who asks gets the check, which then stays, and the model ends
/// where it would have had it asked on the first update.
#[test]
fn managed_update_adds_the_document_check_when_its_owner_asks() {
    for backend in ["rust", "go"] {
        let upgraded = prepared(
            RepositoryRenderModel::from_answers(&load_saved(&legacy_model(backend), backend))
                .unwrap(),
        );

        let (answers, notes) = update_answers(&upgraded, backend, true);
        let added = RepositoryRenderModel::from_answers(&answers).unwrap();

        let direct = RepositoryRenderModel::from_answers(&load_saved_adding_check(
            &legacy_model(backend),
            backend,
        ))
        .unwrap();
        assert!(answers.authored_repository().is_none(), "{backend}");
        assert!(find(&added, "api:openapi").is_some(), "{backend}");
        assert!(notes.is_empty(), "{backend}: {notes:?}");
        assert_eq!(
            added.authored_toml().unwrap(),
            direct.authored_toml().unwrap(),
            "{backend}"
        );
        assert_eq!(added.commands, direct.commands, "{backend}");

        let answers = load_saved(&prepared(added), backend);
        let kept = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert!(answers.authored_repository().is_none(), "{backend}");
        assert!(find(&kept, "api:openapi").is_some(), "{backend}");
    }
}

/// A backend whose document tests differ from the scaffold's names its own
/// command before it asks.
#[test]
fn managed_update_runs_the_owners_document_command_when_asked() {
    for (backend, custom) in [
        (
            "rust",
            "cargo test --workspace --locked public_document -- --format pretty",
        ),
        (
            "go",
            "go test -count=1 -run '^TestPublicDocument$' ./internal/api",
        ),
    ] {
        let mut saved = legacy_model(backend);
        saved
            .commands
            .insert("api_openapi_command".into(), custom.into());

        // Until asked, the update keeps the command and says how to add the
        // check that runs it.
        let (answers, notes) = update_answers(&saved, backend, false);
        let unchanged = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert!(find(&unchanged, "api:openapi").is_none(), "{backend}");
        assert_eq!(
            unchanged.commands["api_openapi_command"], custom,
            "{backend}"
        );
        assert!(
            notes
                .iter()
                .any(|note| note.contains("jig update --add-openapi-check")),
            "{backend}: {notes:?}"
        );

        // The update kept the model one from before the check, so asking
        // afterwards adds the check, which runs the owner's command.
        let answers = load_saved_adding_check(&prepared(unchanged), backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_none(), "{backend}");
        assert!(find(&upgraded, "api:openapi").is_some(), "{backend}");
        assert_eq!(
            upgraded.commands["api_openapi_command"], custom,
            "{backend}"
        );
    }
}

/// An owner who added the generated check by hand, beside the drift check
/// from before it, has the check, so the model no longer predates it and
/// stays generated.
#[test]
fn managed_update_keeps_a_document_check_added_by_hand() {
    for backend in ["rust", "go"] {
        let generated = prepared(
            RepositoryRenderModel::from_answers(&load_saved_adding_check(
                &legacy_model(backend),
                backend,
            ))
            .unwrap(),
        );
        let check = find(&generated, "api:openapi").unwrap().clone();
        let mut saved = legacy_model(backend);
        saved.actions.push(check.clone());
        saved.actions.sort_by(|a, b| a.target.cmp(&b.target));
        for profile in &mut saved.profiles {
            if generated
                .profiles
                .iter()
                .any(|item| item.id == profile.id && item.targets.contains(&check.target))
            {
                profile.targets.push(check.target.clone());
                profile.targets.sort();
            }
        }
        saved.commands.insert(
            "api_openapi_command".into(),
            generated.commands["api_openapi_command"].clone(),
        );

        let answers = load_saved(&saved, backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_none(), "{backend}");
        assert!(find(&upgraded, "api:openapi").is_some(), "{backend}");
    }
}

/// A scaffold's own check fails when its tests are renamed, as documented:
/// generation keeps it, and the model stays generated. A check its owner
/// removed stays removed, even when the owner asks for the check.
#[test]
fn managed_update_leaves_scaffold_document_checks_to_their_owner() {
    for backend in ["rust", "go"] {
        let answers = load_saved(&saved_current_model(backend), backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_none(), "{backend}");
        assert!(find(&upgraded, "api:openapi").is_some(), "{backend}");
        assert!(!drift_reads_backend(&upgraded), "{backend}");

        let mut removed = saved_current_model(backend);
        let check = target_id("api", "openapi").unwrap();
        removed.actions.retain(|action| action.target != check);
        for profile in &mut removed.profiles {
            profile.targets.retain(|target| *target != check);
        }
        removed.commands.remove("api_openapi_command");

        for add_openapi_check in [false, true] {
            let (answers, _) = update_answers(&removed, backend, add_openapi_check);
            let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

            assert!(answers.authored_repository().is_some(), "{backend}");
            assert!(find(&upgraded, "api:openapi").is_none(), "{backend}");
        }
    }
}

/// An update keeps a customized model from before the check as its owner left
/// it, still marked as from before the check: asking leaves the custom model
/// alone and says so, and once the customization is reverted, asking adds the
/// check.
#[test]
fn managed_update_keeps_a_custom_model_eligible_for_the_document_check() {
    for backend in ["rust", "go"] {
        let mut saved = legacy_model(backend);
        customize_frontend(&mut saved);

        // Asking would not change a custom model, so the update says how to
        // declare the check instead.
        let (answers, notes) = update_answers(&saved, backend, false);
        assert!(answers.authored_repository().is_some(), "{backend}");
        assert!(
            notes
                .iter()
                .any(|note| note.contains("declare it in .jig.toml")),
            "{backend}: {notes:?}"
        );
        assert!(
            !notes
                .iter()
                .any(|note| note.contains("--add-openapi-check")),
            "{backend}: {notes:?}"
        );

        let (answers, notes) = update_answers(&saved, backend, true);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert!(answers.authored_repository().is_some(), "{backend}");
        assert!(find(&upgraded, "api:openapi").is_none(), "{backend}");
        assert!(
            notes
                .iter()
                .any(|note| note.contains("left the custom repository model")),
            "{backend}: {notes:?}"
        );

        let mut reverted = upgraded.clone();
        let web = reverted
            .components
            .iter_mut()
            .find(|component| component.id.as_str() == "web")
            .unwrap();
        web.description = legacy_model(backend)
            .components
            .iter()
            .find(|component| component.id.as_str() == "web")
            .unwrap()
            .description
            .clone();
        let answers = load_saved_adding_check(&reverted, backend);
        let regenerated = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_none(), "{backend}");
        assert!(find(&regenerated, "api:openapi").is_some(), "{backend}");
    }
}

/// Without API contracts there is no document check to add, and asking says
/// so instead of suggesting that the owner declare one, also without a saved
/// repository model.
#[test]
fn managed_update_says_the_document_check_needs_api_contracts() {
    let contracts_note = "--add-openapi-check applies only to a repository model with API contracts, and this one has none.";
    for backend in ["rust", "go"] {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join(".jig.toml");
        fs::write(
            &path,
            format!(
                "repo_name = \"ExampleProject\"\nbackend_language = {backend:?}\nsqlx_enabled = false\nschema_dump_enabled = false\n{APP}"
            ),
        )
        .unwrap();
        let (_, notes) =
            RenderAnswers::from_managed_answers_file(&path, temp.path(), true).unwrap();
        assert!(
            notes.iter().any(|note| note == contracts_note),
            "{backend}: {notes:?}"
        );

        let model = RepositoryRenderModel::from_answers(&answers(&format!(
            "backend_language = {backend:?}\n{APP}"
        )))
        .unwrap();
        assert!(!model.frontend_contracts_enabled(), "{backend}");

        let (answers, notes) = update_answers(&model, backend, true);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(find(&upgraded, "api:openapi").is_none(), "{backend}");
        assert_eq!(
            notes
                .iter()
                .filter(|note| note.contains("openapi"))
                .collect::<Vec<_>>(),
            [contracts_note],
            "{backend}"
        );
    }
}

/// A custom model without the generated drift check may keep its API
/// contracts under checks of its own, so asking says how to declare the
/// document check instead of that the model has no contracts.
#[test]
fn managed_update_says_how_to_declare_the_check_beside_contract_checks_of_its_own() {
    for backend in ["rust", "go"] {
        let mut model = legacy_model(backend);
        let drift = target_id("repo", "frontend-contract-drift").unwrap();
        model.actions.retain(|action| action.target != drift);
        for action in &mut model.actions {
            action.depends_on.retain(|target| *target != drift);
        }
        for profile in &mut model.profiles {
            profile.targets.retain(|target| *target != drift);
        }
        model
            .commands
            .remove("repo_frontend_contract_drift_command");

        let (_, notes) = update_answers(&model, backend, true);

        assert_eq!(
            notes
                .iter()
                .filter(|note| note.contains("openapi"))
                .collect::<Vec<_>>(),
            [
                "--add-openapi-check found none of the generated API contract checks in this custom repository model and left it as it is; declare api:openapi in .jig.toml to add the check."
            ],
            "{backend}"
        );
    }
}

/// An action of the check's name on a frontend, such as one that checks the
/// rendered API reference, is the owner's own: the update still says how to
/// add the document check, and the model stays marked as from before it,
/// even when its owner asks.
#[test]
fn managed_update_offers_the_check_beside_a_frontend_openapi_action() {
    for backend in ["rust", "go"] {
        let mut saved = legacy_model(backend);
        let mut reference = find(&saved, "web:typecheck").unwrap().clone();
        reference.target = target_id("web", "openapi").unwrap();
        saved.actions.push(reference);
        saved.actions.sort_by(|a, b| a.target.cmp(&b.target));
        let historical = |model: &RepositoryRenderModel| {
            find(model, "repo:frontend-contract-drift")
                .unwrap()
                .description
                .as_deref()
                == Some(HISTORICAL_DRIFT_DESCRIPTION)
        };

        let (answers, notes) = update_answers(&saved, backend, false);
        let updated = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert!(answers.authored_repository().is_some(), "{backend}");
        assert!(historical(&updated), "{backend}");
        assert!(
            notes
                .iter()
                .any(|note| note.contains("declare it in .jig.toml")),
            "{backend}: {notes:?}"
        );

        let (answers, notes) = update_answers(&prepared(updated), backend, true);
        let asked = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert!(find(&asked, "api:openapi").is_none(), "{backend}");
        assert!(historical(&asked), "{backend}");
        assert!(
            notes
                .iter()
                .any(|note| note.contains("left the custom repository model")),
            "{backend}: {notes:?}"
        );
    }
}

/// A document check counts on whichever backend component adoption or the
/// owner gave it, so the update has nothing to add or say.
#[test]
fn managed_update_finds_the_document_check_on_a_renamed_backend() {
    for backend in ["rust", "go"] {
        let current = saved_current_model(backend);
        let mut saved = legacy_model(backend);
        rename_backend(&mut saved, "server");
        let mut check = find(&current, "api:openapi").unwrap().clone();
        check.target = target_id("server", "openapi").unwrap();
        saved.actions.push(check);
        saved.actions.sort_by(|a, b| a.target.cmp(&b.target));
        saved.commands.insert(
            "api_openapi_command".into(),
            current.commands["api_openapi_command"].clone(),
        );

        for add_openapi_check in [false, true] {
            let (answers, notes) = update_answers(&saved, backend, add_openapi_check);
            let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();
            assert!(find(&upgraded, "server:openapi").is_some(), "{backend}");
            assert!(find(&upgraded, "api:openapi").is_none(), "{backend}");
            assert_ne!(
                find(&upgraded, "repo:frontend-contract-drift")
                    .unwrap()
                    .description
                    .as_deref(),
                Some(HISTORICAL_DRIFT_DESCRIPTION),
                "{backend}"
            );
            assert!(
                !notes.iter().any(|note| note.contains("openapi")),
                "{backend}: {notes:?}"
            );
        }
    }
}
