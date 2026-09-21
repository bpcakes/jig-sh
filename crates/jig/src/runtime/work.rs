use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::command::{
    WorkAppendRequest, WorkCheckRequest, WorkCommand, WorkDecisionRequest, WorkEvidenceRequest,
    WorkFinishRequest, WorkGatesRequest, WorkReceiptsRequest, WorkRefineRequest, WorkRetireRequest,
    WorkReviewRequest, WorkStartRequest,
};
use crate::context::RepoContext;
use crate::execution::ExecutionControl;
use crate::state::{
    DecisionAddRequest, PlanAppendRequest, PlanCloseRequest, PlanDisposition, PlanOpenRequest,
    PlanRetireRequest, ReceiptListFilter, SessionEndIfCurrent, decisions_add, plan_owner_session,
    plans_append, plans_close, plans_open_prepared, plans_retire, prepare_plan_open, receipts_list,
    session_end_if_current, session_start, state_summary_with_cancellation,
};

mod check_schedule;
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
            gates::snapshot_with_cancellation(ctx, opts, &|| observer.cancelled())
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
        WorkCommand::Retire(opts) => retire(ctx, opts),
    }
}

pub(super) fn open_plan_gate_snapshots_with_cancellation(
    ctx: &RepoContext,
    plan_ids: &[String],
    cancelled: &dyn Fn() -> bool,
    freshness_timeout_ms: Option<u64>,
) -> Result<std::collections::BTreeMap<String, Value>> {
    gates::open_plan_snapshots_with_cancellation(ctx, plan_ids, cancelled, freshness_timeout_ms)
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
    let session_id = session["session_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("session start did not return a session id"))?
        .to_string();
    let plan = plans_open_prepared(ctx, plan, Some(session_id))?;

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

#[derive(Default)]
pub(in crate::runtime) struct RequiredGateProof {
    pub(in crate::runtime) worktree_fingerprint: Option<String>,
    pub(in crate::runtime) valid_until_ms: Option<u64>,
    pub(in crate::runtime) requires_time_validity: bool,
}

pub(in crate::runtime) fn finish_after_required_gates_passed(
    ctx: &RepoContext,
    opts: WorkFinishRequest,
    evaluated: RequiredGateProof,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    ensure_finish_authority_is_current(ctx, evaluated.worktree_fingerprint.as_deref(), cancelled)?;

    anyhow::ensure!(
        crate::state::time_validity_is_current(
            evaluated.valid_until_ms,
            evaluated.requires_time_validity,
            crate::state::now_ms()
        ),
        "Required work gate evidence expired before the plan could close; rerun work check and retry"
    );
    crate::cancellation::ensure_status_collection_active(cancelled)?;
    let plan = plans_close(ctx, (&opts).into())?;
    complete_plan_closure(ctx, &opts.plan_id, plan, opts.outcome.or(opts.resolution))
}

fn ensure_finish_authority_is_current(
    ctx: &RepoContext,
    evaluated_worktree_fingerprint: Option<&str>,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    crate::cancellation::ensure_status_collection_active(cancelled)?;
    let current = ctx
        .reload_execution_authority()
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
    let current = ctx
        .reload_execution_authority()
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

/// Retire an open work plan that will not be delivered.
///
/// This is deliberately not a `work finish` variant: it evaluates no required
/// gates, writes no gate evidence, and never relaxes the completion authority
/// that `finish` enforces. It reuses the plan-close lease and linked-run
/// rejection so an open plan cannot be retired out from under a live run.
pub(super) fn retire(ctx: &RepoContext, opts: WorkRetireRequest) -> Result<Value> {
    let disposition = parse_disposition(&opts.disposition)?;
    let reason = opts.reason.trim();
    anyhow::ensure!(
        !reason.is_empty(),
        "Work plan retirement requires a nonblank --reason explaining why the plan is not being delivered"
    );
    let superseded_by = match opts.superseded_by.as_deref().map(str::trim) {
        Some("") => {
            anyhow::bail!("--superseded-by must name a plan or issue reference when it is provided")
        }
        other => other.map(str::to_string),
    };

    let plan = plans_retire(
        ctx,
        PlanRetireRequest {
            plan_id: opts.plan_id.clone(),
            disposition: disposition.as_str(),
            reason: reason.to_string(),
            superseded_by,
        },
    )?;
    complete_plan_closure(
        ctx,
        &opts.plan_id,
        plan,
        Some(disposition.as_str().to_string()),
    )
}

fn complete_plan_closure(
    ctx: &RepoContext,
    plan_id: &str,
    plan: Value,
    outcome: Option<String>,
) -> Result<Value> {
    let (session, session_status) = end_owning_session(ctx, plan_id, outcome)
        .map_err(|error| crate::state::PlanClosurePartialFailure::session(plan_id, &plan, error))?;

    Ok(json!({
        "ok": true,
        "plan": plan,
        "session": session,
        "session_status": session_status,
    }))
}

fn parse_disposition(requested: &str) -> Result<PlanDisposition> {
    PlanDisposition::ALL
        .iter()
        .copied()
        .find(|disposition| disposition.as_str() == requested.trim())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown work plan disposition '{requested}'; expected one of: {}",
                PlanDisposition::ALL
                    .iter()
                    .map(|disposition| disposition.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// End the current session only when durable state proves it opened this plan.
///
/// The plan-open receipt is the durable ownership record. Without a match, an
/// unrelated current session stays untouched and the caller is told why.
fn end_owning_session(
    ctx: &RepoContext,
    plan_id: &str,
    outcome: Option<String>,
) -> Result<(Option<Value>, Value)> {
    let owner = plan_owner_session(ctx, plan_id)?;
    let Some(owner_id) = owner.as_deref() else {
        let current = crate::state::current_session(ctx)?;
        return Ok((
            None,
            json!({
                "action": "left_active",
                "owner_session_id": Value::Null,
                "current_session_id": current,
                "detail": format!(
                    "no durable record proves which session opened plan {plan_id}; no session was ended"
                ),
            }),
        ));
    };

    match session_end_if_current(ctx, owner_id, outcome)? {
        SessionEndIfCurrent::Ended(session) => Ok((
            Some(session),
            json!({
                "action": "ended",
                "owner_session_id": owner,
                "current_session_id": owner,
                "detail": format!("ended owning session {owner_id}"),
            }),
        )),
        SessionEndIfCurrent::NotCurrent(Some(current_id)) => Ok((
            None,
            json!({
                "action": "left_active",
                "owner_session_id": owner,
                "current_session_id": current_id,
                "detail": format!(
                    "left current session {current_id} active; plan {plan_id} is owned by session {owner_id}"
                ),
            }),
        )),
        SessionEndIfCurrent::NotCurrent(None) => Ok((
            None,
            json!({
                "action": "none",
                "owner_session_id": owner,
                "current_session_id": Value::Null,
                "detail": format!("owning session {owner_id} is not the current session"),
            }),
        )),
    }
}

pub(super) fn retire_from_args(ctx: &RepoContext, args: Value) -> Result<Value> {
    let request: WorkRetireRequest = request_from_args(args)?;
    retire(ctx, request)
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
    projection: crate::surface::ResponseSurface,
) -> Result<Value> {
    let mut request: WorkCheckRequest = request_from_args(args)?;
    request.projection = projection;
    checks::check_from_mcp_with_observer(ctx, request, observer)
}

pub(super) fn gates_from_args(
    ctx: &RepoContext,
    args: Value,
    projection: crate::surface::ResponseSurface,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    let mut request: WorkGatesRequest = request_from_args(args)?;
    request.projection = projection;
    if projection == crate::surface::ResponseSurface::AgentV1 {
        gates::snapshot_with_cancellation(ctx, request, cancelled)
    } else {
        gates::gates(ctx, request)
    }
}

pub(super) fn evidence_from_args(
    ctx: &RepoContext,
    args: Value,
    projection: crate::surface::ResponseSurface,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    let mut request: WorkEvidenceRequest = request_from_args(args)?;
    request.projection = projection;
    if projection == crate::surface::ResponseSurface::AgentV1 {
        gates::evidence_with_cancellation(ctx, request, cancelled)
    } else {
        gates::evidence(ctx, request)
    }
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
