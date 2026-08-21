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
use super::workflow::{CodexTaskCheckout, ResolvedWorkflow, WorkflowTick};

const MAX_PROMPT_BYTES: u64 = 1024 * 1024;
const MAX_OUTPUT_CHARS: usize = 16_000;

pub(super) struct CodexTaskExecution<'a> {
    pub(super) item_key: &'a str,
}

pub(super) fn codex_task_tick(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    execution: CodexTaskExecution<'_>,
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
            root: &checkout.path,
            codex_home: codex_home.as_deref(),
            mode: CodexExecMode::Exec,
            model: settings.model.as_deref(),
            approval_policy: Some("never"),
            sandbox: Some(&settings.sandbox),
            ephemeral: true,
            extra_args: Vec::new(),
            output_schema: None,
            prompt: CodexPrompt::Stdin(&prompt),
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

    let action = match worker {
        Ok(worker) => {
            let succeeded = worker.output.status.success();
            let cleanup = checkout.finish(succeeded)?;
            json!({
                "kind": "codex_task_worker",
                "status": if succeeded { "succeeded" } else { "failed" },
                "item_key": execution.item_key,
                "worker_receipt_id": worker.worker_receipt_id,
                "checkout": cleanup,
                "codex_home_resolved": codex_home.map(|home| home.display().to_string()),
                "output": bounded_text(&String::from_utf8_lossy(&worker.output.stdout)),
                "error": if succeeded {
                    Value::Null
                } else {
                    Value::String(format!(
                        "Codex task worker exited with status {}",
                        worker.output.status.code().unwrap_or(1)
                    ))
                },
            })
        }
        Err(error) => {
            let cleanup = checkout.finish(false)?;
            json!({
                "kind": "codex_task_worker",
                "status": "failed",
                "item_key": execution.item_key,
                "worker_receipt_id": Value::Null,
                "checkout": cleanup,
                "codex_home_resolved": codex_home.map(|home| home.display().to_string()),
                "output": Value::Null,
                "error": format!("{error:#}"),
            })
        }
    };

    Ok(WorkflowTick {
        observed: json!({
            "kind": "codex_task",
            "prompt_file": settings.prompt_file.display().to_string(),
            "sandbox": settings.sandbox,
            "checkout": settings.checkout.as_str(),
        }),
        actions: vec![action],
    })
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

struct PreparedCheckout {
    repo_root: PathBuf,
    path: PathBuf,
    isolated: bool,
}

impl PreparedCheckout {
    fn finish(self, succeeded: bool) -> Result<Value> {
        if !self.isolated {
            return Ok(json!({
                "mode": "repo",
                "path": self.path,
                "retained": true,
                "dirty": git_is_dirty(&self.path)?,
            }));
        }
        let dirty = git_is_dirty(&self.path)?;
        let retain = !succeeded || dirty;
        if !retain {
            remove_worktree(&self.repo_root, &self.path, false)?;
        }
        Ok(json!({
            "mode": "worktree",
            "path": self.path,
            "retained": retain,
            "dirty": dirty,
        }))
    }
}

fn prepare_checkout(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    item_key: &str,
    checkout: CodexTaskCheckout,
) -> Result<PreparedCheckout> {
    if checkout == CodexTaskCheckout::Repo {
        return Ok(PreparedCheckout {
            repo_root: ctx.root().to_path_buf(),
            path: ctx.root().to_path_buf(),
            isolated: false,
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
    let output = git_output(
        ctx.root(),
        [
            OsString::from("worktree"),
            OsString::from("add"),
            OsString::from("--detach"),
            path.as_os_str().to_os_string(),
            OsString::from("HEAD"),
        ],
    )?;
    if !output.status.success() {
        let error = git_error("Failed to create Codex task worktree", output);
        let _ = remove_worktree(ctx.root(), &path, true);
        return Err(error);
    }
    Ok(PreparedCheckout {
        repo_root: ctx.root().to_path_buf(),
        path,
        isolated: true,
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
