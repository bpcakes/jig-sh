use super::*;

pub(super) fn render_context(
    template: &PreparedTemplateSource,
    answers: &RenderAnswers,
    contract_version: Option<u32>,
) -> Result<JsonValue> {
    let contract_version = contract_version.unwrap_or(crate::context::CURRENT_CONTRACT_VERSION);
    let mut context = serde_json::to_value(answers)?
        .as_object()
        .cloned()
        .unwrap_or_default();
    context.insert(
        "frontend_harness_enabled".into(),
        JsonValue::Bool(answers.frontend_harness_enabled()),
    );
    context.insert(
        "frontend_gate_apps".into(),
        JsonValue::Array(
            answers
                .frontend_apps()
                .iter()
                .map(|app| {
                    let paths_ignore = answers
                        .frontend_apps()
                        .iter()
                        .filter(|other| other.name != app.name && other.dir != ".")
                        .filter(|other| {
                            !repo_dirs_intersect(&app.dir, &other.dir)
                                && !answers
                                    .frontend_workspace_roots()
                                    .iter()
                                    .any(|root| repo_dirs_intersect(root, &other.dir))
                        })
                        .map(|other| format!("{}/**", other.dir))
                        .collect::<Vec<_>>();
                    json!({
                        "name": app.name,
                        "dir": app.dir,
                        "coverage_threshold": app.coverage_threshold,
                        "key": crate::bootstrap::answers::frontend_gate_key(&app.name),
                        "role": app.role,
                        "paths_ignore": paths_ignore,
                    })
                })
                .collect(),
        ),
    );
    context.insert(
        "rust_gate_command_authority_paths".into(),
        json!(RUST_GATE_COMMAND_AUTHORITY_PATHS),
    );
    context.insert(
        "rust_crate_root_shell_args".into(),
        JsonValue::Array(
            answers
                .rust_crate_roots()
                .iter()
                .map(|root| JsonValue::String(crate::shell::quote(root)))
                .collect(),
        ),
    );
    let mut frontend_gate_shared_paths = FRONTEND_GATE_SHARED_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect::<Vec<_>>();
    for path in answers.frontend_workspace_roots().iter().map(|root| {
        if root == "." {
            "**".into()
        } else {
            format!("{root}/**")
        }
    }) {
        if !frontend_gate_shared_paths.contains(&path) {
            frontend_gate_shared_paths.push(path);
        }
    }
    context.insert(
        "frontend_gate_shared_paths".into(),
        json!(frontend_gate_shared_paths),
    );
    context.insert(
        "go_backend_enabled".into(),
        JsonValue::Bool(answers.go_backend_enabled()),
    );
    context.insert(
        "rust_backend_enabled".into(),
        JsonValue::Bool(answers.rust_backend_enabled()),
    );
    context.insert(
        "file_budget_ci_enabled".into(),
        JsonValue::Bool(answers.file_budget_ci_enabled()),
    );
    context.insert(
        "go_postgres_enabled".into(),
        JsonValue::Bool(answers.go_postgres_enabled()),
    );
    context.insert(
        "go_ci_workflow_enabled".into(),
        JsonValue::Bool(answers.go_ci_workflow_enabled()),
    );
    context.insert(
        "rust_ci_workflow_enabled".into(),
        JsonValue::Bool(answers.rust_ci_workflow_enabled()),
    );
    context.insert(
        "go_sqlc_ci_enabled".into(),
        JsonValue::Bool(answers.go_sqlc_ci_enabled()),
    );
    for (name, target) in [
        ("go_fmt_ci_target", answers.go_fmt_ci_target()),
        ("go_lint_ci_target", answers.go_lint_ci_target()),
        (
            "go_test_locked_ci_target",
            answers.go_test_locked_ci_target(),
        ),
        ("go_sqlc_ci_target", answers.go_sqlc_ci_target()),
        ("rust_fmt_ci_target", answers.rust_fmt_ci_target()),
        ("rust_clippy_ci_target", answers.rust_clippy_ci_target()),
        (
            "rust_test_locked_ci_target",
            answers.rust_test_locked_ci_target(),
        ),
    ] {
        context.insert(name.into(), target.unwrap_or_default().into());
    }
    context.insert(
        "go_postgres_integration_ci_enabled".into(),
        JsonValue::Bool(answers.go_postgres_integration_ci_enabled()),
    );
    let repository = (contract_version >= 6
        || answers.go_ci_workflow_enabled()
        || answers.rust_ci_workflow_enabled())
    .then(|| RepositoryRenderModel::from_answers(answers))
    .transpose()?;
    context.insert(
        "go_ci_input_paths".into(),
        serde_json::to_value(
            repository
                .as_ref()
                .map(RepositoryRenderModel::go_ci_input_paths)
                .unwrap_or_default(),
        )?,
    );
    context.insert(
        "rust_ci_input_paths".into(),
        serde_json::to_value(
            repository
                .as_ref()
                .map(RepositoryRenderModel::rust_ci_input_paths)
                .unwrap_or_default(),
        )?,
    );
    context.insert(
        "rust_component_input_paths".into(),
        serde_json::to_value(
            repository
                .as_ref()
                .map(RepositoryRenderModel::rust_component_input_paths)
                .unwrap_or_default(),
        )?,
    );
    context.insert(
        "rust_workspace_guidance_enabled".into(),
        JsonValue::Bool(
            repository
                .as_ref()
                .is_some_and(RepositoryRenderModel::rust_workspace_guidance_enabled),
        ),
    );
    if contract_version >= 6 {
        let mut repository = repository.expect("contract v6 always resolves a repository model");
        if contract_version >= crate::repository::ACTION_EXECUTION_CONTRACT_VERSION {
            for action in &mut repository.actions {
                if matches!(&action.runner, jig_contract::ActionRunner::Native { operation, .. } if operation == jig_contract::tool::MIGRATION_ADD)
                    && action.arguments.is_empty()
                {
                    action.arguments.insert(
                        "name".into(),
                        jig_contract::ActionArgumentSpec::migration_name(),
                    );
                }
                crate::repository::arguments::normalize_declarations(contract_version, action)?;
            }
        }
        repository.prepare_runner_epoch(contract_version)?;
        let repository_toml = repository.authored_toml()?;
        let repository_commands_toml = repository.commands_toml()?;
        let file_budget_policy_toml = repository.file_budget_policy_toml()?.unwrap_or_default();
        context.insert(
            "frontend_contracts_enabled".into(),
            JsonValue::Bool(repository.frontend_contracts_enabled()),
        );
        context.insert("repository".into(), serde_json::to_value(repository)?);
        context.insert("repository_toml".into(), repository_toml.into());
        context.insert(
            "repository_commands_toml".into(),
            repository_commands_toml.into(),
        );
        context.insert(
            "file_budget_policy_toml".into(),
            file_budget_policy_toml.into(),
        );
    }
    context.insert(
        "_jig".into(),
        json!({
            "commit": template.vcs_ref().unwrap_or_default(),
            "src_path": if answers.template_source_url().is_empty() {
                template.source().to_string()
            } else {
                answers.template_source_url().to_string()
            },
            "template_mode": template.template_mode_answer().unwrap_or(""),
            "template_local_path": template.template_local_path_answer().unwrap_or(""),
            "contract_version": contract_version,
        }),
    );
    Ok(JsonValue::Object(context))
}
