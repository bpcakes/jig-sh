use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
#[cfg(unix)]
use cap_std::fs::OpenOptionsExt as _;
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions as CapabilityOpenOptions},
};
use jig_owned_process::{OwnedProcessOutputStream, ProcessOutputOverflowPolicy};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::state::LOOP_RUNTIME_DIR;
use super::workflow::{
    CodexTaskCheckout, CodexTaskSettings, ResolvedWorkflow, UnexecutedReason, WorkflowCompletion,
    WorkflowExecution, WorkflowOutcome, WorkflowTick,
};
use crate::bootstrap::{GIT_BIN_ENV, external_program, scrub_known_repository_git_environment};
use crate::context::RepoContext;
use crate::execution::{
    ExecutionCommandOutput, ExecutionControl, NoopExecutionObserver, SupervisedExecutionError,
    execution_command_error, internal_execution_output_limit, run_authoritative_execution_command,
    run_supervised_execution_command,
};
use crate::runtime::worker_runner::{
    CodexExecMode, CodexExecOutcome, CodexExecRequest, CodexPrompt, WorkerReceiptRequest,
    run_codex_exec,
};

mod checkout;
mod pre_execution;

use checkout::{PreparedCheckout, TaskOutcome};
use pre_execution::{CheckoutPreparationFailure, unexecuted_task_failure};

const MAX_PROMPT_BYTES: u64 = 1024 * 1024;
const MAX_OUTPUT_CHARS: usize = 16_000;
const WORKER_RECEIPT_PATH: &str = ".agent/state/receipts.jsonl";
const WORKER_RECEIPT_EXCLUDE: &str = ":(exclude).agent/state/receipts.jsonl";

pub(super) struct CodexTaskExecution<'a> {
    pub(super) item_key: &'a str,
}

