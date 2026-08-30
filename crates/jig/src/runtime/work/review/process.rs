use std::ffi::OsString;
use std::process::Output;

use anyhow::Result;
use jig_owned_process::ProcessOutputOverflowPolicy;
use serde_json::Value;

use crate::context::{RepoContext, ReviewScopeArg, WorkReviewGate, parse_review_scope_arg};
use crate::execution::{ExecutionControl, PhasePosition};
use crate::runtime::worker_runner::{
    CodexExecMode, CodexExecRequest, CodexPrompt, WorkerPhase, WorkerReceiptRequest, run_codex_exec,
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
    position: PhasePosition,
    observer: &mut dyn ExecutionControl,
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
            transcript_overflow_policy: ProcessOutputOverflowPolicy::Truncate,
            prompt: CodexPrompt::Argument(prompt),
            receipt: WorkerReceiptRequest {
                purpose: "work_review",
                plan_id: Some(plan_id),
                workflow_id: None,
                item_key: Some(&gate.id),
                collect_git_metadata: true,
                collect_worktree_fingerprint: true,
            },
            receipt_journal: None,
            phase: Some(WorkerPhase {
                label: &gate.id,
                position,
            }),
        },
        observer,
    )?
    .into_completed()?;
    let codex_stdout = output.provider_stdout().to_owned();
    let worker_receipt_id = output.worker_receipt_id().to_owned();
    Ok(CodexReviewCommandOutput {
        output: output.into_process_output(),
        codex_stdout,
        worker_receipt_id,
    })
}

pub(super) fn run_codex_refine(
    ctx: &RepoContext,
    plan_id: &str,
    prompt: &str,
    model: Option<&str>,
    phase_label: &str,
    position: PhasePosition,
    observer: &mut dyn ExecutionControl,
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
            transcript_overflow_policy: ProcessOutputOverflowPolicy::Truncate,
            prompt: CodexPrompt::Stdin(prompt),
            receipt: WorkerReceiptRequest {
                purpose: "work_refine",
                plan_id: Some(plan_id),
                workflow_id: None,
                item_key: None,
                collect_git_metadata: true,
                collect_worktree_fingerprint: true,
            },
            receipt_journal: None,
            phase: Some(WorkerPhase {
                label: phase_label,
                position,
            }),
        },
        observer,
    )?
    .into_completed()?;
    let worker_receipt_id = output.worker_receipt_id().to_owned();
    Ok(CodexRefineCommandOutput {
        output: output.into_process_output(),
        worker_receipt_id,
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
