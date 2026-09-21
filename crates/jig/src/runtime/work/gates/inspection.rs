use super::*;

pub(in crate::runtime::work) fn gates(ctx: &RepoContext, opts: WorkGatesRequest) -> Result<Value> {
    let timeout = inspection_timeout(opts.freshness_timeout_ms)?;
    let plan_id = resolve_work_plan_id(ctx, opts.plan_id)?;
    let report = gate_report(ctx, &plan_id, timeout)?;
    project(&report, "work gates", opts.projection)
}

pub(in crate::runtime::work) fn snapshot_with_cancellation(
    ctx: &RepoContext,
    opts: WorkGatesRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    ensure_gate_collection_active(cancelled)?;
    let timeout = inspection_timeout(opts.freshness_timeout_ms)?;
    let plan_id = resolve_work_plan_id_with_cancellation(ctx, opts.plan_id, cancelled)?;
    ensure_gate_collection_active(cancelled)?;
    let report = gate_report_with_cancellation(ctx, &plan_id, cancelled, timeout)?;
    project(&report, "work gates", opts.projection)
}

pub(in crate::runtime::work) fn evidence(
    ctx: &RepoContext,
    opts: WorkEvidenceRequest,
) -> Result<Value> {
    let timeout = inspection_timeout(opts.freshness_timeout_ms)?;
    let plan_id = resolve_work_plan_id(ctx, opts.plan_id)?;
    let report = gate_report(ctx, &plan_id, timeout)?;
    if opts.projection == crate::surface::ResponseSurface::AgentV1 {
        return compact::render(&report, "work evidence", None);
    }
    evidence_from_report(report)
}

pub(in crate::runtime::work) fn evidence_with_cancellation(
    ctx: &RepoContext,
    opts: WorkEvidenceRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    ensure_gate_collection_active(cancelled)?;
    let timeout = inspection_timeout(opts.freshness_timeout_ms)?;
    let plan_id = resolve_work_plan_id_with_cancellation(ctx, opts.plan_id, cancelled)?;
    ensure_gate_collection_active(cancelled)?;
    let report = gate_report_with_cancellation(ctx, &plan_id, cancelled, timeout)?;
    if opts.projection == crate::surface::ResponseSurface::AgentV1 {
        return compact::render(&report, "work evidence", None);
    }
    evidence_from_report(report)
}

fn evidence_from_report(report: GateReport) -> Result<Value> {
    let latest = latest_passing_gates(&report);
    let mut status = report.to_value();
    if let Some(argv) = status
        .get_mut("recovery")
        .and_then(|recovery| recovery.get_mut("next_step"))
        .and_then(|command| command.get_mut("argv"))
        .and_then(Value::as_array_mut)
        && argv.get(2).and_then(Value::as_str) == Some("gates")
    {
        argv[2] = json!("evidence");
    }
    let object = status
        .as_object_mut()
        .ok_or_else(|| anyhow!("work gate status was not a JSON object"))?;
    object.insert("command".into(), json!("work evidence"));
    object.insert("latest_passing_gates".into(), json!(latest));
    Ok(status)
}

fn project(
    report: &GateReport,
    command: &str,
    projection: crate::surface::ResponseSurface,
) -> Result<Value> {
    match projection {
        crate::surface::ResponseSurface::Standard => Ok(report.to_value()),
        crate::surface::ResponseSurface::AgentV1 => compact::render(report, command, None),
    }
}

pub(in crate::runtime::work) fn completion_after_check(
    ctx: &RepoContext,
    plan_id: &str,
    check: &Value,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    let report = gate_report_with_cancellation(ctx, plan_id, cancelled, RECORDING_TIMEOUT_MS)?;
    compact::render(&report, "work check", Some(check))
}
