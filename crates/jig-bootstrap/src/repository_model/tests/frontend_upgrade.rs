use super::*;

const APP: &str = r#"
[[frontend_apps]]
name = "web"
dir = "frontend/web"
coverage_threshold = 80
kind = "vite"
role = "spa"
"#;

fn legacy_model(backend: &str) -> RepositoryRenderModel {
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
    // The actual generated input shape before the contract-only cutover.
    for action in &mut model.actions {
        if action.target.component.as_str() == "repo"
            && matches!(
                action.target.action.as_str(),
                "frontend-contract-drift" | "frontend-public-boundary"
            )
        {
            action.inputs = FRONTEND_SHARED_INPUTS
                .iter()
                .copied()
                .chain([
                    "Cargo.toml",
                    "**/Cargo.toml",
                    "**/*.rs",
                    "go.mod",
                    "**/go.mod",
                    "**/*.go",
                ])
                .map(str::to_owned)
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
    for backend in ["rust", "go"] {
        let saved = legacy_model(backend);
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
    for action in upgraded.actions.iter().filter(|action| {
        matches!(
            action.target.action.as_str(),
            "frontend-contract-drift" | "frontend-public-boundary"
        )
    }) {
        for input in [
            "Cargo.lock",
            "go.sum",
            "go.work",
            "go.work.sum",
            "vendor/modules.txt",
        ] {
            assert!(
                action.inputs.contains(&input.into()),
                "{} must read {input}",
                action.target
            );
        }
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
fn upgraded_contracts_select_lockfile_and_workspace_changes_without_frontend_fanout() {
    use jig_context::RepoContext;
    use jig_repository::{PlanRunRequest, RepositoryCatalog, plan_run_with_cancellation};

    for backend in ["rust", "go"] {
        let answers = load_saved(&legacy_model(backend), backend);
        let mut model = RepositoryRenderModel::from_answers(&answers).unwrap();
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
            "frontend/web",
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
        for path in backend_paths
            .iter()
            .copied()
            .chain(["openapi/public.json", "packages/web-api-client/index.ts"])
        {
            fs::write(root.join(path), "changed\n").unwrap();
            let plan = plan_run_with_cancellation(
                &context,
                &catalog,
                PlanRunRequest {
                    selectors: [
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
            let selected = plan
                .targets
                .iter()
                .map(|item| item.target.to_string())
                .collect::<Vec<_>>();
            assert!(
                selected.contains(&"repo:frontend-contract-drift".into()),
                "{backend} {path}: {selected:?}"
            );
            let frontend_expected = !backend_paths.contains(&path);
            for check in ["lint", "typecheck", "build", "test"] {
                assert_eq!(
                    selected.contains(&format!("web:{check}")),
                    frontend_expected,
                    "{backend} {path}: {selected:?}"
                );
            }
            fs::remove_file(root.join(path)).unwrap();
        }
    }
}
