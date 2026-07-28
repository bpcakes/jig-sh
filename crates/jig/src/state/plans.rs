use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cancellation::ensure_status_collection_active;
use crate::context::RepoContext;
use crate::tool_defs::{args, tool};

use super::events::{
    PlanEvent, append_jsonl, append_text, ensure_state_layout, new_id, now_ms, read_jsonl,
    read_jsonl_with_cancellation, rel_path,
};
use super::receipts::{StateToolReceipt, record_successful_state_tool};

#[derive(Debug, Deserialize)]
pub(crate) struct PlanOpenRequest {
    pub(crate) title: String,
    pub(crate) body: Option<String>,
    pub(crate) body_file: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PlanAppendRequest {
    pub(crate) plan_id: String,
    pub(crate) body: Option<String>,
    pub(crate) body_file: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PlanCloseRequest {
    pub(crate) plan_id: String,
    pub(crate) resolution: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlanStatus {
    Open,
    Closed,
}

pub(crate) fn plans_open(ctx: &RepoContext, request: PlanOpenRequest) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let plan_id = new_id("plan");
    let body = plan_body(request.body, request.body_file)?;
    let plan_path = ctx.plan_body_path(&plan_id);
    if let Some(parent) = plan_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&plan_path, body)?;

    let event = PlanEvent::open(
        new_id("plan-event"),
        plan_id.clone(),
        now_ms(),
        request.title.clone(),
        Some(rel_path(ctx.root(), &plan_path)?),
    );
    append_jsonl(&ctx.state_file("plans.jsonl"), &event)?;

    let receipt_id = record_successful_state_tool(
        ctx,
        StateToolReceipt {
            tool_name: tool::PLANS_OPEN,
            args: json!({
                args::OPERATION: "plan_open",
                "title": request.title,
            }),
            started_at_ms: event.timestamp_ms(),
            plan_id: Some(plan_id.clone()),
            session_override: None,
        },
    )?;

    Ok(json!({
        "ok": true,
        "plan_id": plan_id,
        "body_path": event.body_path(),
        "receipt_id": receipt_id,
    }))
}

pub(crate) fn plans_append(ctx: &RepoContext, request: PlanAppendRequest) -> Result<Value> {
    ensure_state_layout(ctx)?;
    ensure_plan_is_open(ctx, &request.plan_id)?;
    let body = plan_body(request.body, request.body_file)?;
    let plan_path = ctx.plan_body_path(&request.plan_id);
    append_text(&plan_path, format!("\n\n{body}").as_bytes())?;

    let event = PlanEvent::append(
        new_id("plan-event"),
        request.plan_id.clone(),
        now_ms(),
        Some(rel_path(ctx.root(), &plan_path)?),
    );
    append_jsonl(&ctx.state_file("plans.jsonl"), &event)?;

    let receipt_id = record_successful_state_tool(
        ctx,
        StateToolReceipt {
            tool_name: tool::PLANS_APPEND,
            args: json!({
                args::OPERATION: "plan_append",
                "plan_id": request.plan_id,
            }),
            started_at_ms: event.timestamp_ms(),
            plan_id: Some(event.plan_id().to_string()),
            session_override: None,
        },
    )?;

    Ok(json!({
        "ok": true,
        "plan_id": event.plan_id(),
        "receipt_id": receipt_id,
    }))
}

pub(crate) fn plans_close(ctx: &RepoContext, request: PlanCloseRequest) -> Result<Value> {
    ensure_state_layout(ctx)?;
    ensure_plan_is_open(ctx, &request.plan_id)?;

    let event = PlanEvent::close(
        new_id("plan-event"),
        request.plan_id.clone(),
        now_ms(),
        request.resolution.clone(),
    );
    append_jsonl(&ctx.state_file("plans.jsonl"), &event)?;

    let receipt_id = record_successful_state_tool(
        ctx,
        StateToolReceipt {
            tool_name: tool::PLANS_CLOSE,
            args: json!({
                args::OPERATION: "plan_close",
                "plan_id": request.plan_id,
                "resolution": request.resolution,
            }),
            started_at_ms: event.timestamp_ms(),
            plan_id: Some(event.plan_id().to_string()),
            session_override: None,
        },
    )?;

    Ok(json!({
        "ok": true,
        "plan_id": event.plan_id(),
        "receipt_id": receipt_id,
    }))
}

pub(crate) fn ensure_plan_is_open(ctx: &RepoContext, plan_id: &str) -> Result<()> {
    match plan_status(ctx, plan_id)? {
        Some(PlanStatus::Open) => Ok(()),
        Some(PlanStatus::Closed) => bail!("Plan is already closed: {plan_id}"),
        None => bail!("Plan not found: {plan_id}"),
    }
}

pub(crate) fn ensure_plan_exists(ctx: &RepoContext, plan_id: &str) -> Result<()> {
    match plan_status(ctx, plan_id)? {
        Some(_) => Ok(()),
        None => bail!("Plan not found: {plan_id}"),
    }
}

pub(crate) fn ensure_plan_exists_with_cancellation(
    ctx: &RepoContext,
    plan_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    match plan_status_with_cancellation(ctx, plan_id, cancelled)? {
        Some(_) => Ok(()),
        None => bail!("Plan not found: {plan_id}"),
    }
}

pub(crate) fn plan_status(ctx: &RepoContext, plan_id: &str) -> Result<Option<PlanStatus>> {
    ensure_state_layout(ctx)?;
    let events = read_jsonl::<PlanEvent>(&ctx.state_file("plans.jsonl"))?;
    Ok(plan_status_from_events(&events, plan_id))
}

pub(crate) fn plan_status_with_cancellation(
    ctx: &RepoContext,
    plan_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<PlanStatus>> {
    ensure_plan_scan_active(cancelled)?;
    ensure_state_layout(ctx)?;
    let events =
        read_jsonl_with_cancellation::<PlanEvent>(&ctx.state_file("plans.jsonl"), cancelled)?;
    let mut opened = false;
    let mut closed = false;
    for event in &events {
        ensure_plan_scan_active(cancelled)?;
        if event.plan_id() != plan_id {
            continue;
        }
        match event {
            PlanEvent::Open { .. } => {
                opened = true;
                closed = false;
            }
            PlanEvent::Close { .. } => closed = true,
            _ => {}
        }
    }
    ensure_plan_scan_active(cancelled)?;
    Ok(match (opened, closed) {
        (true, false) => Some(PlanStatus::Open),
        (true, true) => Some(PlanStatus::Closed),
        (false, _) => None,
    })
}

pub(crate) fn open_plan_summaries(ctx: &RepoContext) -> Result<Vec<Value>> {
    ensure_state_layout(ctx)?;
    let events = read_jsonl::<PlanEvent>(&ctx.state_file("plans.jsonl"))?;
    Ok(open_plans(&events))
}

pub(crate) fn open_plan_summaries_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<Value>> {
    ensure_plan_scan_active(cancelled)?;
    ensure_state_layout(ctx)?;
    let events =
        read_jsonl_with_cancellation::<PlanEvent>(&ctx.state_file("plans.jsonl"), cancelled)?;
    let mut closed = HashSet::new();
    let mut opened = BTreeMap::<String, (&str, Option<&str>)>::new();
    for event in &events {
        ensure_plan_scan_active(cancelled)?;
        match event {
            PlanEvent::Open {
                plan_id,
                title,
                body_path,
                ..
            } => {
                opened.insert(plan_id.clone(), (title.as_str(), body_path.as_deref()));
            }
            PlanEvent::Close { plan_id, .. } => {
                closed.insert(plan_id.clone());
            }
            _ => {}
        }
    }
    ensure_plan_scan_active(cancelled)?;
    Ok(opened
        .into_iter()
        .filter(|(plan_id, _)| !closed.contains(plan_id))
        .map(|(plan_id, (title, body_path))| {
            json!({
                "plan_id": plan_id,
                "title": title,
                "body_path": body_path,
            })
        })
        .collect())
}

fn ensure_plan_scan_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
}

#[cfg(test)]
pub(crate) fn seed_open_plan_for_test(
    ctx: &RepoContext,
    plan_id: &str,
    title: &str,
    body: &str,
) -> Result<()> {
    ensure_state_layout(ctx)?;
    let plan_path = ctx.plan_body_path(plan_id);
    if let Some(parent) = plan_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&plan_path, body)?;
    let event = PlanEvent::open(
        new_id("plan-event"),
        plan_id.to_string(),
        now_ms(),
        title.to_string(),
        Some(rel_path(ctx.root(), &plan_path)?),
    );
    append_jsonl(&ctx.state_file("plans.jsonl"), &event)
}

