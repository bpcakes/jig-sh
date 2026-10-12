//! A repository from before the document check gains it on update only when
//! its backend still has the scaffold's document tests, which the check runs.
//! The tests belong to the application, which may have renamed or moved them.

use super::backend_freshness::{drift_reads_backend, find};
use super::*;

const GO_TEST: &str =
    "package httpapi\n\nimport \"testing\"\n\nfunc TestOpenAPIIsCurrent(t *testing.T) {}\n";

/// A repository with `files`, from the scaffold or its owner.
fn repository(files: &[(&str, &str)]) -> TempDir {
    let temp = TempDir::new().unwrap();
    for (path, contents) in files {
        let path = temp.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    temp
}

fn rust_test(name: &str) -> String {
    format!("#[cfg(test)]\nmod tests {{\n    #[test]\n    fn {name}() {{}}\n}}\n")
}

#[test]
fn managed_update_adds_the_document_check_only_beside_its_tests() {
    let public = rust_test("committed_public_openapi_is_current");
    let admin = rust_test("committed_admin_openapi_is_current");
    let renamed = rust_test("public_openapi_document_matches");
    let renamed_go = GO_TEST.replace("TestOpenAPIIsCurrent", "TestOpenAPIDocumentMatches");
    for (backend, label, files, checked) in [
        (
            "rust",
            "scaffold",
            vec![("crates/example-project-http/src/lib.rs", public.as_str())],
            true,
        ),
        // Cargo runs the tests of the whole workspace.
        (
            "rust",
            "moved test",
            vec![("apps/example-project-api/tests/openapi.rs", &public)],
            true,
        ),
        (
            "rust",
            "admin document",
            vec![
                ("crates/example-project-http/src/lib.rs", &public),
                ("crates/example-project-admin-http/src/tests.rs", &admin),
                ("openapi/admin.json", "{}\n"),
            ],
            true,
        ),
        (
            "rust",
            "renamed test",
            vec![("crates/example-project-http/src/lib.rs", &renamed)],
            false,
        ),
        (
            "rust",
            "admin document without its test",
            vec![
                ("crates/example-project-http/src/lib.rs", &public),
                ("openapi/admin.json", "{}\n"),
            ],
            false,
        ),
        ("rust", "no tests", vec![], false),
        (
            "go",
            "scaffold",
            vec![("internal/httpapi/httpapi_test.go", GO_TEST)],
            true,
        ),
        (
            "go",
            "other test file",
            vec![("internal/httpapi/openapi_test.go", GO_TEST)],
            true,
        ),
        (
            "go",
            "renamed test",
            vec![("internal/httpapi/httpapi_test.go", &renamed_go)],
            false,
        ),
        (
            "go",
            "moved package",
            vec![("internal/api/api_test.go", GO_TEST)],
            false,
        ),
        // Go compiles tests only from test files.
        (
            "go",
            "not a test file",
            vec![("internal/httpapi/httpapi.go", GO_TEST)],
            false,
        ),
    ] {
        let label = format!("{backend} {label}");
        let repository = repository(&files);

        let answers = load_saved_at(repository.path(), &legacy_model(backend), backend);
        let mut upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        // Without its tests the check could never pass, so the repository
        // keeps the drift check that exports the documents from the backend,
        // and stays generated.
        assert!(answers.authored_repository().is_none(), "{label}");
        assert_eq!(find(&upgraded, "api:openapi").is_some(), checked, "{label}");
        assert_eq!(
            upgraded.commands.contains_key("api_openapi_command"),
            checked,
            "{label}"
        );
        assert!(drift_reads_backend(&upgraded), "{label}");

        // The next update reads back what this one wrote.
        upgraded
            .prepare_runner_epoch(jig_context::CURRENT_CONTRACT_VERSION)
            .unwrap();
        upgraded
            .prepare_freshness_epoch(jig_context::CURRENT_CONTRACT_VERSION)
            .unwrap();
        let answers = load_saved_at(repository.path(), &upgraded, backend);
        let reloaded = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert!(answers.authored_repository().is_none(), "{label}");
        assert_eq!(find(&reloaded, "api:openapi").is_some(), checked, "{label}");

        // Once added, the check stays, and fails, if its tests are renamed.
        if checked {
            let renamed = self::repository(&[]);
            let answers = load_saved_at(renamed.path(), &upgraded, backend);
            let reloaded = RepositoryRenderModel::from_answers(&answers).unwrap();
            assert!(answers.authored_repository().is_none(), "{label}");
            assert!(find(&reloaded, "api:openapi").is_some(), "{label}");
        }
    }
}

/// A scaffold's own check fails when its tests are renamed, as documented:
/// generation keeps it, and the model stays generated. A check its owner
/// removed stays removed, with or without the tests.
#[test]
fn managed_update_leaves_scaffold_document_checks_to_their_owner() {
    for backend in ["rust", "go"] {
        let repository = repository(&[]);

        let answers = load_saved_at(repository.path(), &saved_current_model(backend), backend);
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

        let answers = load_saved_at(repository.path(), &removed, backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_some(), "{backend}");
        assert!(find(&upgraded, "api:openapi").is_none(), "{backend}");
    }
}

/// An update that left the check out keeps the model marked as one from
/// before the check, so the update after the tests return adds it, and the
/// model ends where an update beside the tests would have left it.
#[test]
fn managed_update_adds_the_document_check_once_its_tests_exist() {
    for backend in ["rust", "go"] {
        let repository = repository(&[]);
        let answers = load_saved_at(repository.path(), &legacy_model(backend), backend);
        let mut upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert!(find(&upgraded, "api:openapi").is_none(), "{backend}");
        upgraded
            .prepare_runner_epoch(jig_context::CURRENT_CONTRACT_VERSION)
            .unwrap();
        upgraded
            .prepare_freshness_epoch(jig_context::CURRENT_CONTRACT_VERSION)
            .unwrap();

        write_scaffold_document_tests(repository.path(), backend);
        let answers = load_saved_at(repository.path(), &upgraded, backend);
        let reloaded = RepositoryRenderModel::from_answers(&answers).unwrap();

        let direct =
            RepositoryRenderModel::from_answers(&load_saved(&legacy_model(backend), backend))
                .unwrap();
        assert!(answers.authored_repository().is_none(), "{backend}");
        assert!(find(&reloaded, "api:openapi").is_some(), "{backend}");
        assert_eq!(
            reloaded.authored_toml().unwrap(),
            direct.authored_toml().unwrap(),
            "{backend}"
        );
        assert_eq!(reloaded.commands, direct.commands, "{backend}");
    }
}

/// An owner who added the generated check by hand, beside the drift check
/// from before it, has the check, so the model no longer predates it and
/// stays generated.
#[test]
fn managed_update_keeps_a_document_check_added_by_hand() {
    for backend in ["rust", "go"] {
        let mut generated =
            RepositoryRenderModel::from_answers(&load_saved(&legacy_model(backend), backend))
                .unwrap();
        generated
            .prepare_runner_epoch(jig_context::CURRENT_CONTRACT_VERSION)
            .unwrap();
        generated
            .prepare_freshness_epoch(jig_context::CURRENT_CONTRACT_VERSION)
            .unwrap();
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
