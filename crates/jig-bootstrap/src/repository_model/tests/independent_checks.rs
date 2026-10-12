use super::*;

#[test]
fn generated_verification_checks_are_independent_profile_requirements() {
    for (answers, backend) in [
        (answers(""), "api"),
        (answers("backend_language = \"go\"\n"), "api"),
        (rust_workspace_answers(), "workspace"),
        (
            scaffold_answers(
                r#"
[[frontend_apps]]
name = "web"
dir = "frontend/web"
coverage_threshold = 80
kind = "vite"
role = "spa"
"#,
            ),
            "api",
        ),
    ] {
        let model = RepositoryRenderModel::from_answers(&answers).unwrap();
        let profile = model
            .profiles
            .iter()
            .find(|profile| profile.id.as_str() == "verify")
            .unwrap();
        let mut expected = vec![
            target_id(backend, "test").unwrap(),
            target_id(backend, "fmt").unwrap(),
            target_id(REPO_COMPONENT, "contract").unwrap(),
            target_id(REPO_COMPONENT, "file-budget").unwrap(),
        ];
        if !answers.backend_language().is_go() {
            expected.push(target_id(backend, "clippy").unwrap());
        }
        if answers.frontend_harness_enabled() {
            expected.push(target_id("web", "test").unwrap());
        }
        for target in expected {
            assert!(profile.targets.contains(&target), "missing {target}");
            let action = model
                .actions
                .iter()
                .find(|action| action.target == target)
                .unwrap();
            assert!(
                action.depends_on.is_empty(),
                "{target} must not rerun when an independent policy receipt changes"
            );
        }
    }
}

#[test]
fn authored_execution_prerequisites_survive_answer_reload() {
    let mut model = RepositoryRenderModel::from_answers(&answers("")).unwrap();
    let build = target_id("api", "build").unwrap();
    let test = target_id("api", "test").unwrap();
    let mut prerequisite = ActionSpec::new(
        build.clone(),
        ActionIntent::Check,
        ActionRunner::command("api_build_command"),
    );
    prerequisite.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
    model.actions.push(prerequisite);
    model
        .commands
        .insert("api_build_command".into(), "cargo build --workspace".into());
    let test_action = model
        .actions
        .iter_mut()
        .find(|action| action.target == test)
        .unwrap();
    test_action.depends_on = vec![build.clone()];
    test_action
        .provenance
        .insert("depends_on".into(), FieldProvenance::Declared);
    let reloaded = answers(&format!(
        "{}\n{}",
        model.authored_toml().unwrap(),
        model.commands_toml().unwrap()
    ));
    let rerendered = RepositoryRenderModel::from_answers(&reloaded).unwrap();
    let test_action = rerendered
        .actions
        .iter()
        .find(|action| action.target == test)
        .unwrap();
    assert_eq!(test_action.depends_on, [build]);
    assert_eq!(
        test_action.provenance.get("depends_on"),
        Some(&FieldProvenance::Declared)
    );
}
