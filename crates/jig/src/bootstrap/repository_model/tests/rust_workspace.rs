use super::*;

#[test]
fn neutral_rust_workspace_uses_existing_records_actions_and_aliases() {
    let answers = rust_workspace_answers();
    let model = RepositoryRenderModel::from_answers(&answers).unwrap();

    assert_eq!(
        model
            .components
            .iter()
            .map(|component| component.id.as_str())
            .collect::<Vec<_>>(),
        ["repo", "workspace"]
    );
    let workspace = model
        .components
        .iter()
        .find(|component| component.id.as_str() == "workspace")
        .unwrap();
    assert_eq!(workspace.root, ".");
    assert_eq!(
        workspace.description.as_deref(),
        Some("Repository Rust workspace.")
    );
    assert_eq!(workspace.tags, ["workspace"]);
    assert_eq!(workspace.adapters, ["rust"]);
    assert!(model.rust_workspace_guidance_enabled());
    assert!(model.rust_ci_input_paths().contains(&"**".into()));
    assert_eq!(model.rust_component_input_paths(), ["**"]);

    for (target, alias) in [
        ("workspace:fmt", jig_contract::tool::FMT_CHECK),
        ("workspace:clippy", jig_contract::tool::CLIPPY),
        ("workspace:test", jig_contract::tool::TEST),
        ("workspace:test-locked", jig_contract::tool::TEST_LOCKED),
        ("repo:file-budget", jig_contract::tool::FILE_BUDGET),
    ] {
        let action = model
            .actions
            .iter()
            .find(|action| action.target.to_string() == target)
            .unwrap_or_else(|| panic!("missing {target}"));
        assert!(
            action
                .legacy_aliases
                .iter()
                .any(|candidate| candidate == alias),
            "{target} is missing compatibility alias {alias}"
        );
    }
    assert!(model.components.iter().all(|component| {
        component.id.as_str() != "api"
            && !component.tags.iter().any(|tag| tag == "backend")
            && !component
                .description
                .as_deref()
                .is_some_and(|description| description.contains("backend"))
    }));
    assert!(
        model
            .actions
            .iter()
            .all(|action| { !matches!(action.target.component.as_str(), "api" | "web" | "admin") })
    );
    assert_eq!(
        answers.rust_fmt_ci_target().as_deref(),
        Some("workspace:fmt")
    );
    assert_eq!(
        answers.rust_clippy_ci_target().as_deref(),
        Some("workspace:clippy")
    );
    assert_eq!(
        answers.rust_test_locked_ci_target().as_deref(),
        Some("workspace:test-locked")
    );
    assert!(
        serde_json::to_value(&answers)
            .unwrap()
            .get("repository_projection_hint")
            .is_none()
    );
    let authored = model.authored_toml().unwrap();
    assert!(!authored.contains("rust-library"));
    assert!(!authored.contains("rust-cli"));
    assert!(!authored.contains("projection"));
}

#[test]
fn neutral_workspace_guidance_is_recovered_from_authored_semantics() {
    let initial = RepositoryRenderModel::from_answers(&rust_workspace_answers()).unwrap();
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
        &path,
        format!(
            "repo_name = \"ExampleProject\"\nsqlx_enabled = false\nschema_dump_enabled = false\n{}\n{}",
            initial.authored_toml().unwrap(),
            initial.commands_toml().unwrap()
        ),
    )
    .unwrap();

    let reloaded = RenderAnswers::from_answers_file(&path).unwrap();
    assert_eq!(
        reloaded.repository_projection_hint(),
        RepositoryProjectionHint::Backend
    );
    let rerendered = RepositoryRenderModel::from_answers(&reloaded).unwrap();

    assert!(rerendered.rust_workspace_guidance_enabled());
    assert_eq!(
        serde_json::to_value(&rerendered).unwrap(),
        serde_json::to_value(&initial).unwrap()
    );
}

#[test]
fn existing_backend_projection_does_not_enable_workspace_guidance() {
    let model = RepositoryRenderModel::from_answers(&answers("")).unwrap();
    assert!(!model.rust_workspace_guidance_enabled());
    assert!(
        model
            .components
            .iter()
            .any(|component| component.id.as_str() == "api"
                && component.description.as_deref() == Some("Primary application backend."))
    );
}
