//! Merging the base branch and pushing the PR branch.

use std::path::Path;

use anyhow::anyhow;
use jig_context::{CommandTimeout, RepoContext};
use jig_execution::{ExecutionCommandError, ExecutionControl, NoopExecutionObserver};
use serde_json::{Value, json};

use super::git::{
    git_checked, git_error, git_execution_output, git_output, git_stdout,
    git_with_pr_manager_identity, require_no_merge_introduced_conflict_markers,
};
use super::outcome::{PrPushError, PrPushResult, PrRepairStepError, PrRepairStepResult};

pub(super) fn start_base_merge(
    ctx: &RepoContext,
    worktree: &Path,
    base_ref: &str,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<Value> {
    let base_ref = remote_branch_ref(base_ref);
    let fetch = git_output(ctx, worktree, ["fetch", "origin", &base_ref], observer)?;
    if !fetch.status.success() {
        return Err(PrRepairStepError::failed(git_error(
            "git fetch base branch failed",
            fetch,
        )));
    }
    let base_head = git_stdout(
        ctx,
        worktree,
        ["rev-parse", "--verify", "FETCH_HEAD^{commit}"],
        observer,
    )?;
    let merge = git_output(
        ctx,
        worktree,
        git_with_pr_manager_identity(["merge", "--no-edit", "FETCH_HEAD"]),
        observer,
    )?;
    Ok(json!({
        "exit_status": merge.status.code().unwrap_or(1),
        "stdout": String::from_utf8_lossy(&merge.stdout),
        "stderr": String::from_utf8_lossy(&merge.stderr),
        "conflicts": !merge.status.success(),
        "base_head": base_head,
    }))
}

pub(super) fn validation_tree_after_base_merge(
    ctx: &RepoContext,
    worktree: &Path,
    merge: Option<&Value>,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<String> {
    if merge
        .and_then(|value| value.get("conflicts"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        // The ort strategy records its conflicted working-tree snapshot here.
        // Comparing the worker result with this tree validates only the repair,
        // rather than revalidating every incoming base-branch line.
        git_stdout(
            ctx,
            worktree,
            ["rev-parse", "--verify", "AUTO_MERGE^{tree}"],
            observer,
        )
    } else {
        git_stdout(ctx, worktree, ["rev-parse", "HEAD"], observer)
    }
}

pub(super) fn remote_branch_ref(branch: &str) -> String {
    format!("refs/heads/{branch}")
}

pub(super) fn require_remote_head(
    ctx: &RepoContext,
    cwd: &Path,
    remote_ref: &str,
    expected_head: &str,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<()> {
    let observed = remote_head_for_ref(ctx, cwd, remote_ref, observer)?;
    if observed != expected_head {
        return Err(PrRepairStepError::failed(anyhow!(
            "Remote ref {remote_ref} changed after the GitHub snapshot: expected {expected_head}, found {observed}"
        )));
    }
    Ok(())
}

pub(super) fn remote_head_for_ref(
    ctx: &RepoContext,
    cwd: &Path,
    remote_ref: &str,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<String> {
    let stdout = git_stdout(
        ctx,
        cwd,
        ["ls-remote", "--exit-code", "origin", remote_ref],
        observer,
    )?;
    let observed = remote_head_from_ls_remote(stdout.as_bytes(), remote_ref).ok_or_else(|| {
        PrRepairStepError::failed(anyhow!("Remote ref {remote_ref} was not found"))
    })?;
    Ok(observed.to_string())
}

pub(super) fn commit_and_push(
    ctx: &RepoContext,
    worktree: &Path,
    head_ref: &str,
    base_head: &str,
    incoming_base_head: Option<&str>,
    validation_tree: &str,
    observer: &mut dyn ExecutionControl,
) -> PrPushResult<Value> {
    let dirty_before_commit = git_stdout(ctx, worktree, ["status", "--porcelain"], observer)?;
    if !dirty_before_commit.is_empty() {
        // Git metadata is deliberately outside the workspace-write worker's
        // authority. The parent owns staging as well as validation and commit.
        git_checked(ctx, worktree, ["add", "-A"], observer)?;
        let unmerged = git_stdout(ctx, worktree, ["ls-files", "--unmerged"], observer)?;
        if !unmerged.is_empty() {
            return Err(PrRepairStepError::failed(anyhow!(
                "PR manager parent staging left unresolved merge entries in the Git index"
            ))
            .into());
        }
        git_checked(
            ctx,
            worktree,
            ["diff", "--check", validation_tree.trim(), "--"],
            observer,
        )?;
        // AUTO_MERGE deliberately includes the original conflict markers, so its
        // worker-only diff cannot prove that the resolution removed them. Intersect
        // marker diagnostics from both merge parents: incoming examples remain valid,
        // while markers introduced by Git are absent from both parents.
        require_no_merge_introduced_conflict_markers(
            ctx,
            worktree,
            base_head,
            incoming_base_head,
            None,
            observer,
        )?;
        git_checked(
            ctx,
            worktree,
            git_with_pr_manager_identity([
                "commit",
                "-m",
                &format!("chore: update PR via Jig PR manager ({head_ref})"),
            ]),
            observer,
        )?;
    }
    let final_head = git_stdout(ctx, worktree, ["rev-parse", "HEAD"], observer)?;
    let changed = final_head != base_head.trim();
    if !changed {
        return Ok(json!({
            "status": "no_changes",
            "pushed": false,
            "base_head": base_head.trim(),
            "final_head": final_head.trim(),
        }));
    }
    git_checked(
        ctx,
        worktree,
        ["diff", "--check", validation_tree.trim(), &final_head, "--"],
        observer,
    )?;
    require_no_merge_introduced_conflict_markers(
        ctx,
        worktree,
        base_head,
        incoming_base_head,
        Some(&final_head),
        observer,
    )?;

    let ancestry = git_output(
        ctx,
        worktree,
        [
            "merge-base",
            "--is-ancestor",
            base_head.trim(),
            final_head.as_str(),
        ],
        observer,
    )?;
    if !ancestry.status.success() {
        return Err(PrRepairStepError::failed(git_error(
            "PR repair head does not descend from the observed head",
            ancestry,
        ))
        .into());
    }

    let remote_ref = remote_branch_ref(head_ref);
    let expected_remote_head = base_head.trim();
    let lease = format!("--force-with-lease={remote_ref}:{expected_remote_head}");
    let push_ref = format!("HEAD:{remote_ref}");
    let push_args = ["push", &lease, "origin", &push_ref];
    let push_result = git_execution_output(worktree, push_args, ctx.command_timeout(), observer);
    let push_error = match push_result {
        Ok(push) if push.status.success() => None,
        Ok(push) => Some(PrPushError::Ambiguous {
            error: git_error("git push with expected-head lease failed", push),
            final_head: final_head.clone(),
        }),
        Err(error) => Some(pr_push_execution_error(error, &final_head)),
    };
    if let Some(push_error) = push_error {
        let reconciliation = reconcile_remote_push(ctx, worktree, head_ref, &final_head);
        if reconciliation.confirmed {
            return Ok(push_result_value(
                base_head,
                &final_head,
                Some(reconciliation.detail),
            ));
        }
        return Err(match push_error {
            PrPushError::Step(PrRepairStepError::Cancelled(detail)) => {
                PrPushError::Step(PrRepairStepError::Cancelled(format!(
                    "{detail}; push outcome was not confirmed: {}",
                    reconciliation.detail
                )))
            }
            PrPushError::Step(PrRepairStepError::Failed(error)) => {
                PrPushError::Step(PrRepairStepError::Failed(error.context(format!(
                    "push outcome was not confirmed: {}",
                    reconciliation.detail
                ))))
            }
            PrPushError::Ambiguous { error, final_head } => PrPushError::Ambiguous {
                error: error.context(format!(
                    "push outcome was not confirmed: {}",
                    reconciliation.detail
                )),
                final_head,
            },
        });
    }

    Ok(push_result_value(base_head, &final_head, None))
}

pub(super) fn pr_push_execution_error(
    error: ExecutionCommandError,
    final_head: &str,
) -> PrPushError {
    match error {
        ExecutionCommandError::CancelledBeforeStart => PrPushError::Step(
            PrRepairStepError::Cancelled("git push was cancelled before it started".into()),
        ),
        ExecutionCommandError::Cancelled => PrPushError::Ambiguous {
            error: anyhow!("git push was cancelled while it was running"),
            final_head: final_head.to_string(),
        },
        ExecutionCommandError::Failed {
            error,
            process_started: true,
        } => PrPushError::Ambiguous {
            error,
            final_head: final_head.to_string(),
        },
        ExecutionCommandError::Failed {
            error,
            process_started: false,
        } => PrPushError::Step(PrRepairStepError::Failed(error)),
    }
}

pub(super) fn push_result_value(
    base_head: &str,
    final_head: &str,
    reconciliation: Option<String>,
) -> Value {
    let mut value = json!({
        "status": "pushed",
        "pushed": true,
        "base_head": base_head.trim(),
        "final_head": final_head.trim(),
        "force": true,
        "force_with_lease": true,
        "expected_remote_head": base_head.trim(),
    });
    if let Some(reconciliation) = reconciliation {
        value["reconciliation"] = Value::String(reconciliation);
    }
    value
}

struct PushReconciliation {
    confirmed: bool,
    detail: String,
}

fn reconcile_remote_push(
    ctx: &RepoContext,
    worktree: &Path,
    head_ref: &str,
    final_head: &str,
) -> PushReconciliation {
    let remote_ref = format!("refs/heads/{head_ref}");
    let mut observer = NoopExecutionObserver;
    let timeout_seconds = ctx.command_timeout().as_secs().min(30);
    let timeout = CommandTimeout::from_seconds(timeout_seconds)
        .expect("the reconciliation timeout is nonzero and within the command timeout range");
    let output = git_execution_output(
        worktree,
        ["ls-remote", "--exit-code", "origin", &remote_ref],
        timeout,
        &mut observer,
    );
    match output {
        Ok(output) if output.status.success() => {
            let observed = remote_head_from_ls_remote(&output.stdout, &remote_ref);
            PushReconciliation {
                confirmed: observed == Some(final_head),
                detail: match observed {
                    Some(observed) if observed == final_head => {
                        format!("remote {remote_ref} confirmed at {observed}")
                    }
                    Some(observed) => {
                        format!("remote {remote_ref} resolved to {observed}; expected {final_head}")
                    }
                    None => format!("remote {remote_ref} returned no matching head"),
                },
            }
        }
        Ok(output) => PushReconciliation {
            confirmed: false,
            detail: format!(
                "remote {remote_ref} reconciliation exited with status {}",
                output.status.code().unwrap_or(1)
            ),
        },
        Err(error) => PushReconciliation {
            confirmed: false,
            detail: format!("remote {remote_ref} reconciliation failed: {error}"),
        },
    }
}

pub(super) fn remote_head_from_ls_remote<'a>(
    stdout: &'a [u8],
    remote_ref: &str,
) -> Option<&'a str> {
    std::str::from_utf8(stdout)
        .ok()?
        .lines()
        .filter_map(|line| line.split_once(char::is_whitespace))
        .find_map(|(head, reference)| (reference.trim() == remote_ref).then_some(head.trim()))
}
