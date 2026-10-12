use std::path::Path;

use super::*;

mod backend_freshness;
mod document_commands;
mod legacy_document_tests;
mod readoption;

const APP: &str = r#"
[[frontend_apps]]
name = "web"
dir = "frontend/web"
coverage_threshold = 80
kind = "vite"
role = "spa"
"#;

const ADMIN_APP: &str = r#"
[[frontend_apps]]
name = "admin"
dir = "frontend/admin"
coverage_threshold = 85
kind = "vite"
role = "admin"
"#;

// Backend inputs the contract checks read before the lockfile cutover.
const LEGACY_BACKEND_INPUTS: &[&str] = &[
    "Cargo.toml",
    "**/Cargo.toml",
    "**/*.rs",
    "go.mod",
    "**/go.mod",
    "**/*.go",
];

// The same inputs after the lockfile cutover.
const LOCKFILE_BACKEND_INPUTS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "**/Cargo.toml",
    "**/Cargo.lock",
    "**/*.rs",
    "go.mod",
    "go.sum",
    "go.work",
    "go.work.sum",
    "**/go.mod",
    "**/go.sum",
    "**/go.work",
    "**/go.work.sum",
    "**/*.go",
    "vendor/modules.txt",
    "**/vendor/modules.txt",
];

fn legacy_model(backend: &str) -> RepositoryRenderModel {
    saved_model(backend, LEGACY_BACKEND_INPUTS)
}

fn saved_model(backend: &str, backend_inputs: &[&str]) -> RepositoryRenderModel {
    saved_model_with(backend, APP, backend_inputs)
}

/// A model saved by an earlier generator: it always declared a boundary
/// check, its contract checks read `backend_inputs`, and it had no document
/// check.
fn saved_model_with(backend: &str, apps: &str, backend_inputs: &[&str]) -> RepositoryRenderModel {
    let mut model = RepositoryRenderModel::from_answers(&scaffold_answers(&format!(
        "backend_language = {backend:?}\n{apps}"
    )))
    .unwrap();
    for component in &mut model.components {
        if component.id.as_str() == "web" {
            component.depends_on = vec![component_id("api").unwrap()];
            component
                .provenance
                .insert("depends_on".into(), FieldProvenance::Inferred);
        }
    }
    // Frontend checks used to pull in the contract checks.
    let drift = target_id("repo", "frontend-contract-drift").unwrap();
    let boundary = target_id("repo", "frontend-public-boundary").unwrap();
    if !model.actions.iter().any(|item| item.target == boundary) {
        let description = if backend == "go" {
            "Check public frontend manifests and artifacts for privileged markers."
        } else {
            "Check the repository-wide public/private dependency boundary."
        };
        let index = model
            .actions
            .iter()
            .position(|item| item.target == drift)
            .unwrap();
        model.actions.insert(
            index + 1,
            contract_check_action("frontend-public-boundary", description, Vec::new()).unwrap(),
        );
        let targets = &mut model.profiles[0].targets;
        let index = targets.iter().position(|target| target == &drift).unwrap();
        targets.insert(index + 1, boundary.clone());
        model.commands.insert(
            "repo_frontend_public_boundary_command".into(),
            "scripts/check-webapps.sh contracts-boundary-check frontend/web".into(),
        );
    }
    for action in &mut model.actions {
        match action.target.to_string().as_str() {
            "web:typecheck" => action.depends_on = vec![drift.clone(), boundary.clone()],
            "web:build" => action.depends_on = vec![boundary.clone()],
            _ => {}
        }
    }
    let document_check = target_id("api", "openapi").unwrap();
    model
        .actions
        .retain(|action| action.target != document_check);
    for profile in &mut model.profiles {
        profile.targets.retain(|target| *target != document_check);
    }
    model.commands.remove("api_openapi_command");
    for action in &mut model.actions {
        if action.target == drift {
            action.description =
                Some("Check repository-wide OpenAPI and generated-client drift.".into());
        }
    }
    for action in &mut model.actions {
        if action.target.component.as_str() == "repo"
            && matches!(
                action.target.action.as_str(),
                "frontend-contract-drift" | "frontend-public-boundary"
            )
        {
            action.inputs = FRONTEND_SHARED_INPUTS
                .iter()
                .chain(backend_inputs)
                .map(|input| (*input).to_owned())
                .collect();
            if action.target.action.as_str() == "frontend-public-boundary" {
                action
                    .inputs
                    .extend(["docs/public/**".into(), "public-docs/**".into()]);
            }
            action.inputs.sort();
            action.inputs.dedup();
        }
    }
    model
        .prepare_runner_epoch(jig_context::CURRENT_CONTRACT_VERSION)
        .unwrap();
    model
        .prepare_freshness_epoch(jig_context::CURRENT_CONTRACT_VERSION)
        .unwrap();
    model
}