pub(super) fn codex_task_tick(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    execution: CodexTaskExecution<'_>,
    observer: &mut dyn ExecutionControl,
) -> Result<WorkflowTick> {
    let settings = workflow
        .codex_task
        .as_ref()
        .ok_or_else(|| anyhow!("Workflow '{}' is missing codex_task settings", workflow.id))?;
    let prompt = match read_prompt(ctx, &settings.prompt_file) {
        Ok(prompt) => prompt,
        Err(error) => {
            return Ok(unexecuted_task_failure(
                settings,
                UnexecutedReason::PreExecutionError,
                execution.item_key,
                None,
                None,
                format!("{error:#}"),
            ));
        }
    };
    let codex_home = match workflow
        .codex_home_configured
        .as_deref()
        .map(|home| crate::codex::resolve_configured_home_from_dir(home, ctx.root()))
        .transpose()
    {
        Ok(codex_home) => codex_home,
        Err(error) => {
            return Ok(unexecuted_task_failure(
                settings,
                UnexecutedReason::PreExecutionError,
                execution.item_key,
                None,
                None,
                format!("{error:#}"),
            ));
        }
    };
    let checkout = match prepare_checkout(
        ctx,
        workflow,
        execution.item_key,
        settings.checkout,
        observer,
    ) {
        Ok(checkout) => checkout,
        Err(error) => {
            return Ok(unexecuted_task_failure(
                settings,
                error.reason(),
                execution.item_key,
                codex_home.as_deref(),
                error.retained_worktree().map(str::to_string),
                format!("{error:#}"),
            ));
        }
    };
    let worker = run_codex_exec(
        ctx,
        CodexExecRequest {
            root: checkout.path(),
            codex_home: codex_home.as_deref(),
            mode: CodexExecMode::Exec,
            model: settings.model.as_deref(),
            approval_policy: Some("never"),
            sandbox: Some(&settings.sandbox),
            ephemeral: true,
            extra_args: Vec::new(),
            output_schema: None,
            transcript_overflow_policy: ProcessOutputOverflowPolicy::Truncate,
            prompt: CodexPrompt::Stdin(&prompt),
            receipt: WorkerReceiptRequest {
                purpose: "scheduled_codex_task",
                plan_id: None,
                workflow_id: Some(&workflow.id),
                item_key: Some(execution.item_key),
                collect_git_metadata: matches!(settings.checkout, CodexTaskCheckout::Repo),
                collect_worktree_fingerprint: matches!(settings.checkout, CodexTaskCheckout::Repo),
            },
            phase: None,
        },
        observer,
    );

    let (action, completion) = match worker {
        Ok(CodexExecOutcome::Completed(worker)) => {
            let worker_succeeded = worker.status().success();
            let checkout = checkout.finish(
                if worker_succeeded {
                    TaskOutcome::Succeeded
                } else {
                    TaskOutcome::Failed
                },
                ctx,
                Some(worker.worker_receipt_id()),
            );
            let worker_error = (!worker_succeeded).then(|| {
                format!(
                    "Codex task worker exited with status {}",
                    worker.status().code().unwrap_or(1)
                )
            });
            let (outcome, error) = classify_checkout(
                &checkout,
                if worker_succeeded && checkout.error.is_none() {
                    WorkflowOutcome::Succeeded
                } else {
                    WorkflowOutcome::Failed
                },
                CheckoutTermination::Completed,
                worker_error,
            );
            let completion = WorkflowCompletion {
                outcome,
                execution: WorkflowExecution::Executed,
                worker_receipt_id: Some(worker.worker_receipt_id().to_owned()),
                worktree: checkout.report.retained_worktree(),
                error: error.clone(),
            };
            let action = json!({
                "kind": "codex_task_worker",
                "status": match outcome {
                    WorkflowOutcome::Succeeded => "succeeded",
                    WorkflowOutcome::Failed => "failed",
                    WorkflowOutcome::NeedsAttention => "needs_attention",
                },
                "item_key": execution.item_key,
                "worker_receipt_id": worker.worker_receipt_id(),
                "checkout": checkout.report.value(),
                "codex_home_resolved": codex_home.map(|home| home.display().to_string()),
                "output": bounded_bytes(worker.authoritative_stdout()),
                "provider_stdout": bounded_text(worker.provider_stdout()),
                "provider_stdout_truncated": worker.provider_stdout_truncated(),
                "error": error,
            });
            (action, completion)
        }
        Ok(CodexExecOutcome::Cancelled {
            before_start,
            worker_receipt_id,
        }) => {
            let checkout = checkout.finish(
                if before_start {
                    TaskOutcome::Succeeded
                } else {
                    TaskOutcome::Failed
                },
                ctx,
                Some(&worker_receipt_id),
            );
            let timing = if before_start {
                " before the worker started"
            } else {
                " while the worker was running"
            };
            let termination = if before_start {
                CheckoutTermination::BeforeStart
            } else {
                CheckoutTermination::AfterStart
            };
            let (outcome, error) = classify_checkout(
                &checkout,
                WorkflowOutcome::Failed,
                termination,
                Some(format!("Scheduled Codex task was cancelled{timing}")),
            );
            let completion = WorkflowCompletion {
                outcome,
                execution: if before_start {
                    WorkflowExecution::Unexecuted(UnexecutedReason::CancelledBeforeStart)
                } else {
                    WorkflowExecution::Executed
                },
                worker_receipt_id: Some(worker_receipt_id.clone()),
                worktree: checkout.report.retained_worktree(),
                error: error.clone(),
            };
            let action = json!({
                "kind": "codex_task_worker",
                "status": task_outcome_status(outcome),
                "item_key": execution.item_key,
                "worker_receipt_id": worker_receipt_id,
                "checkout": checkout.report.value(),
                "codex_home_resolved": codex_home.map(|home| home.display().to_string()),
                "output": Value::Null,
                "error": error,
            });
            (action, completion)
        }
        Err(error) => {
            let worker_failure =
                error.downcast_ref::<crate::runtime::worker_runner::CodexExecFailure>();
            let worker_receipt_id = worker_failure
                .and_then(|error| error.worker_receipt_id())
                .map(str::to_string);
            let unexecuted = worker_failure.is_some_and(|error| error.worker_was_unexecuted());
            let unexecuted_reason =
                if worker_failure.is_some_and(|error| error.worker_was_cancelled_before_start()) {
                    UnexecutedReason::CancelledBeforeStart
                } else {
                    UnexecutedReason::PreExecutionError
                };
            let checkout = checkout.finish(
                if unexecuted {
                    TaskOutcome::Succeeded
                } else {
                    TaskOutcome::Failed
                },
                ctx,
                worker_receipt_id.as_deref(),
            );
            let termination = if unexecuted {
                CheckoutTermination::BeforeStart
            } else {
                CheckoutTermination::AfterStart
            };
            let (outcome, error) = classify_checkout(
                &checkout,
                WorkflowOutcome::Failed,
                termination,
                Some(format!("{error:#}")),
            );
            let retained_worktree = checkout.report.retained_worktree();
            let completion = WorkflowCompletion {
                outcome,
                execution: if unexecuted {
                    WorkflowExecution::Unexecuted(unexecuted_reason)
                } else {
                    WorkflowExecution::Executed
                },
                worker_receipt_id: worker_receipt_id.clone(),
                worktree: retained_worktree,
                error: error.clone(),
            };
            let action = json!({
                "kind": "codex_task_worker",
                "status": task_outcome_status(outcome),
                "item_key": execution.item_key,
                "worker_receipt_id": worker_receipt_id,
                "checkout": checkout.report.value(),
                "codex_home_resolved": codex_home.map(|home| home.display().to_string()),
                "output": Value::Null,
                "error": error,
            });
            (action, completion)
        }
    };

    Ok(WorkflowTick::with_completion(
        json!({
            "kind": "codex_task",
            "prompt_file": settings.prompt_file.display().to_string(),
            "sandbox": settings.sandbox,
            "checkout": settings.checkout.as_str(),
        }),
        vec![action],
        completion,
    ))
}

