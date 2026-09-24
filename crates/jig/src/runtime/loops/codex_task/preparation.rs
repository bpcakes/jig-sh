use super::*;

pub(super) fn failed_preparation_tick(
    settings: &CodexTaskSettings,
    item_key: &str,
    codex_home: Option<&Path>,
    checkout: checkout::CheckoutCompletion,
    failure: PreparationFailure,
) -> WorkflowTick {
    let retained_worktree = checkout.report.retained_worktree();
    let needs_attention = retained_worktree.is_some();
    let error = if let Some(checkout_error) = checkout.error {
        format!("{}; {checkout_error}", failure.error)
    } else {
        failure.error.clone()
    };
    let action = json!({
        "kind": "codex_task_worker",
        "status": if needs_attention { "needs_attention" } else { "failed" },
        "item_key": item_key,
        "worker_started": false,
        "worker_receipt_id": Value::Null,
        "checkout": checkout.report.value(),
        "codex_home_resolved": codex_home.map(|home| home.display().to_string()),
        "preparation": failure.evidence,
        "output": Value::Null,
        "error": error,
    });
    WorkflowTick::with_completion(
        json!({
            "kind": "codex_task",
            "prompt_file": settings.prompt_file.display().to_string(),
            "sandbox": settings.sandbox,
            "checkout": settings.checkout.as_str(),
        }),
        vec![action],
        WorkflowCompletion {
            outcome: if needs_attention {
                WorkflowOutcome::NeedsAttention
            } else {
                WorkflowOutcome::Failed
            },
            execution: WorkflowExecution::Unexecuted(failure.reason),
            repository_revision: checkout.report.repository_revision_state(),
            worker_receipt_id: None,
            worktree: retained_worktree,
            error: Some(error),
        },
    )
}

pub(super) struct PreparationFailure {
    pub(super) reason: UnexecutedReason,
    pub(super) error: String,
    pub(super) evidence: Value,
}

pub(super) fn run_preparation(
    ctx: &RepoContext,
    settings: &CodexTaskSettings,
    checkout: &Path,
    codex_home: Option<&Path>,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<Value, PreparationFailure> {
    if settings.checkout != CodexTaskCheckout::Worktree {
        return Err(failure(
            settings,
            "failed",
            false,
            UnexecutedReason::PreExecutionError,
            "Task preparation requires an isolated worktree".into(),
            &[],
            &[],
        ));
    }
    let Some(argv) = settings
        .prepare_command
        .as_deref()
        .filter(|argv| !argv.is_empty())
    else {
        return Err(failure(
            settings,
            "failed",
            false,
            UnexecutedReason::PreExecutionError,
            "Task preparation command is missing".into(),
            &[],
            &[],
        ));
    };
    let profile = match settings.sandbox.as_str() {
        "read-only" => ":read-only",
        "workspace-write" => ":workspace",
        _ => {
            return Err(failure(
                settings,
                "failed",
                false,
                UnexecutedReason::PreExecutionError,
                "Task preparation has an unsupported sandbox policy".into(),
                &[],
                &[],
            ));
        }
    };
    let mut command = Command::new(crate::codex::codex_bin());
    command
        .current_dir(checkout)
        .args([
            OsStr::new("sandbox"),
            OsStr::new("--permission-profile"),
            OsStr::new(profile),
            OsStr::new("--include-managed-config"),
            OsStr::new("--cd"),
            checkout.as_os_str(),
            OsStr::new("--"),
        ])
        .args(argv);
    if let Some(codex_home) = codex_home {
        command.env(crate::codex::CODEX_HOME_ENV, codex_home);
    }
    let timeout = ctx.command_timeout();
    let output_limit = internal_execution_output_limit();
    let output = run_supervised_execution_command(
        &mut command,
        timeout.duration(),
        output_limit,
        "Codex task preparation",
        observer,
    );
    match output {
        Ok(output) if output.status.success() => Ok(json!({
            "status": "succeeded",
            "sandbox": settings.sandbox,
            "started": true,
            "stdout": bounded_bytes(&output.stdout),
            "stderr": bounded_bytes(&output.stderr),
        })),
        Ok(output) => Err(failure(
            settings,
            "failed",
            true,
            UnexecutedReason::PreExecutionError,
            format!(
                "Task preparation exited with status {}; inspect the retained worktree and preparation output before acknowledging the occurrence",
                output
                    .status
                    .code()
                    .map_or_else(|| "signal".into(), |code| code.to_string())
            ),
            &output.stdout,
            &output.stderr,
        )),
        Err(error) => {
            let (status, started, reason, stdout, stderr) = match &error {
                SupervisedExecutionError::CancelledBeforeStart => (
                    "cancelled",
                    false,
                    UnexecutedReason::CancelledBeforeStart,
                    &[][..],
                    &[][..],
                ),
                SupervisedExecutionError::Cancelled => (
                    "cancelled",
                    true,
                    UnexecutedReason::CancelledBeforeStart,
                    &[][..],
                    &[][..],
                ),
                SupervisedExecutionError::TimedOut => (
                    "timed_out",
                    true,
                    UnexecutedReason::PreExecutionError,
                    &[][..],
                    &[][..],
                ),
                SupervisedExecutionError::OutputLimitExceeded { stdout, stderr, .. } => (
                    "failed",
                    true,
                    UnexecutedReason::PreExecutionError,
                    stdout.as_slice(),
                    stderr.as_slice(),
                ),
                SupervisedExecutionError::Failed {
                    process_started, ..
                } => (
                    "failed",
                    *process_started,
                    UnexecutedReason::PreExecutionError,
                    &[][..],
                    &[][..],
                ),
            };
            let stdout = stdout.to_vec();
            let stderr = stderr.to_vec();
            Err(failure(
                settings,
                status,
                started,
                reason,
                format!(
                    "{}; inspect the retained worktree before acknowledging the occurrence",
                    execution_command_error(error, timeout, output_limit, "Codex task preparation")
                ),
                &stdout,
                &stderr,
            ))
        }
    }
}

fn failure(
    settings: &CodexTaskSettings,
    status: &str,
    started: bool,
    reason: UnexecutedReason,
    error: String,
    stdout: &[u8],
    stderr: &[u8],
) -> PreparationFailure {
    PreparationFailure {
        reason,
        evidence: json!({
            "status": status,
            "sandbox": settings.sandbox,
            "started": started,
            "stdout": bounded_bytes(stdout),
            "stderr": bounded_bytes(stderr),
        }),
        error,
    }
}