/// What jig generates for a saved legacy scaffold, which always declared a
/// boundary check; only a Rust one keeps it.
fn generated_model(backend: &str, apps: &str) -> RepositoryRenderModel {
    let mut answers = scaffold_answers(&format!("backend_language = {backend:?}\n{apps}"));
    answers.enable_scaffolded_public_boundary();
    RepositoryRenderModel::from_answers(&answers).unwrap()
}

/// Loads `model` as `jig update` does, in a repository that has the
/// scaffold's committed-document tests.
fn load_saved(model: &RepositoryRenderModel, backend: &str) -> RenderAnswers {
    load_saved_with(model, backend, APP)
}

fn load_saved_with(model: &RepositoryRenderModel, backend: &str, apps: &str) -> RenderAnswers {
    let temp = TempDir::new().unwrap();
    write_scaffold_document_tests(temp.path(), backend);
    load_saved_at(temp.path(), model, backend, apps)
}

/// Loads `model`, saved with the frontend `apps`, as `jig update` does, in
/// the repository at `root`.
fn load_saved_at(
    root: &Path,
    model: &RepositoryRenderModel,
    backend: &str,
    apps: &str,
) -> RenderAnswers {
    let path = root.join(".jig.toml");
    fs::write(
        &path,
        format!(
            "repo_name = \"ExampleProject\"\nbackend_language = {backend:?}\nsqlx_enabled = false\nschema_dump_enabled = false\n{apps}\n{}\n{}",
            model.authored_toml().unwrap(),
            model.commands_toml().unwrap()
        ),
    )
    .unwrap();
    RenderAnswers::from_managed_answers_file(&path, root)
        .unwrap()
        .0
}

/// A Cargo workspace like a scaffold's, whose members `cargo test` tests.
const RUST_WORKSPACE: &str = "[workspace]\nmembers = [\"apps/*\", \"crates/*\"]\n";

/// The manifest of a workspace member.
const RUST_PACKAGE: &str = "[package]\nname = \"example-project\"\nversion = \"0.1.0\"\n";

/// The committed-document test that scaffolds with API contracts write.
fn write_scaffold_document_tests(root: &Path, backend: &str) {
    let files: &[(&str, &str)] = if backend == "go" {
        &[(
            "internal/httpapi/httpapi_test.go",
            "package httpapi\n\nimport \"testing\"\n\nfunc TestOpenAPIIsCurrent(t *testing.T) {}\n",
        )]
    } else {
        &[
            ("Cargo.toml", RUST_WORKSPACE),
            ("crates/example-project-http/Cargo.toml", RUST_PACKAGE),
            (
                "crates/example-project-http/src/lib.rs",
                "#[cfg(test)]\nmod tests {\n    #[test]\n    fn committed_public_openapi_is_current() {}\n}\n",
            ),
        ]
    };
    for (path, contents) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
}

/// A model generated with the scaffold's client-only contract script.
fn scaffolded_model(backend: &str, answers: &str) -> RepositoryRenderModel {
    let mut scaffold = scaffold_answers(&format!("backend_language = {backend:?}\n{answers}{APP}"));
    scaffold.enable_client_only_contract_drift();
    RepositoryRenderModel::from_answers(&scaffold).unwrap()
}