fn read_prompt(ctx: &RepoContext, configured: &Path) -> Result<String> {
    let path = ctx.root().join(configured);
    let mut file = open_prompt_file(ctx.root(), configured).with_context(|| {
        format!(
            "Codex task prompt must resolve inside the repository: {}",
            path.display()
        )
    })?;
    let metadata = file
        .metadata()
        .with_context(|| format!("Failed to inspect Codex task prompt {}", path.display()))?;
    if !metadata.is_file() {
        bail!(
            "Codex task prompt is not a regular file: {}",
            path.display()
        );
    }
    if metadata.len() > MAX_PROMPT_BYTES {
        bail!(
            "Codex task prompt exceeds {MAX_PROMPT_BYTES} bytes: {}",
            path.display()
        );
    }
    let mut prompt = Vec::new();
    file.by_ref()
        .take(MAX_PROMPT_BYTES + 1)
        .read_to_end(&mut prompt)
        .with_context(|| format!("Failed to read Codex task prompt {}", path.display()))?;
    decode_prompt(prompt, &path)
}

fn open_prompt_file(root: &Path, configured: &Path) -> Result<File> {
    open_prompt_file_with_observer(root, configured, || Ok(()))
}

fn open_prompt_file_with_observer(
    root: &Path,
    configured: &Path,
    after_root_opened: impl FnOnce() -> Result<()>,
) -> Result<File> {
    let repository = Dir::open_ambient_dir(root, ambient_authority())
        .with_context(|| format!("Failed to open repository root {}", root.display()))?;
    after_root_opened()?;
    let mut options = CapabilityOpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NONBLOCK);
    repository
        .open_with(configured, &options)
        .map(cap_std::fs::File::into_std)
        .map_err(|error| {
            anyhow!(
                "Codex task prompt must resolve inside the repository: {}: {error}",
                configured.display()
            )
        })
}

fn decode_prompt(prompt: Vec<u8>, path: &Path) -> Result<String> {
    if prompt.len() as u64 > MAX_PROMPT_BYTES {
        bail!(
            "Codex task prompt exceeds {MAX_PROMPT_BYTES} bytes: {}",
            path.display()
        );
    }
    String::from_utf8(prompt)
        .with_context(|| format!("Failed to read UTF-8 Codex task prompt {}", path.display()))
}

