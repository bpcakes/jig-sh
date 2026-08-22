use std::ffi::OsString;
use std::process::Output;

use anyhow::Result;
use serde_json::Value;

use crate::context::{RepoContext, ReviewScopeArg, WorkReviewGate, parse_review_scope_arg};
use crate::runtime::worker_runner::{
    CodexExecMode, CodexExecRequest, CodexPrompt, WorkerReceiptRequest, run_codex_exec,
};

pub(super) struct CodexReviewCommandOutput {
    pub(super) output: Output,
    pub(super) codex_stdout: String,
    pub(super) worker_receipt_id: String,
}

pub(super) fn run_codex_review(
    ctx: &RepoContext,
    plan_id: &str,
    gate: &WorkReviewGate,
    prompt: &str,
    schema: &Value,
) -> Result<CodexReviewCommandOutput> {
    let output = run_codex_exec(
        ctx,
        CodexExecRequest {
            root: ctx.root(),
            codex_home: None,
            mode: CodexExecMode::Review,
            model: gate.model.as_deref(),
            approval_policy: None,
            sandbox: None,
            ephemeral: true,
            extra_args: review_scope_args(gate)?,
            output_schema: Some(schema),
            prompt: CodexPrompt::Argument(prompt),
            cancelled: None,
            receipt: WorkerReceiptRequest {
                purpose: "work_review",
                plan_id: Some(plan_id),
                workflow_id: None,
                item_key: Some(&gate.id),
                collect_git_metadata: true,
                collect_worktree_fingerprint: true,
            },
        },
    )?;
    Ok(CodexReviewCommandOutput {
        output: output.output,
        codex_stdout: output.provider_stdout,
        worker_receipt_id: output.worker_receipt_id,
    })
}

pub(super) fn run_codex_refine(
    ctx: &RepoContext,
    plan_id: &str,
    prompt: &str,
    model: Option<&str>,
) -> Result<CodexRefineCommandOutput> {
    let output = run_codex_exec(
        ctx,
        CodexExecRequest {
            root: ctx.root(),
            codex_home: None,
            mode: CodexExecMode::Exec,
            model,
            approval_policy: Some("never"),
            sandbox: Some("workspace-write"),
            ephemeral: true,
            extra_args: Vec::new(),
            output_schema: None,
            prompt: CodexPrompt::Stdin(prompt),
            cancelled: None,
            receipt: WorkerReceiptRequest {
                purpose: "work_refine",
                plan_id: Some(plan_id),
                workflow_id: None,
                item_key: None,
                collect_git_metadata: true,
                collect_worktree_fingerprint: true,
            },
        },
    )?;
    Ok(CodexRefineCommandOutput {
        output: output.output,
        worker_receipt_id: output.worker_receipt_id,
    })
}

pub(super) struct CodexRefineCommandOutput {
    pub(super) output: Output,
    pub(super) worker_receipt_id: String,
}

fn review_scope_args(gate: &WorkReviewGate) -> Result<Vec<OsString>> {
    Ok(match parse_review_scope_arg(&gate.scope)? {
        ReviewScopeArg::Uncommitted => vec![OsString::from("--uncommitted")],
        ReviewScopeArg::Base(base) => vec![OsString::from("--base"), OsString::from(base)],
        ReviewScopeArg::Commit(commit) => vec![OsString::from("--commit"), OsString::from(commit)],
    })
}