/// A current scaffold as `jig init` saves it.
fn saved_current_model(backend: &str) -> RepositoryRenderModel {
    let mut current = scaffolded_model(backend, "");
    current
        .prepare_runner_epoch(jig_context::CURRENT_CONTRACT_VERSION)
        .unwrap();
    current
        .prepare_freshness_epoch(jig_context::CURRENT_CONTRACT_VERSION)
        .unwrap();
    current
}

/// Renames the backend component as adoption names it after the repository
/// workspace, keeping its command keys.
fn rename_backend(model: &mut RepositoryRenderModel, id: &str) {
    let renamed = component_id(id).unwrap();
    let rename = |target: &mut TargetId| {
        if target.component.as_str() == "api" {
            target.component = renamed.clone();
        }
    };
    for component in &mut model.components {
        if component.id.as_str() == "api" {
            component.id = renamed.clone();
        }
    }
    for action in &mut model.actions {
        rename(&mut action.target);
        action.depends_on.iter_mut().for_each(rename);
    }
    for profile in &mut model.profiles {
        profile.targets.iter_mut().for_each(rename);
    }
}

#[test]
fn managed_update_upgrades_legacy_contract_models_and_keeps_them_generated() {
    for (backend, backend_inputs) in ["rust", "go"].into_iter().flat_map(|backend| {
        [
            (backend, LEGACY_BACKEND_INPUTS),
            (backend, LOCKFILE_BACKEND_INPUTS),
        ]
    }) {
        let saved = saved_model(backend, backend_inputs);
        let answers = load_saved(&saved, backend);
        assert!(answers.authored_repository().is_none());
        assert!(answers.scaffolded_frontend_contracts());
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();
        let current = generated_model(backend, APP);
        assert_eq!(
            upgraded.authored_toml().unwrap(),
            current.authored_toml().unwrap()
        );
        assert_eq!(upgraded.commands, current.commands);
        assert_eq!(
            upgraded
                .actions
                .iter()
                .any(|item| item.target.action.as_str() == "frontend-public-boundary"),
            backend == "rust",
            "{backend}"
        );
        let reloaded = load_saved(&upgraded, backend);
        assert!(reloaded.authored_repository().is_none());
        assert!(reloaded.scaffolded_frontend_contracts());
    }
}

#[test]
fn managed_update_retires_the_dependency_of_uncontracted_frontends_and_keeps_them_generated() {
    for backend in ["rust", "go"] {
        let current = RepositoryRenderModel::from_answers(&answers(&format!(
            "backend_language = {backend:?}\n{APP}"
        )))
        .unwrap();
        let mut saved = current.clone();
        let web = saved
            .components
            .iter_mut()
            .find(|item| item.id.as_str() == "web")
            .unwrap();
        web.depends_on = vec![component_id("api").unwrap()];
        web.provenance
            .insert("depends_on".into(), FieldProvenance::Inferred);
        saved
            .prepare_runner_epoch(jig_context::CURRENT_CONTRACT_VERSION)
            .unwrap();
        saved
            .prepare_freshness_epoch(jig_context::CURRENT_CONTRACT_VERSION)
            .unwrap();

        let answers = load_saved(&saved, backend);

        assert!(
            answers.authored_repository().is_none(),
            "{backend}: an unmodified saved model must stay generated"
        );
        assert!(!answers.scaffolded_frontend_contracts());
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert_eq!(
            upgraded.authored_toml().unwrap(),
            current.authored_toml().unwrap()
        );
    }
}

