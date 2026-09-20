use super::*;

pub(super) fn check_with_execution(
    ctx: &RepoContext,
    opts: WorkCheckRequest,
    execution: WorkCheckExecution,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    // Closed plans are inspectable through gates/evidence, but checks append
    // fresh receipts and must stay tied to open work.
    crate::state::ensure_plan_is_open(ctx, &opts.plan_id)?;
    if !opts.gates.is_empty() {
        if !opts.tools.is_empty() {
            bail!("Work check accepts either gate ids or tool names, not both");
        }
        let configured = ctx.work_gates();
        let mut selected = Vec::new();
        let mut targets = BTreeSet::new();
        let mut seen = BTreeSet::new();
        for id in &opts.gates {
            if !seen.insert(id) {
                continue;
            }
            let gate = configured
                .iter()
                .find(|gate| gate.id() == id)
                .ok_or_else(|| anyhow!("Unknown configured check gate id: {id}"))?;
            match gate {
                crate::context::WorkGate::Check(gate) => {
                    validate_check_tool(ctx, &gate.tool, "Work check")?;
                    selected.push(SelectedCheck::Gate {
                        gate: gate.clone(),
                        force: true,
                    });
                }
                crate::context::WorkGate::Evidence(gate) => {
                    let catalog = RepositoryCatalog::from_context(ctx)?;
                    targets.extend(resolve_evidence_targets(&catalog, &gate.selector)?);
                }
                crate::context::WorkGate::CodexReview(_) => {
                    bail!(
                        "Gate {id} is a review gate; use work review --plan-id {} --gate {id}",
                        opts.plan_id
                    );
                }
                crate::context::WorkGate::Unsupported(_) => {
                    bail!("Unsupported configured check gate id: {id}");
                }
            }
        }
        if targets.is_empty() {
            return check_selected_with_observer(ctx, &opts.plan_id, selected, execution, observer);
        }
        // Resolve and validate the whole native plan before any legacy child starts.
        {
            let catalog = RepositoryCatalog::from_context(ctx)?;
            crate::repository::plan_run_with_cancellation(
                ctx,
                &catalog,
                crate::repository::PlanRunRequest {
                    selectors: targets.iter().map(ToString::to_string).collect(),
                    work_plan_id: Some(opts.plan_id.clone()),
                    ..Default::default()
                },
                &|| observer.cancelled(),
            )?;
        }
        return check_combined_with_execution(
            ctx,
            &opts.plan_id,
            CheckSelection {
                selected,
                targets,
                force: true,
            },
            FailureMode::Abort,
            execution,
            observer,
        );
    }
    if opts.tools.is_empty() {
        return check_configured_with_execution(
            ctx,
            &opts.plan_id,
            FailureMode::Abort,
            execution,
            observer,
        );
    }
    let mut result = check_selected_with_observer(
        ctx,
        &opts.plan_id,
        selected_checks(ctx, &opts.gates, &opts.tools)?,
        execution,
        observer,
    )?;
    if !opts.tools.is_empty()
        && ctx
            .work_gates()
            .iter()
            .any(|gate| matches!(gate, crate::context::WorkGate::Evidence(_)))
    {
        result["native_evidence_note"] = json!(
            "Selected --tool checks record legacy tool evidence; these receipts cannot satisfy configured native target gates. Inspect work gates for plan-bound native refresh commands."
        );
    }
    Ok(result)
}
