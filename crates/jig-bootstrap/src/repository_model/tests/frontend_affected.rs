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
            ["**/*.go", "go.mod", "go.sum", "**/go.sum"],
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
        let drift = action(&model, "repo:frontend-contract-drift");
        for input in backend_inputs {
            assert!(
                drift.inputs.iter().any(|candidate| candidate == input),
                "contract drift must read {input}"
            );
        }
        assert!(
            model
                .profiles
                .iter()
                .find(|profile| profile.id.as_str() == DEFAULT_PROFILE)
                .unwrap()
                .targets
                .contains(&drift.target)
        );
    }
}

#[test]
fn frontends_without_contracts_keep_the_backend_dependency() {
    let model = RepositoryRenderModel::from_answers(&answers(FRONTEND_APPS)).unwrap();

    for frontend in ["web", "admin"] {
        let spec = component(&model, frontend);
        assert_eq!(spec.depends_on, [component_id(BACKEND_COMPONENT).unwrap()]);
        assert_eq!(
            spec.provenance.get("depends_on"),
            Some(&FieldProvenance::Inferred)
        );
    }
}