#[test]
fn managed_update_keeps_a_declared_contract_check_dependency() {
    // Go would otherwise retire its boundary check; the declared dependency
    // keeps it in both languages.
    for backend in ["rust", "go"] {
        let mut saved = legacy_model(backend);
        let typecheck = saved
            .actions
            .iter_mut()
            .find(|item| item.target.to_string() == "web:typecheck")
            .unwrap();
        typecheck
            .provenance
            .insert("depends_on".into(), FieldProvenance::Declared);
        let expected = typecheck.depends_on.clone();

        let answers = load_saved(&saved, backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_some());
        let find = |target: &str| {
            upgraded
                .actions
                .iter()
                .find(|item| item.target.to_string() == target)
                .unwrap_or_else(|| panic!("{backend}: missing {target}"))
        };
        assert_eq!(find("web:typecheck").depends_on, expected);
        assert!(find("web:build").depends_on.is_empty());
        find("repo:frontend-public-boundary");
    }
}

#[test]
fn managed_update_keeps_contract_check_dependencies_outside_the_generated_shapes() {
    let mut saved = legacy_model("rust");
    let drift = target_id("repo", "frontend-contract-drift").unwrap();
    let boundary = target_id("repo", "frontend-public-boundary").unwrap();
    // Added by hand without marking the provenance declared; no generator
    // ever wrote these shapes.
    let kept = [
        ("web:test", vec![boundary.clone()]),
        ("web:lint", vec![drift, boundary]),
    ];
    for (target, dependencies) in &kept {
        saved
            .actions
            .iter_mut()
            .find(|item| item.target.to_string() == *target)
            .unwrap()
            .depends_on
            .clone_from(dependencies);
    }

    let answers = load_saved(&saved, "rust");
    let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

    let find = |target: &str| {
        upgraded
            .actions
            .iter()
            .find(|item| item.target.to_string() == target)
            .unwrap()
    };
    for (target, dependencies) in &kept {
        assert_eq!(&find(target).depends_on, dependencies, "{target}");
    }
    assert!(find("web:typecheck").depends_on.is_empty());
    assert!(find("web:build").depends_on.is_empty());
}

#[test]
fn managed_update_refreshes_generated_inputs_inside_a_custom_model() {
    let mut saved = legacy_model("rust");
    saved
        .components
        .iter_mut()
        .find(|item| item.id.as_str() == "web")
        .unwrap()
        .description = Some("Custom frontend description".into());
    let answers = load_saved(&saved, "rust");
    assert!(answers.authored_repository().is_some());
    let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();
    let frontend = upgraded
        .components
        .iter()
        .find(|item| item.id.as_str() == "web")
        .unwrap();
    assert_eq!(
        frontend.description.as_deref(),
        Some("Custom frontend description")
    );
    assert!(frontend.depends_on.is_empty());
    let current = generated_model("rust", APP);
    let find = |model: &RepositoryRenderModel, target: &str| {
        model
            .actions
            .iter()
            .find(|item| item.target.to_string() == target)
            .unwrap()
            .clone()
    };
    for target in [
        "api:test",
        "api:test-locked",
        "repo:frontend-public-boundary",
    ] {
        assert_eq!(
            find(&upgraded, target).inputs,
            find(&current, target).inputs,
            "{target}"
        );
    }
    // Without a document check, the drift check keeps reading the backend.
    let mut drift = find(&current, "repo:frontend-contract-drift").inputs;
    drift.extend(
        BACKEND_CONTRACT_INPUTS
            .iter()
            .map(|input| (*input).to_owned()),
    );
    drift.sort();
    drift.dedup();
    assert_eq!(
        find(&upgraded, "repo:frontend-contract-drift").inputs,
        drift
    );
    let boundary = find(&upgraded, "repo:frontend-public-boundary");
    assert!(boundary.inputs.contains(&"Cargo.lock".into()));
    assert!(!boundary.inputs.contains(&"**/*.rs".into()));
}

#[test]
fn managed_update_retires_the_go_boundary_check_inside_a_custom_model() {
    let mut saved = legacy_model("go");
    saved
        .components
        .iter_mut()
        .find(|item| item.id.as_str() == "web")
        .unwrap()
        .description = Some("Custom frontend description".into());

    let answers = load_saved(&saved, "go");
    let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

    assert!(answers.authored_repository().is_some());
    let boundary = target_id("repo", "frontend-public-boundary").unwrap();
    assert!(!upgraded.actions.iter().any(|item| item.target == boundary));
    assert!(!upgraded.profiles[0].targets.contains(&boundary));
    assert!(
        !upgraded
            .commands
            .contains_key("repo_frontend_public_boundary_command")
    );
    assert!(upgraded.frontend_contracts_enabled());
}

