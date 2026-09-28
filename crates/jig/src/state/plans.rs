#[cfg(test)]
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{self, File, OpenOptions};

use anyhow::{Context, Result, bail};
use fs4::fs_std::FileExt;
use serde_json::{Value, json};

use crate::cancellation::ensure_status_collection_active;
use crate::context::RepoContext;

use super::jsonl::{read_dashboard_jsonl, read_jsonl};
use super::plan_files::validate_plan_id;
use super::records::{PlanBaseline, PlanEvent};
use super::support::{AdvisoryLeaseFile, ensure_state_layout};

#[cfg(test)]
mod test_support;
#[cfg(test)]
pub(crate) use test_support::{PlanOpenRequest, plans_open, seed_open_plan_for_test};

const PLAN_EXECUTION_LEASE_DIR: &str = ".agent/.cache/plan-execution-leases";

#[cfg(test)]
thread_local! {
    static PLAN_BASELINE_SCAN_COUNT: Cell<usize> = const { Cell::new(0) };
}

pub(super) struct ActivePlanRunLease {
    _file: AdvisoryLeaseFile,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlanStatus {
    Open,
    Closed,
}

pub(super) fn acquire_active_plan_run_lease(
    ctx: &RepoContext,
    plan_id: &str,
) -> Result<ActivePlanRunLease> {
    let file = open_plan_execution_lease(ctx, plan_id)?;
    FileExt::lock_shared(&file)
        .with_context(|| format!("Failed to acquire execution lease for work plan '{plan_id}'"))?;
    // Finish holds this lease exclusively across its final open-state check
    // and close append. Acquiring shared first and checking second prevents a
    // run from starting after the plan closes.
    ensure_plan_is_open(ctx, plan_id)?;
    Ok(ActivePlanRunLease {
        _file: AdvisoryLeaseFile::new(file),
    })
}

fn open_plan_execution_lease(ctx: &RepoContext, plan_id: &str) -> Result<File> {
    validate_plan_id(plan_id)
        .context("plan id cannot be used as a safe execution lease filename")?;
    ensure_state_layout(ctx)?;
    let lease_dir = ctx.root().join(PLAN_EXECUTION_LEASE_DIR);
    fs::create_dir_all(&lease_dir)
        .with_context(|| format!("Failed to create {}", lease_dir.display()))?;
    let path = lease_dir.join(format!("{plan_id}.lock"));
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| {
            format!(
                "Failed to open work plan execution lease {}",
                path.display()
            )
        })
}

pub(crate) fn ensure_plan_is_open(ctx: &RepoContext, plan_id: &str) -> Result<()> {
    match plan_status(ctx, plan_id)? {
        Some(PlanStatus::Open) => Ok(()),
        Some(PlanStatus::Closed) => bail!("Plan is already closed: {plan_id}"),
        None => bail!("Plan not found: {plan_id}"),
    }
}

pub(crate) fn plan_status(ctx: &RepoContext, plan_id: &str) -> Result<Option<PlanStatus>> {
    let events = read_jsonl::<PlanEvent>(&ctx.state_file("plans.jsonl"))?;
    Ok(plan_status_from_events(&events, plan_id))
}

pub(crate) fn open_plan_summaries(ctx: &RepoContext) -> Result<Vec<Value>> {
    let events = read_jsonl::<PlanEvent>(&ctx.state_file("plans.jsonl"))?;
    Ok(open_plans(&events))
}

pub(crate) fn plan_baseline(ctx: &RepoContext, plan_id: &str) -> Result<Option<PlanBaseline>> {
    #[cfg(test)]
    PLAN_BASELINE_SCAN_COUNT.set(PLAN_BASELINE_SCAN_COUNT.get() + 1);
    let events = read_jsonl::<PlanEvent>(&ctx.state_file("plans.jsonl"))?;
    unique_plan_baselines(&events, &BTreeSet::from([plan_id.to_string()]))?
        .remove(plan_id)
        .ok_or_else(|| anyhow::anyhow!("Plan baseline resolver omitted requested plan {plan_id}"))
}

pub(crate) fn plan_baseline_with_cancellation(
    ctx: &RepoContext,
    plan_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<PlanBaseline>> {
    #[cfg(test)]
    PLAN_BASELINE_SCAN_COUNT.set(PLAN_BASELINE_SCAN_COUNT.get() + 1);
    ensure_plan_scan_active(cancelled)?;
    let events = read_dashboard_jsonl::<PlanEvent>(&ctx.state_file("plans.jsonl"), cancelled)?;
    ensure_plan_scan_active(cancelled)?;
    unique_plan_baselines(&events, &BTreeSet::from([plan_id.to_string()]))?
        .remove(plan_id)
        .ok_or_else(|| anyhow::anyhow!("Plan baseline resolver omitted requested plan {plan_id}"))
}

fn unique_plan_baselines(
    events: &[PlanEvent],
    plan_ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, Option<PlanBaseline>>> {
    let mut baselines = plan_ids
        .iter()
        .cloned()
        .map(|plan_id| (plan_id, None))
        .collect::<BTreeMap<_, _>>();
    let mut opened = BTreeSet::new();
    for event in events {
        let PlanEvent::Open {
            plan_id, baseline, ..
        } = event
        else {
            continue;
        };
        let Some(slot) = baselines.get_mut(plan_id) else {
            continue;
        };
        if !opened.insert(plan_id.clone()) {
            bail!(
                "Plan {plan_id} has multiple Open records; repair the append-only plan stream before collecting gate evidence"
            );
        }
        slot.clone_from(baseline);
    }
    Ok(baselines)
}

fn ensure_plan_scan_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
}

pub(super) fn open_plans(events: &[PlanEvent]) -> Vec<Value> {
    let mut closed = HashSet::new();
    let mut opened = BTreeMap::<String, (&str, Option<&str>, Option<&PlanBaseline>)>::new();
    for event in events {
        match event {
            PlanEvent::Open {
                plan_id,
                title,
                body_path,
                baseline,
                ..
            } => {
                opened.insert(
                    plan_id.clone(),
                    (title.as_str(), body_path.as_deref(), baseline.as_ref()),
                );
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
        .map(|(plan_id, (title, body_path, baseline))| {
            json!({
                "plan_id": plan_id,
                "title": title,
                "body_path": body_path,
                "baseline": baseline,
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