pub(super) fn open_plans(events: &[PlanEvent]) -> Vec<Value> {
    let mut closed = HashSet::new();
    let mut opened = BTreeMap::<String, (&str, Option<&str>)>::new();
    for event in events {
        match event {
            PlanEvent::Open {
                plan_id,
                title,
                body_path,
                ..
            } => {
                opened.insert(plan_id.clone(), (title.as_str(), body_path.as_deref()));
            }
            PlanEvent::Close { plan_id, .. } => {
                closed.insert(plan_id.clone());
            }
            _ => {}
        }
    }

    opened
        .into_iter()
        .filter(|(plan_id, _)| !closed.contains(plan_id))
        .map(|(plan_id, (title, body_path))| {
            json!({
                "plan_id": plan_id,
                "title": title,
                "body_path": body_path,
            })
        })
        .collect()
}

fn plan_status_from_events(events: &[PlanEvent], plan_id: &str) -> Option<PlanStatus> {
    let mut opened = false;
    let mut closed = false;

    for event in events.iter().filter(|event| event.plan_id() == plan_id) {
        match event {
            PlanEvent::Open { .. } => {
                opened = true;
                closed = false;
            }
            PlanEvent::Close { .. } => closed = true,
            _ => {}
        }
    }

    match (opened, closed) {
        (true, false) => Some(PlanStatus::Open),
        (true, true) => Some(PlanStatus::Closed),
        (false, _) => None,
    }
}

fn plan_body(body: Option<String>, body_file: Option<PathBuf>) -> Result<String> {
    match (body, body_file) {
        (Some(text), None) => Ok(text),
        (None, Some(path)) => fs::read_to_string(path).context("Failed to read plan body file"),
        (None, None) => Ok(String::from("# Plan\n")),
        (Some(_), Some(_)) => bail!("Provide either --body or --body-file, not both."),
    }
}