#[test]
fn managed_update_keeps_a_customized_boundary_check() {
    let mut saved = legacy_model("rust");
    let boundary = saved
        .actions
        .iter_mut()
        .find(|item| item.target.to_string() == "repo:frontend-public-boundary")
        .unwrap();
    boundary.inputs.push("docs/privileged/**".into());
    let expected = boundary.clone();

    let answers = load_saved(&saved, "rust");
    let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

    assert!(answers.authored_repository().is_some());
    assert_eq!(
        upgraded
            .actions
            .iter()
            .find(|item| item.target == expected.target)
            .unwrap()
            .inputs,
        expected.inputs
    );
    assert!(upgraded.profiles[0].targets.contains(&expected.target));
    assert!(
        upgraded
            .commands
            .contains_key("repo_frontend_public_boundary_command")
    );
}

#[test]
fn managed_update_narrows_admin_boundary_inputs_and_keeps_models_generated() {
    let apps = format!("{APP}{ADMIN_APP}");
    let current = RepositoryRenderModel::from_answers(&scaffold_answers(&format!(
        "backend_language = \"rust\"\n{apps}"
    )))
    .unwrap();
    for backend_inputs in [LEGACY_BACKEND_INPUTS, LOCKFILE_BACKEND_INPUTS] {
        let saved = saved_model_with("rust", &apps, backend_inputs);

        let answers = load_saved_with(&saved, "rust", &apps);

        assert!(answers.authored_repository().is_none());
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert_eq!(
            upgraded.authored_toml().unwrap(),
            current.authored_toml().unwrap()
        );
    }
}

#[test]
fn managed_update_preserves_authored_contract_inputs_commands_and_dependencies() {
    for customization in ["inputs", "provenance", "command"] {
        let mut saved = legacy_model("rust");
        let action = saved
            .actions
            .iter_mut()
            .find(|item| item.target.action.as_str() == "frontend-contract-drift")
            .unwrap();
        match customization {
            "inputs" => action.inputs.push("custom-contracts/**".into()),
            "provenance" => {
                action
                    .provenance
                    .insert("inputs".into(), FieldProvenance::Declared);
            }
            "command" => {
                saved.commands.insert(
                    "repo_frontend_contract_drift_command".into(),
                    "scripts/custom-contracts.sh contracts-drift-check".into(),
                );
            }
            _ => unreachable!(),
        }
        let expected = action.clone();
        let frontend = saved
            .components
            .iter_mut()
            .find(|item| item.id.as_str() == "web")
            .unwrap();
        frontend
            .provenance
            .insert("depends_on".into(), FieldProvenance::Declared);
        let expected_frontend = frontend.clone();
        let answers = load_saved(&saved, "rust");
        assert!(answers.authored_repository().is_some());
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert_eq!(
            upgraded
                .actions
                .iter()
                .find(|item| item.target == expected.target)
                .unwrap(),
            &expected
        );
        assert_eq!(
            upgraded
                .components
                .iter()
                .find(|item| item.id.as_str() == "web")
                .unwrap(),
            &expected_frontend
        );
        assert_eq!(upgraded.commands, saved.commands);
    }
}