fn combine_task_errors(primary: Option<String>, cleanup: Option<String>) -> Option<String> {
    match (primary, cleanup) {
        (Some(primary), Some(cleanup)) => Some(format!(
            "{primary}; checkout cleanup also failed: {cleanup}"
        )),
        (Some(primary), None) => Some(primary),
        (None, Some(cleanup)) => Some(cleanup),
        (None, None) => None,
    }
}

#[derive(Clone, Copy)]
enum CheckoutTermination {
    Completed,
    BeforeStart,
    AfterStart,
}

fn classify_checkout(
    checkout: &checkout::CheckoutCompletion,
    fallback: WorkflowOutcome,
    termination: CheckoutTermination,
    primary_error: Option<String>,
) -> (WorkflowOutcome, Option<String>) {
    let repository_integrity_failed = checkout.report.repository_requires_attention();
    let repository_side_effects_ambiguous =
        matches!(termination, CheckoutTermination::AfterStart) && checkout.report.is_repository();
    let retained_before_start = matches!(termination, CheckoutTermination::BeforeStart)
        && checkout.report.retained_worktree().is_some();
    let needs_attention =
        repository_integrity_failed || repository_side_effects_ambiguous || retained_before_start;
    let error = combine_task_errors(primary_error, checkout.error.clone());
    let error = combine_task_errors(
        error,
        repository_integrity_failed.then(|| {
            "Codex task left the shared repository checkout dirty or its state could not be verified"
                .to_string()
        }),
    );
    (
        if needs_attention {
            WorkflowOutcome::NeedsAttention
        } else {
            fallback
        },
        error,
    )
}

const fn task_outcome_status(outcome: WorkflowOutcome) -> &'static str {
    match outcome {
        WorkflowOutcome::Succeeded => "succeeded",
        WorkflowOutcome::Failed => "failed",
        WorkflowOutcome::NeedsAttention => "needs_attention",
    }
}

fn prepare_checkout(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    item_key: &str,
    checkout: CodexTaskCheckout,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<PreparedCheckout, CheckoutPreparationFailure> {
    if checkout == CodexTaskCheckout::Repo {
        if observer.cancelled() {
            return Err(CheckoutPreparationFailure::cancelled(anyhow!(
                "Scheduled Codex task was cancelled before shared-checkout preflight"
            )));
        }
        require_ignored_runtime_path(
            ctx,
            Path::new(LOOP_RUNTIME_DIR),
            "Codex task runtime path",
            "repo",
            observer,
        )?;
        match repo_task_has_changes(ctx, ctx.root(), observer) {
            Ok(false) => {}
            Ok(true) => {
                return Err(CheckoutPreparationFailure::new(anyhow!(
                    "Shared repository checkout is dirty before Codex task execution; preserve or discard the existing changes before retrying"
                )));
            }
            Err(error) => {
                let error = error.context(
                    "Failed to verify that the shared repository checkout is clean before Codex task execution",
                );
                return Err(if observer.cancelled() {
                    CheckoutPreparationFailure::cancelled(error)
                } else {
                    CheckoutPreparationFailure::new(error)
                });
            }
        }
        return Ok(PreparedCheckout::Repo {
            path: ctx.root().to_path_buf(),
            receipt_journal: checkout::ReceiptJournalBaseline::capture(ctx)?,
        });
    }

    let digest = Sha256::digest(format!("{}\0{item_key}", workflow.id).as_bytes());
    let name = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let path = ctx
        .root()
        .join(LOOP_RUNTIME_DIR)
        .join("worktrees")
        .join("tasks")
        .join(name);
    if path.exists() {
        return Err(CheckoutPreparationFailure::retained(
            &path,
            anyhow!("Codex task worktree already exists: {}", path.display()),
        ));
    }
    require_ignored_task_worktree_root(ctx, observer)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create Codex task worktree parent {}",
                parent.display()
            )
        })?;
    }
    let initial_head = git_stdout(ctx, ctx.root(), ["rev-parse", "HEAD"], observer)?;
    let output = match git_output(
        ctx,
        ctx.root(),
        [
            OsString::from("worktree"),
            OsString::from("add"),
            OsString::from("--detach"),
            path.as_os_str().to_os_string(),
            OsString::from(&initial_head),
        ],
        observer,
    ) {
        Ok(output) => output,
        Err(error) => {
            let mut cleanup_observer = NoopExecutionObserver;
            let cleanup = remove_worktree(ctx, ctx.root(), &path, true, &mut cleanup_observer);
            return Err(checkout_preparation_error(&path, error, cleanup.err()));
        }
    };
    if !output.status.success() {
        let error = git_error("Failed to create Codex task worktree", output);
        let mut cleanup_observer = NoopExecutionObserver;
        let cleanup = remove_worktree(ctx, ctx.root(), &path, true, &mut cleanup_observer);
        return Err(checkout_preparation_error(&path, error, cleanup.err()));
    }
    Ok(PreparedCheckout::Worktree {
        repo_root: ctx.root().to_path_buf(),
        path,
        initial_head,
    })
}

