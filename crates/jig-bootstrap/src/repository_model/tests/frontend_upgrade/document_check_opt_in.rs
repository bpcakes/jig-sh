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

        let answers = load_saved_adding_check(&saved, backend);
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
