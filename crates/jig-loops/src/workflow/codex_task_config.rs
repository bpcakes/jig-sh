use super::*;

pub(super) fn config_codex_task(
    workflow: &LoopWorkflowConfig,
) -> Result<Option<CodexTaskSettings>> {
    if workflow.kind != CODEX_TASK_KIND {
        return Ok(None);
    }
    let prompt_file = workflow.prompt_file.clone().ok_or_else(|| {
        anyhow::anyhow!(
            "Loop workflow '{}' is missing required codex_task prompt_file",
            workflow.id
        )
    })?;
    let checkout = match workflow.checkout.as_deref().unwrap_or("worktree") {
        "repo" => CodexTaskCheckout::Repo,
        "worktree" => CodexTaskCheckout::Worktree,
        checkout => bail!(
            "Loop workflow '{}' has unsupported codex_task checkout '{checkout}'",
            workflow.id
        ),
    };
    Ok(Some(CodexTaskSettings {
        prompt_file,
        model: workflow.model.clone(),
        sandbox: workflow
            .sandbox
            .clone()
            .unwrap_or_else(|| "read-only".into()),
        checkout,
        prepare_command: workflow.prepare_command.clone(),
    }))
}
