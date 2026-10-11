use super::*;

const FRONTEND_APPS: &str = r#"
[[frontend_apps]]
name = "web"
dir = "frontend/web"
coverage_threshold = 80
kind = "vite"
role = "spa"

[[frontend_apps]]
name = "admin"
dir = "frontend/admin"
coverage_threshold = 85
kind = "vite"
role = "admin"
"#;

fn component<'a>(model: &'a RepositoryRenderModel, id: &str) -> &'a ComponentSpec {
    model
        .components
        .iter()
        .find(|component| component.id.as_str() == id)
        .unwrap()
}

fn action<'a>(model: &'a RepositoryRenderModel, target: &str) -> &'a ActionSpec {
    model
        .actions
        .iter()
        .find(|action| action.target.to_string() == target)
        .unwrap()
}

#[test]
fn scaffolded_frontends_reach_backend_changes_only_through_contracts() {
    for (backend, backend_inputs) in [
        ("", ["**/*.rs", "Cargo.toml", "Cargo.lock", "**/Cargo.lock"]),
        (
            "backend_language = \"go\"\n",
            ["**/*.go", "go.mod", "go.sum", "go.work"],
        ),
    ] {
        let model = RepositoryRenderModel::from_answers(&scaffold_answers(&format!(
            "{backend}{FRONTEND_APPS}"
        )))
        .unwrap();

        assert!(component(&model, "api").propagate_affected_to_dependents);
        for frontend in ["web", "admin"] {
            let spec = component(&model, frontend);
            assert!(
                spec.depends_on.is_empty(),
                "{frontend} must not select every check on backend changes"
            );
            assert!(!spec.provenance.contains_key("depends_on"));
            for check in ["lint", "typecheck", "build", "test"] {
                let inputs = &action(&model, &format!("{frontend}:{check}")).inputs;
                for contract in ["openapi/**", "packages/*-api-client/**"] {
                    assert!(
                        inputs.iter().any(|input| input == contract),
                        "{frontend}:{check} must read {contract}"
                    );
                }
            }
        }
        // The document check proves the documents match the backend; the
        // drift check proves the clients match the documents.
        let drift = action(&model, "repo:frontend-contract-drift");
        let documents = action(&model, "api:openapi");
        for contract in ["openapi/**", "packages/*-api-client/**"] {
            assert!(drift.inputs.iter().any(|input| input == contract));
        }
        assert!(documents.inputs.iter().any(|input| input == "openapi/**"));
        for input in backend_inputs {
            assert!(
                documents.inputs.iter().any(|candidate| candidate == input),
                "the document check must read {input}"
            );
            assert!(
                !drift.inputs.iter().any(|candidate| candidate == input),
                "contract drift must not read {input}"
            );
        }
        let profile = model
            .profiles
            .iter()
            .find(|profile| profile.id.as_str() == DEFAULT_PROFILE)
            .unwrap();
        assert!(profile.targets.contains(&drift.target));
        assert!(profile.targets.contains(&documents.target));
    }
}

fn with_saved_backend_dependency(
    mut model: RepositoryRenderModel,
    source: FieldProvenance,
) -> RepositoryRenderModel {
    for component in &mut model.components {
        if component.tags.iter().any(|tag| tag == "frontend") {
            component.depends_on = vec![component_id(BACKEND_COMPONENT).unwrap()];
            component.provenance.insert("depends_on".into(), source);
        }
    }
    model
}

fn reload_saved(model: &RepositoryRenderModel) -> RepositoryRenderModel {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
        &path,
        format!(
            "repo_name = \"ExampleProject\"\nsqlx_enabled = false\nschema_dump_enabled = false\n{FRONTEND_APPS}\n{}\n{}",
            model.authored_toml().unwrap(),
            model.commands_toml().unwrap()
        ),
    )
    .unwrap();
    RepositoryRenderModel::from_answers(&RenderAnswers::from_answers_file(&path).unwrap()).unwrap()
}

#[test]
fn saved_generated_backend_dependency_is_retired_with_or_without_contract_checks() {
    for answers in [scaffold_answers(FRONTEND_APPS), answers(FRONTEND_APPS)] {
        let current = RepositoryRenderModel::from_answers(&answers).unwrap();
        let saved = with_saved_backend_dependency(current.clone(), FieldProvenance::Inferred);

        let upgraded = reload_saved(&saved);

        assert_eq!(upgraded.components, current.components);
        assert_eq!(upgraded.actions, current.actions);
        assert_eq!(
            upgraded.authored_toml().unwrap(),
            current.authored_toml().unwrap()
        );
    }
}

#[test]
fn saved_backend_dependency_is_kept_when_authored() {
    for answers in [scaffold_answers(FRONTEND_APPS), answers(FRONTEND_APPS)] {
        let saved = with_saved_backend_dependency(
            RepositoryRenderModel::from_answers(&answers).unwrap(),
            FieldProvenance::Declared,
        );

        let reloaded = reload_saved(&saved);

        assert_eq!(reloaded.components, saved.components);
        for frontend in ["web", "admin"] {
            assert_eq!(
                component(&reloaded, frontend).depends_on,
                [component_id(BACKEND_COMPONENT).unwrap()]
            );
        }
    }
}

#[test]
fn frontends_without_contracts_do_not_depend_on_the_backend() {
    let model = RepositoryRenderModel::from_answers(&answers(FRONTEND_APPS)).unwrap();

    assert!(component(&model, "api").propagate_affected_to_dependents);
    for frontend in ["web", "admin"] {
        let spec = component(&model, frontend);
        assert!(spec.depends_on.is_empty(), "{frontend}");
        assert!(!spec.provenance.contains_key("depends_on"));
    }
}

#[test]
fn retirement_follows_an_adopted_backend_id_and_keeps_other_dependencies() {
    let mut components = RepositoryRenderModel::from_answers(&answers(FRONTEND_APPS))
        .unwrap()
        .components;
    let workspace = component_id("workspace").unwrap();
    let shared = component_id("shared").unwrap();
    for component in &mut components {
        let dependency = match component.id.as_str() {
            // Adoption renames the generated backend after its candidate.
            "api" => {
                component.id = workspace.clone();
                component.tags = vec!["workspace".into()];
                continue;
            }
            "web" => workspace.clone(),
            "admin" => shared.clone(),
            _ => continue,
        };
        component.depends_on = vec![dependency];
        component
            .provenance
            .insert("depends_on".into(), FieldProvenance::Inferred);
    }
    let mut shared_package = ComponentSpec::new(shared.clone(), "packages/shared");
    shared_package.tags = vec!["frontend".into(), "shared".into()];
    shared_package.adapters = vec!["typescript".into()];
    components.push(shared_package);

    retire_saved_frontend_dependencies(&mut components, &mut []);

    let find = |id: &str| {
        components
            .iter()
            .find(|component| component.id.as_str() == id)
            .unwrap()
    };
    assert!(find("web").depends_on.is_empty());
    assert!(!find("web").provenance.contains_key("depends_on"));
    assert_eq!(find("admin").depends_on, [shared]);
}