#[test]
fn contracts_split_backend_and_client_freshness_without_frontend_fanout() {
    use jig_context::RepoContext;
    use jig_repository::{PlanRunRequest, RepositoryCatalog, plan_run_with_cancellation};

    // A scaffold's client-only drift check ignores backend changes; an
    // upgraded repository's own contract script still exports from the
    // backend, so its drift check keeps running on them.
    for (backend, client_only) in [("rust", true), ("go", true), ("rust", false), ("go", false)] {
        let mut model = if client_only {
            let mut scaffold = scaffold_answers(&format!("backend_language = {backend:?}\n{APP}"));
            scaffold.enable_client_only_contract_drift();
            RepositoryRenderModel::from_answers(&scaffold).unwrap()
        } else {
            RepositoryRenderModel::from_answers(&load_saved(&legacy_model(backend), backend))
                .unwrap()
        };
        model
            .prepare_runner_epoch(jig_context::CURRENT_CONTRACT_VERSION)
            .unwrap();
        model
            .prepare_freshness_epoch(jig_context::CURRENT_CONTRACT_VERSION)
            .unwrap();
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        for dir in [
            ".agent",
            "frontend/web/src",
            "src",
            "internal",
            "vendor",
            "openapi",
            "packages/web-api-client",
        ] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join(".jig.toml"), format!(
            "repo_name = \"ExampleProject\"\ndefault_branch = \"main\"\n_src_path = \"embedded:jig-sh\"\n_commit = \"fixture\"\n{}\n{}",
            model.authored_toml().unwrap(), model.commands_toml().unwrap()
        )).unwrap();
        let mut manifest = serde_json::to_value(&model).unwrap();
        manifest["contract_version"] = serde_json::json!(jig_context::CURRENT_CONTRACT_VERSION);
        manifest["tool_namespace"] = serde_json::json!("jig");
        fs::write(
            root.join(".agent/jig-contract.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Example",
                "-c",
                "user.email=example@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "fixture",
            ],
        ] {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let context = RepoContext::load_from_root(root.to_path_buf()).unwrap();
        let catalog = RepositoryCatalog::from_context(&context).unwrap();
        let backend_paths: &[&str] = if backend == "rust" {
            &["Cargo.lock", "src/lib.rs"]
        } else {
            &[
                "go.sum",
                "go.work",
                "go.work.sum",
                "vendor/modules.txt",
                "internal/service.go",
            ]
        };
        let selected_after = |path: &str| {
            fs::write(root.join(path), "changed\n").unwrap();
            let plan = plan_run_with_cancellation(
                &context,
                &catalog,
                PlanRunRequest {
                    selectors: [
                        "api:test",
                        "api:openapi",
                        "repo:frontend-contract-drift",
                        "web:lint",
                        "web:typecheck",
                        "web:build",
                        "web:test",
                    ]
                    .map(str::to_owned)
                    .into(),
                    affected_base: Some("HEAD".into()),
                    ..PlanRunRequest::default()
                },
                &|| false,
            )
            .unwrap();
            fs::remove_file(root.join(path)).unwrap();
            plan.targets
                .iter()
                .map(|item| item.target.to_string())
                .collect::<Vec<_>>()
        };
        // The document check owns document freshness and the drift check
        // owns client freshness; neither change fans out to the frontend
        // checks, and a document change does not rerun every backend test.
        let expectations = backend_paths
            .iter()
            .map(|path| (*path, true, true, !client_only, false))
            .chain([
                ("openapi/public.json", false, true, true, true),
                ("packages/web-api-client/index.ts", false, false, true, true),
                ("frontend/web/src/main.ts", false, false, false, true),
            ]);
        for (path, backend_tests, documents, drift, frontend) in expectations {
            let selected = selected_after(path);
            assert_eq!(
                selected.contains(&"api:test".into()),
                backend_tests,
                "{backend} {path}: {selected:?}"
            );
            assert_eq!(
                selected.contains(&"api:openapi".into()),
                documents,
                "{backend} {path}: {selected:?}"
            );
            assert_eq!(
                selected.contains(&"repo:frontend-contract-drift".into()),
                drift,
                "{backend} {path}: {selected:?}"
            );
            for check in ["lint", "typecheck", "build", "test"] {
                assert_eq!(
                    selected.contains(&format!("web:{check}")),
                    frontend,
                    "{backend} {path}: {selected:?}"
                );
            }
        }
    }
}
