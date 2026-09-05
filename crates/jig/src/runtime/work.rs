use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::command::{
    WorkAppendRequest, WorkCheckRequest, WorkCommand, WorkDecisionRequest, WorkEvidenceRequest,
    WorkFinishRequest, WorkGatesRequest, WorkReceiptsRequest, WorkRefineRequest, WorkReviewRequest,
    WorkStartRequest,
};
use crate::context::RepoContext;
use crate::execution::ExecutionControl;
use crate::state::{
    DecisionAddRequest, PlanAppendRequest, PlanCloseRequest, PlanOpenRequest, ReceiptListFilter,
    SessionEndRequest, current_session, decisions_add, plans_append, plans_close,
    plans_open_prepared, prepare_plan_open, receipts_list, session_end, session_start,
    state_summary_with_cancellation,
};

mod checks;
#[cfg(test)]
pub(in crate::runtime) use checks::check_tools_collect_failures_with_observer;
mod gates;
mod goal;
mod review;
mod scope;
mod tools;

impl From<WorkStartRequest> for PlanOpenRequest {
    fn from(request: WorkStartRequest) -> Self {
        Self {
            title: request.title,
            body: request.body,
            body_file: request.body_file,
            base: request.base,
        }
    }
}

impl From<WorkAppendRequest> for PlanAppendRequest {
    fn from(request: WorkAppendRequest) -> Self {
        Self {
            plan_id: request.plan_id,
            body: request.body,
            body_file: request.body_file,
        }
    }
}

impl From<WorkDecisionRequest> for DecisionAddRequest {
    fn from(request: WorkDecisionRequest) -> Self {
        Self {
            title: request.title,
            selected_option: request.selected_option,
            rationale: request.rationale,
            alternatives: request.alternatives,
            plan_id: request.plan_id,
        }
    }
}

impl From<WorkReceiptsRequest> for ReceiptListFilter {
    fn from(request: WorkReceiptsRequest) -> Self {
        Self {
            session_id: request.session_id,
            plan_id: request.plan_id,
            tool_name: request.tool_name,
            failed_only: request.failed_only,
            limit: request.limit,
        }
    }
}

impl From<&WorkFinishRequest> for PlanCloseRequest {
    fn from(request: &WorkFinishRequest) -> Self {
        Self {
            plan_id: request.plan_id.clone(),
            resolution: request.resolution.clone(),
        }
    }
}

pub(super) fn dispatch_with_observer(
    ctx: &RepoContext,
    command: WorkCommand,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    match command {
        WorkCommand::Goal(opts) => goal::goal(ctx, opts),
        WorkCommand::Start(opts) => start(ctx, opts.into()),
        WorkCommand::Append(opts) => plans_append(ctx, opts.into()),
        WorkCommand::Check(opts) => checks::check_with_observer(ctx, opts, observer),
        WorkCommand::Gates(opts) => {
            gates::snapshot_with_cancellation(ctx, opts.plan_id, &|| observer.cancelled())
        }
        WorkCommand::Evidence(opts) => {
            gates::evidence_with_cancellation(ctx, opts, &|| observer.cancelled())
        }
        WorkCommand::Review(opts) => review::review_with_observer(ctx, opts, observer),
        WorkCommand::Refine(opts) => review::refine_with_observer(ctx, opts, observer),
        WorkCommand::Decide(opts) => decisions_add(ctx, opts.into()),
        WorkCommand::Receipts(opts) => receipts_list(ctx, opts.into()),
        WorkCommand::Status => {
            state_summary_with_cancellation(ctx, &|| observer.cancelled()).map(|mut value| {
                value["command"] = json!("work status");
                value
            })
        }
        WorkCommand::Finish(opts) => finish_with_cancellation(ctx, opts, &|| observer.cancelled()),
    }
}

pub(super) fn open_plan_gate_snapshots_with_cancellation(
    ctx: &RepoContext,
    plan_ids: &[String],
    cancelled: &dyn Fn() -> bool,
) -> Result<std::collections::BTreeMap<String, Value>> {
    gates::open_plan_snapshots_with_cancellation(ctx, plan_ids, cancelled)
}

pub(crate) use gates::{
    DashboardGateReport, dashboard_gate_receipt_indexes,
    dashboard_open_plan_reports_with_cancellation,
};

pub(super) fn start(ctx: &RepoContext, plan: PlanOpenRequest) -> Result<Value> {
    // Resolve and validate all caller-controlled plan input before starting a
    // durable session. CLI parsing catches common conflicts, while this keeps
    // MCP and other runtime callers from leaving an orphan session on failure.
    let plan = prepare_plan_open(ctx, plan)?;
    let session = session_start(ctx)?;
    let plan = plans_open_prepared(ctx, plan)?;

    Ok(json!({
        "ok": true,
        "session": session,
        "plan": plan,
    }))
}

pub(super) fn finish(ctx: &RepoContext, opts: WorkFinishRequest) -> Result<Value> {
    finish_with_cancellation(ctx, opts, &|| false)
}

pub(in crate::runtime) fn finish_with_cancellation(
    ctx: &RepoContext,
    opts: WorkFinishRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    // Check before gate evaluation so unknown or already-closed plans report
    // plan-state errors instead of misleading gate failures. plans_close
    // rechecks after gates to preserve the state-layer invariant.
    crate::state::ensure_plan_is_open(ctx, &opts.plan_id)?;
    // Gate evidence and the source fingerprint it authenticates are a
    // read-only view of the checkout. Retain a shared lease through the plan
    // close commit point so an unrelated effectful run cannot make that view
    // stale between the final fingerprint check and durable closure.
    let _repository_execution = crate::state::acquire_repository_execution_lease(
        ctx,
        &[jig_contract::ActionEffect::ReadOnly],
    )?;
    let evaluated_worktree_fingerprint =
        gates::ensure_required_gates_passed_with_cancellation(ctx, &opts.plan_id, cancelled)?;
    finish_after_required_gates_passed(ctx, opts, evaluated_worktree_fingerprint, cancelled)
}

