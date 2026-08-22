use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::bootstrap::{GIT_BIN_ENV, external_program, scrub_known_repository_git_environment};
use crate::context::RepoContext;
use crate::runtime::worker_runner::{
    CodexExecMode, CodexExecRequest, CodexPrompt, WorkerReceiptRequest, run_codex_exec,
};

use super::state::LOOP_CACHE_DIR;
use super::workflow::{CodexTaskCheckout, ResolvedWorkflow, WorkflowCompletion, WorkflowTick};

const MAX_PROMPT_BYTES: u64 = 1024 * 1024;
const MAX_OUTPUT_CHARS: usize = 16_000;

pub(super) struct CodexTaskExecution<'a> {
    pub(super) item_key: &'a str,
}

pub(super) fn codex_task_tick(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    execution: CodexTaskExecution<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<WorkflowTick> {
    let settings = workflow
        .codex_task
        .as_ref()
        .ok_or_else(|| anyhow!("Workflow '{}' is missing codex_task settings", workflow.id))?;
    let prompt = read_prompt(ctx, &settings.prompt_file)?;
    let codex_home = workflow
        .codex_home_configured
        .as_deref()
        .map(|home| crate::codex::resolve_configured_home_from_dir(home, ctx.root()))
        .transpose()?;
    let checkout = prepare_checkout(ctx, workflow, execution.item_key, settings.checkout)?;
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
            prompt: CodexPrompt::Stdin(&prompt),
            cancelled: Some(cancelled),
            receipt: WorkerReceiptRequest {
                purpose: "scheduled_codex_task",
                plan_id: None,
                workflow_id: Some(&workflow.id),
                item_key: Some(execution.item_key),
                collect_git_metadata: matches!(settings.checkout, CodexTaskCheckout::Repo),
                collect_worktree_fingerprint: matches!(settings.checkout, CodexTaskCheckout::Repo),
            },
        },
    );

    let (action, completion) = match worker {
        Ok(worker) => {
            let worker_succeeded = worker.output.status.success();
            let checkout = checkout.finish(if worker_succeeded {
                TaskOutcome::Succeeded
            } else {
                TaskOutcome::Failed
            });
            let worker_error = (!worker_succeeded).then(|| {
                format!(
                    "Codex task worker exited with status {}",
                    worker.output.status.code().unwrap_or(1)
                )
            });
            let error = combine_task_errors(worker_error, checkout.error);
            let completion = WorkflowCompletion {
                worker_receipt_id: Some(worker.worker_receipt_id.clone()),
                worktree: checkout.report.retained_worktree(),
                error: error.clone(),
            };
            let action = json!({
                "kind": "codex_task_worker",
                "status": if error.is_none() { "succeeded" } else { "failed" },
                "item_key": execution.item_key,
                "worker_receipt_id": worker.worker_receipt_id,
                "checkout": checkout.report.value(),
                "codex_home_resolved": codex_home.map(|home| home.display().to_string()),
                "output": bounded_text(&String::from_utf8_lossy(&worker.output.stdout)),
                "error": error,
            });
            (action, completion)
        }
        Err(error) => {
            let worker_receipt_id = error.worker_receipt_id().map(str::to_string);
            let checkout = checkout.finish(TaskOutcome::Failed);
            let error = combine_task_errors(Some(format!("{error:#}")), checkout.error);
            let completion = WorkflowCompletion {
                worker_receipt_id: worker_receipt_id.clone(),
                worktree: checkout.report.retained_worktree(),
                error: error.clone(),
            };
            let action = json!({
                "kind": "codex_task_worker",
                "status": "failed",
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
    let canonical_root = fs::canonicalize(ctx.root())
        .with_context(|| format!("Failed to resolve repository root {}", ctx.root().display()))?;
    let path = ctx.root().join(configured);
    let canonical_path = fs::canonicalize(&path)
        .with_context(|| format!("Failed to resolve Codex task prompt {}", path.display()))?;
    if !canonical_path.starts_with(&canonical_root) {
        bail!(
            "Codex task prompt escapes the repository: {}",
            configured.display()
        );
    }
    let metadata = canonical_path
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
    fs::read_to_string(&canonical_path)
        .with_context(|| format!("Failed to read UTF-8 Codex task prompt {}", path.display()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TaskOutcome {
    Succeeded,
    Failed,
}

enum PreparedCheckout {
    Repo {
        path: PathBuf,
    },
    Worktree {
        repo_root: PathBuf,
        path: PathBuf,
        initial_head: String,
    },
}

struct CheckoutCompletion {
    report: CheckoutReport,
    error: Option<String>,
}

enum CheckoutReport {
    Repository {
        path: PathBuf,
        dirty: Option<bool>,
    },
    Worktree {
        path: PathBuf,
        retained: bool,
        dirty: Option<bool>,
        head_changed: Option<bool>,
    },
}

impl CheckoutReport {
    fn retained_worktree(&self) -> Option<String> {
        match self {
            Self::Worktree {
                path,
                retained: true,
                ..
            } => Some(path.display().to_string()),
            Self::Repository { .. } | Self::Worktree { .. } => None,
        }
    }

    fn value(&self) -> Value {
        match self {
            Self::Repository { path, dirty } => json!({
                "mode": "repo",
                "path": path,
                "retained": true,
                "dirty": dirty,
            }),
            Self::Worktree {
                path,
                retained,
                dirty,
                head_changed,
            } => json!({
                "mode": "worktree",
                "path": path,
                "retained": retained,
                "dirty": dirty,
                "head_changed": head_changed,
            }),
        }
    }
}

impl PreparedCheckout {
    fn path(&self) -> &Path {
        match self {
            Self::Repo { path } | Self::Worktree { path, .. } => path,
        }
    }

    fn finish(self, outcome: TaskOutcome) -> CheckoutCompletion {
        match self {
            Self::Repo { path } => match git_is_dirty(&path) {
                Ok(dirty) => CheckoutCompletion {
                    report: CheckoutReport::Repository {
                        path,
                        dirty: Some(dirty),
                    },
                    error: None,
                },
                Err(error) => CheckoutCompletion {
                    report: CheckoutReport::Repository { path, dirty: None },
                    error: Some(format!(
                        "Failed to inspect retained task checkout: {error:#}"
                    )),
                },
            },
            Self::Worktree {
                repo_root,
                path,
                initial_head,
            } => {
                let dirty = git_is_dirty(&path);
                let final_head = git_stdout(&path, ["rev-parse", "HEAD"]);
                let mut errors = Vec::new();
                if let Err(error) = &dirty {
                    errors.push(format!("Failed to inspect task worktree status: {error:#}"));
                }
                if let Err(error) = &final_head {
                    errors.push(format!("Failed to inspect task worktree HEAD: {error:#}"));
                }
                let dirty = dirty.ok();
                let head_changed = final_head.ok().map(|head| head != initial_head);
                let mut retained = outcome == TaskOutcome::Failed
                    || dirty.unwrap_or(true)
                    || head_changed.unwrap_or(true);
                if !retained && let Err(error) = remove_worktree(&repo_root, &path, false) {
                    retained = true;
                    errors.push(format!("Failed to remove clean task worktree: {error:#}"));
                }
                CheckoutCompletion {
                    report: CheckoutReport::Worktree {
                        path,
                        retained,
                        dirty,
                        head_changed,
                    },
                    error: (!errors.is_empty()).then(|| errors.join("; ")),
                }
            }
        }
    }
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

fn prepare_checkout(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    item_key: &str,
    checkout: CodexTaskCheckout,
) -> Result<PreparedCheckout> {
    if checkout == CodexTaskCheckout::Repo {
        return Ok(PreparedCheckout::Repo {
            path: ctx.root().to_path_buf(),
        });
    }

    let digest = Sha256::digest(format!("{}\0{item_key}", workflow.id).as_bytes());
    let name = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let path = ctx
        .root()
        .join(LOOP_CACHE_DIR)
        .join("worktrees")
        .join("tasks")
        .join(name);
    if path.exists() {
        bail!("Codex task worktree already exists: {}", path.display());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create Codex task worktree parent {}",
                parent.display()
            )
        })?;
    }
    let initial_head = git_stdout(ctx.root(), ["rev-parse", "HEAD"])?;
    let output = git_output(
        ctx.root(),
        [
            OsString::from("worktree"),
            OsString::from("add"),
            OsString::from("--detach"),
            path.as_os_str().to_os_string(),
            OsString::from(&initial_head),
        ],
    )?;
    if !output.status.success() {
        let error = git_error("Failed to create Codex task worktree", output);
        let _ = remove_worktree(ctx.root(), &path, true);
        return Err(error);
    }
    Ok(PreparedCheckout::Worktree {
        repo_root: ctx.root().to_path_buf(),
        path,
        initial_head,
    })
}

fn git_is_dirty(worktree: &Path) -> Result<bool> {
    let output = git_output(
        worktree,
        ["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if !output.status.success() {
        return Err(git_error("Failed to inspect Codex task worktree", output));
    }
    Ok(!output.stdout.is_empty())
}

fn git_stdout<I, S>(cwd: &Path, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_output(cwd, args)?;
    if !output.status.success() {
        return Err(git_error("Git command failed", output));
    }
    String::from_utf8(output.stdout)
        .context("Git command returned non-UTF-8 output")
        .map(|output| output.trim().to_string())
}

fn remove_worktree(repo_root: &Path, worktree: &Path, force: bool) -> Result<()> {
    let mut args = vec![OsString::from("worktree"), OsString::from("remove")];
    if force {
        args.push(OsString::from("--force"));
    }
    args.push(worktree.as_os_str().to_os_string());
    let output = git_output(repo_root, args)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(git_error("Failed to remove Codex task worktree", output))
    }
}

fn git_output<I, S>(cwd: &Path, args: I) -> Result<std::process::Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(external_program(GIT_BIN_ENV, "git"));
    command
        .current_dir(cwd)
        .arg("--no-replace-objects")
        .args(args);
    scrub_known_repository_git_environment(&mut command);
    command
        .output()
        .with_context(|| format!("Failed to start git in {}", cwd.display()))
}

fn git_error(label: &str, output: std::process::Output) -> anyhow::Error {
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

fn bounded_text(text: &str) -> String {
    text.chars().take(MAX_OUTPUT_CHARS).collect()
}