fn require_ignored_task_worktree_root(
    ctx: &RepoContext,
    observer: &mut dyn ExecutionControl,
) -> Result<()> {
    let worktree_root = Path::new(LOOP_RUNTIME_DIR).join("worktrees/tasks");
    require_ignored_runtime_path(
        ctx,
        &worktree_root,
        "Codex task worktree path",
        "worktree",
        observer,
    )
}

fn require_ignored_runtime_path(
    ctx: &RepoContext,
    path: &Path,
    description: &str,
    checkout: &str,
    observer: &mut dyn ExecutionControl,
) -> Result<()> {
    let output = git_output(
        ctx,
        ctx.root(),
        [
            OsString::from("check-ignore"),
            OsString::from("--quiet"),
            OsString::from("--"),
            path.as_os_str().to_os_string(),
        ],
        observer,
    )?;
    match output.status.code() {
        Some(0) => Ok(()),
        Some(1) => bail!(
            "{description} is not ignored by Git: {}; refresh the managed .gitignore with `scripts/jig update --recopy` before using {checkout} checkout",
            path.display()
        ),
        _ => Err(git_error(
            &format!("Failed to verify that the {description} is ignored"),
            output,
        )),
    }
}

fn git_is_dirty(
    ctx: &RepoContext,
    worktree: &Path,
    observer: &mut dyn ExecutionControl,
) -> Result<bool> {
    git_status_has_changes(ctx, worktree, false, observer)
}

fn repo_task_has_changes(
    ctx: &RepoContext,
    worktree: &Path,
    observer: &mut dyn ExecutionControl,
) -> Result<bool> {
    git_status_has_changes(ctx, worktree, true, observer)
}

fn git_status_has_changes(
    ctx: &RepoContext,
    worktree: &Path,
    exclude_worker_receipt: bool,
    observer: &mut dyn ExecutionControl,
) -> Result<bool> {
    let mut args = vec![
        OsString::from("status"),
        OsString::from("--porcelain=v1"),
        OsString::from("--untracked-files=normal"),
        OsString::from("--"),
        OsString::from("."),
    ];
    if exclude_worker_receipt {
        args.push(OsString::from(WORKER_RECEIPT_EXCLUDE));
    }
    let (mut command, label) = git_command(worktree, args);
    let timeout = ctx.command_timeout();
    let output_limit = internal_execution_output_limit();
    let output = match run_supervised_execution_command(
        &mut command,
        timeout.duration(),
        output_limit,
        &label,
        observer,
    ) {
        Ok(output) => output,
        Err(SupervisedExecutionError::OutputLimitExceeded {
            stream: OwnedProcessOutputStream::Stdout,
            ..
        }) => return Ok(true),
        Err(error) => {
            return Err(
                execution_command_error(error, timeout, output_limit, &label).into_anyhow(),
            );
        }
    };
    if !output.status.success() {
        return Err(git_error("Failed to inspect Codex task worktree", output));
    }
    Ok(!output.stdout.is_empty())
}