pub(in crate::runtime) fn finish_after_required_gates_passed(
    ctx: &RepoContext,
    opts: WorkFinishRequest,
    evaluated_worktree_fingerprint: Option<String>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    ensure_finish_authority_is_current(ctx, evaluated_worktree_fingerprint.as_deref(), cancelled)?;

    let plan = plans_close(ctx, (&opts).into())?;
    let session = match current_session(ctx)? {
        Some(_) => Some(session_end(
            ctx,
            session_end_request_for_finish(opts.outcome.or(opts.resolution)),
        )?),
        None => None,
    };

    Ok(json!({
        "ok": true,
        "plan": plan,
        "session": session,
    }))
}

fn ensure_finish_authority_is_current(
    ctx: &RepoContext,
    evaluated_worktree_fingerprint: Option<&str>,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    crate::cancellation::ensure_status_collection_active(cancelled)?;
    let current = RepoContext::load_from_root(ctx.root().to_path_buf())
        .context("Failed to reload repository authority before closing the work plan")?;
    ensure_finish_config_is_current(ctx, &current)?;
    if let Some(evaluated) = evaluated_worktree_fingerprint {
        let current_fingerprint =
            crate::state::current_worktree_fingerprint_with_cancellation(&current, cancelled)?;
        let Some(current_fingerprint) = current_fingerprint.fingerprint else {
            anyhow::bail!(
                "Current worktree fingerprint could not be verified after evaluating required work gates: {}",
                current_fingerprint
                    .error
                    .unwrap_or_else(|| "unknown fingerprint error".into())
            );
        };
        if current_fingerprint != evaluated {
            anyhow::bail!(
                "Worktree changed while evaluating required work gates; rerun `jig work gates` and retry"
            );
        }
    }
    // The worktree scan excludes `.agent/**`; reload once more afterward so a
    // manifest-only authority change racing that scan cannot reach plan close.
    crate::cancellation::ensure_status_collection_active(cancelled)?;
    let current = RepoContext::load_from_root(ctx.root().to_path_buf())
        .context("Failed to recheck repository authority before closing the work plan")?;
    ensure_finish_config_is_current(ctx, &current)
}

fn ensure_finish_config_is_current(ctx: &RepoContext, current: &RepoContext) -> Result<()> {
    if current.work_gates() != ctx.work_gates() {
        anyhow::bail!(
            "Work gate configuration changed while evaluating required work gates; rerun `jig work gates` and retry"
        );
    }
    if current.contract_digest() != ctx.contract_digest() {
        anyhow::bail!(
            "Repository execution authority changed while evaluating required work gates; rerun `jig work gates` and retry"
        );
    }
    Ok(())
}

pub(super) fn start_from_args(ctx: &RepoContext, args: Value) -> Result<Value> {
    let request: WorkStartRequest = request_from_args(args)?;
    start(ctx, request.into())
}

pub(super) fn goal_from_args(ctx: &RepoContext, args: Value) -> Result<Value> {
    goal::goal(ctx, request_from_args(args)?)
}

pub(super) fn append_from_args(ctx: &RepoContext, args: Value) -> Result<Value> {
    let request: WorkAppendRequest = request_from_args(args)?;
    plans_append(ctx, request.into())
}

pub(super) fn check_from_args_with_observer(
    ctx: &RepoContext,
    args: Value,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let request: WorkCheckRequest = request_from_args(args)?;
    checks::check_from_mcp_with_observer(ctx, request, observer)
}

pub(super) fn gates_from_args(ctx: &RepoContext, args: Value) -> Result<Value> {
    let request: WorkGatesRequest = request_from_args(args)?;
    gates::gates(ctx, request)
}

pub(super) fn evidence_from_args(ctx: &RepoContext, args: Value) -> Result<Value> {
    let request: WorkEvidenceRequest = request_from_args(args)?;
    gates::evidence(ctx, request)
}

pub(super) fn review_from_args_with_observer(
    ctx: &RepoContext,
    args: Value,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let request: WorkReviewRequest = request_from_args(args)?;
    review::review_with_observer(ctx, request, observer)
}

pub(super) fn refine_from_args_with_observer(
    ctx: &RepoContext,
    args: Value,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let request: WorkRefineRequest = request_from_args(args)?;
    review::refine_from_mcp_with_observer(ctx, request, observer)
}

pub(super) fn decide_from_args(ctx: &RepoContext, args: Value) -> Result<Value> {
    let request: WorkDecisionRequest = request_from_args(args)?;
    decisions_add(ctx, request.into())
}

pub(super) fn receipts_from_args(ctx: &RepoContext, args: Value) -> Result<Value> {
    let request: WorkReceiptsRequest = request_from_args(args)?;
    receipts_list(ctx, request.into())
}

pub(super) fn finish_from_args(ctx: &RepoContext, args: Value) -> Result<Value> {
    let request: WorkFinishRequest = request_from_args(args)?;
    finish(ctx, request)
}

fn request_from_args<T>(args: Value) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(args).context("Invalid work tool arguments")
}

const fn session_end_request_for_finish(outcome: Option<String>) -> SessionEndRequest {
    SessionEndRequest {
        session_id: None,
        outcome,
    }
}
