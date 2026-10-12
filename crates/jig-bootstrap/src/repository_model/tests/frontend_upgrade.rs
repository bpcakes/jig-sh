use super::*;

mod backend_freshness;
mod document_commands;

const APP: &str = r#"
[[frontend_apps]]
name = "web"
dir = "frontend/web"
coverage_threshold = 80
kind = "vite"
role = "spa"
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

fn legacy_model(backend: &str) -> RepositoryRenderModel {
    saved_model(backend, LEGACY_BACKEND_INPUTS)
}

/// A model saved by an earlier generator whose contract checks read
/// `backend_inputs` and which had no document check.
fn saved_model(backend: &str, backend_inputs: &[&str]) -> RepositoryRenderModel {
    let mut model = RepositoryRenderModel::from_answers(&scaffold_answers(&format!(
        "backend_language = {backend:?}\n{APP}"
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

fn load_saved(model: &RepositoryRenderModel, backend: &str) -> RenderAnswers {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join(".jig.toml");
    fs::write(
        &path,
        format!(
            "repo_name = \"ExampleProject\"\nbackend_language = {backend:?}\nsqlx_enabled = false\nschema_dump_enabled = false\n{APP}\n{}\n{}",
            model.authored_toml().unwrap(),
            model.commands_toml().unwrap()
        ),
    )
    .unwrap();
    RenderAnswers::from_managed_answers_file(&path, temp.path())
        .unwrap()
        .0
}

#[test]
fn managed_update_upgrades_legacy_contract_models_and_keeps_them_generated() {
    for (backend, backend_inputs) in ["rust", "go"].into_iter().flat_map(|backend| {
        [
            (backend, LEGACY_BACKEND_INPUTS),
            (backend, BACKEND_CONTRACT_INPUTS),
        ]
    }) {
        let saved = saved_model(backend, backend_inputs);
        let answers = load_saved(&saved, backend);
        assert!(answers.authored_repository().is_none());
        assert!(answers.scaffolded_frontend_contracts());
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();
        let current = RepositoryRenderModel::from_answers(&scaffold_answers(&format!(
            "backend_language = {backend:?}\n{APP}"
        )))
        .unwrap();
        assert_eq!(
            upgraded.authored_toml().unwrap(),
            current.authored_toml().unwrap()
        );
        assert_eq!(upgraded.commands, current.commands);
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
    let mut saved = legacy_model("rust");
    let typecheck = saved
        .actions
        .iter_mut()
        .find(|item| item.target.to_string() == "web:typecheck")
        .unwrap();
    typecheck
        .provenance
        .insert("depends_on".into(), FieldProvenance::Declared);
    let expected = typecheck.depends_on.clone();

    let answers = load_saved(&saved, "rust");
    let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

    assert!(answers.authored_repository().is_some());
    let find = |target: &str| {
        upgraded
            .actions
            .iter()
            .find(|item| item.target.to_string() == target)
            .unwrap()
    };
    assert_eq!(find("web:typecheck").depends_on, expected);
    assert!(find("web:build").depends_on.is_empty());
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
    let current = RepositoryRenderModel::from_answers(&scaffold_answers(&format!(
        "backend_language = \"rust\"\n{APP}"
    )))
    .unwrap();
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
    for input in ["Cargo.lock", "go.sum", "go.work", "vendor/modules.txt"] {
        assert!(boundary.inputs.contains(&input.into()), "{input}");
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