fn git_stdout<I, S>(
    ctx: &RepoContext,
    cwd: &Path,
    args: I,
    observer: &mut dyn ExecutionControl,
) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_output(ctx, cwd, args, observer)?;
    if !output.status.success() {
        return Err(git_error("Git command failed", output));
    }
    String::from_utf8(output.stdout)
        .context("Git command returned non-UTF-8 output")
        .map(|output| output.trim().to_string())
}

fn remove_worktree(
    ctx: &RepoContext,
    repo_root: &Path,
    worktree: &Path,
    force: bool,
    observer: &mut dyn ExecutionControl,
) -> Result<()> {
    let mut args = vec![OsString::from("worktree"), OsString::from("remove")];
    if force {
        args.push(OsString::from("--force"));
    }
    args.push(worktree.as_os_str().to_os_string());
    let output = git_output(ctx, repo_root, args, observer)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(git_error("Failed to remove Codex task worktree", output))
    }
}

fn git_output<I, S>(
    ctx: &RepoContext,
    cwd: &Path,
    args: I,
    observer: &mut dyn ExecutionControl,
) -> Result<ExecutionCommandOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let (mut command, label) = git_command(cwd, args);
    run_authoritative_execution_command(
        &mut command,
        ctx.command_timeout(),
        internal_execution_output_limit(),
        &label,
        observer,
    )
    .map_err(|error| error.into_anyhow())
}

fn git_command<I, S>(cwd: &Path, args: I) -> (Command, String)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_os_string())
        .collect::<Vec<_>>();
    let operation = args
        .first()
        .map(|arg| arg.to_string_lossy())
        .unwrap_or_else(|| "command".into());
    let label = format!("Codex task git {operation}");
    let mut command = Command::new(external_program(GIT_BIN_ENV, "git"));
    command
        .current_dir(cwd)
        .arg("--no-replace-objects")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    scrub_known_repository_git_environment(&mut command);
    (command, label)
}

fn git_error(label: &str, output: ExecutionCommandOutput) -> anyhow::Error {
    anyhow!(
        "{} with status {}. stdout: {} stderr: {}",
        label,
        output
            .status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "signal".into()),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn checkout_preparation_error(
    path: &Path,
    error: anyhow::Error,
    cleanup_error: Option<anyhow::Error>,
) -> CheckoutPreparationFailure {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            let error = match cleanup_error {
                Some(cleanup_error) => anyhow!(
                    "{error:#}; partial Codex task worktree may remain at {}; cleanup failed: {cleanup_error:#}",
                    path.display()
                ),
                None => anyhow!(
                    "{error:#}; partial Codex task worktree remains at {} after cleanup",
                    path.display()
                ),
            };
            CheckoutPreparationFailure::retained(path, error)
        }
        Err(inspect_error) if inspect_error.kind() == std::io::ErrorKind::NotFound => {
            CheckoutPreparationFailure::new(error)
        }
        Err(inspect_error) => {
            let error = match cleanup_error {
                Some(cleanup_error) => anyhow!(
                    "{error:#}; cleanup failed and Jig could not inspect the possible partial Codex task worktree at {}: {cleanup_error:#}; inspection failed: {inspect_error}",
                    path.display()
                ),
                None => anyhow!(
                    "{error:#}; Jig could not verify cleanup of the possible partial Codex task worktree at {}: {inspect_error}",
                    path.display()
                ),
            };
            CheckoutPreparationFailure::retained(path, error)
        }
    }
}

fn bounded_text(text: &str) -> String {
    text.chars().take(MAX_OUTPUT_CHARS).collect()
}

fn bounded_bytes(bytes: &[u8]) -> String {
    bounded_text(&String::from_utf8_lossy(bytes))
}

#[cfg(test)]
#[path = "codex_task/tests.rs"]
mod tests;

#[cfg(test)]
mod regression_tests;
