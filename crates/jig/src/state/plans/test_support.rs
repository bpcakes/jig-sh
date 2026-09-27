//! Test fixtures that record plan-open events the way the removed
//! `jig work start` did, for tests of the remaining plan-state readers.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::git_receipts::{resolve_empty_tree_for_unborn_repository, resolve_git_commit};
use crate::tool_defs::tool;

use super::super::jsonl::append_jsonl;
use super::super::plan_files::create_plan_body;
use super::super::receipts::{StateToolReceipt, record_successful_state_tool};
use super::super::records::{PlanBaseline, PlanEvent};
use super::super::support::{new_id, now_ms, rel_path};

#[derive(Debug, Deserialize)]
pub(crate) struct PlanOpenRequest {
    pub(crate) title: String,
    pub(crate) body: Option<String>,
    pub(crate) body_file: Option<PathBuf>,
    pub(crate) base: Option<String>,
}

pub(crate) struct PreparedPlanOpen {
    title: String,
    body: String,
    baseline: PlanBaseline,
}

pub(crate) fn plans_open(ctx: &RepoContext, request: PlanOpenRequest) -> Result<Value> {
    plans_open_prepared(ctx, prepare_plan_open(ctx, request)?, None)
}

pub(crate) fn prepare_plan_open(
    ctx: &RepoContext,
    request: PlanOpenRequest,
) -> Result<PreparedPlanOpen> {
    let baseline = plan_baseline_for_open(ctx, request.base.as_deref())?;
    Ok(PreparedPlanOpen {
        title: request.title,
        body: plan_open_body(request.body, request.body_file)?,
        baseline,
    })
}

pub(crate) fn plans_open_prepared(
    ctx: &RepoContext,
    request: PreparedPlanOpen,
    owner_session_id: Option<String>,
) -> Result<Value> {
    let plan_id = new_id("plan");
    let plan_path = create_plan_body(ctx, &plan_id, &request.body)?;

    let event = PlanEvent::open_with_baseline(
        new_id("plan-event"),
        plan_id.clone(),
        now_ms(),
        request.title.clone(),
        Some(rel_path(ctx.root(), &plan_path)?),
        request.baseline.clone(),
    );
    append_jsonl(&ctx.state_file("plans.jsonl"), &event)?;

    let receipt_id = record_successful_state_tool(
        ctx,
        StateToolReceipt {
            tool_name: tool::PLANS_OPEN,
            args: json!({
                "operation": "plan_open",
                "title": request.title,
                "baseline": event.baseline(),
            }),
            started_at_ms: event.timestamp_ms(),
            plan_id: Some(plan_id.clone()),
            // A work start already knows the session it created. Never infer
            // this ownership edge from the mutable repository-global pointer:
            // another concurrent start may have replaced it by the time the
            // plan-open receipt is appended.
            session_override: owner_session_id,
        },
    )?;

    Ok(json!({
        "ok": true,
        "plan_id": plan_id,
        "body_path": event.body_path(),
        "baseline": event.baseline(),
        "receipt_id": receipt_id,
    }))
}

fn plan_baseline_for_open(ctx: &RepoContext, requested: Option<&str>) -> Result<PlanBaseline> {
    let reference = requested.unwrap_or("HEAD").trim();
    if reference.is_empty() {
        bail!("Plan baseline ref must not be blank");
    }
    match resolve_git_commit(ctx.root(), reference) {
        Ok(commit_oid) => Ok(PlanBaseline {
            requested_ref: reference.to_string(),
            commit_oid: Some(commit_oid),
            empty_tree_oid: None,
            error: None,
        }),
        Err(error) if requested.is_some() => Err(error)
            .with_context(|| format!("Failed to resolve explicit plan baseline ref '{reference}'")),
        Err(error) => match resolve_empty_tree_for_unborn_repository(ctx.root()) {
            Ok(Some(empty_tree_oid)) => Ok(PlanBaseline {
                requested_ref: reference.to_string(),
                commit_oid: None,
                empty_tree_oid: Some(empty_tree_oid),
                error: None,
            }),
            Ok(None) | Err(_) => Ok(PlanBaseline {
                requested_ref: reference.to_string(),
                commit_oid: None,
                empty_tree_oid: None,
                error: Some(format!("{error:#}")),
            }),
        },
    }
}

pub(crate) fn seed_open_plan_for_test(
    ctx: &RepoContext,
    plan_id: &str,
    title: &str,
    body: &str,
) -> Result<()> {
    let plan_path = create_plan_body(ctx, plan_id, body)?;
    let event = PlanEvent::open(
        new_id("plan-event"),
        plan_id.to_string(),
        now_ms(),
        title.to_string(),
        Some(rel_path(ctx.root(), &plan_path)?),
    );
    append_jsonl(&ctx.state_file("plans.jsonl"), &event)
}

fn plan_open_body(body: Option<String>, body_file: Option<PathBuf>) -> Result<String> {
    match (body, body_file) {
        (Some(text), None) => Ok(text),
        (None, Some(path)) => fs::read_to_string(path).context("Failed to read plan body file"),
        (None, None) => Ok(String::from("# Plan\n")),
        (Some(_), Some(_)) => bail!("Provide either `body` or `body_file`, not both."),
    }
}

/// Records a plan-close event the way the removed `jig work finish` did.
pub(crate) fn seed_closed_plan_for_test(
    ctx: &RepoContext,
    plan_id: &str,
    resolution: &str,
) -> Result<()> {
    let event = PlanEvent::close(
        new_id("plan-event"),
        plan_id.to_string(),
        now_ms(),
        Some(resolution.to_string()),
    );
    append_jsonl(&ctx.state_file("plans.jsonl"), &event)
}
