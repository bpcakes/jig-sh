use super::*;

pub(super) fn check_configured_with_execution(
    ctx: &RepoContext,
    plan_id: &str,
    failure_mode: FailureMode,
    execution: WorkCheckExecution,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let selected = selected_checks(ctx, &[], &[])?;
    let targets = configured_evidence_targets_if_any(ctx)?;
    check_combined_with_execution(
        ctx,
        plan_id,
        CheckSelection {
            selected,
            targets,
            force: false,
        },
        failure_mode,
        execution,
        observer,
    )
}

pub(super) struct CheckSelection {
    pub(super) selected: Vec<SelectedCheck>,
    pub(super) targets: BTreeSet<jig_contract::TargetId>,
    pub(super) force: bool,
}

pub(super) fn check_combined_with_execution(
    ctx: &RepoContext,
    plan_id: &str,
    selection: CheckSelection,
    failure_mode: FailureMode,
    execution: WorkCheckExecution,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let CheckSelection {
        selected,
        targets,
        force,
    } = selection;
    if selected.is_empty() && targets.is_empty() {
        bail!(EMPTY_CHECK_SELECTION_MESSAGE);
    }

    let mut result = if selected.is_empty() {
        json!({
            "ok": true,
            "plan_id": plan_id,
            "checks": [],
            "change_evidence": null,
            "gate_evidence": [],
            "receipt_id": null,
        })
    } else {
        // Run the full configured set before reporting aggregate failures so
        // repository evidence is still refreshed when a command-backed check
        // fails.
        check_selected_with_failure_mode(
            ctx,
            plan_id,
            selected,
            FailureMode::Collect,
            execution,
            observer,
        )?
    };
    let mut check_failures = result["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|check| manifest_tool_result_failure(check).transpose())
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .map(|(_, message)| message)
        .collect::<Vec<_>>();
    if let Some(error) = result["error"].as_str()
        && !check_failures.iter().any(|failure| failure == error)
    {
        check_failures.push(error.to_string());
    }

    if targets.is_empty() {
        if failure_mode.aborts() && !check_failures.is_empty() {
            bail!("{}", check_failures.join("\n"));
        }
        if let Some(object) = result.as_object_mut() {
            object.insert("ok".into(), json!(check_failures.is_empty()));
        }
        return Ok(result);
    }

    let evidence = targets::check(ctx, plan_id, targets, force, execution, observer)?;
    let evidence_ok = evidence["ok"] == true;
    let evidence_failure = evidence["error"]
        .as_str()
        .unwrap_or("required target evidence is not current and passing");
    let checks_ok = check_failures.is_empty();
    let object = result
        .as_object_mut()
        .ok_or_else(|| anyhow!("work check result was not a JSON object"))?;
    object.extend(
        evidence
            .as_object()
            .expect("target check returns an object")
            .clone(),
    );
    object.insert("ok".into(), json!(checks_ok && evidence_ok));

    if failure_mode.aborts() {
        let check_failure = (!checks_ok).then(|| check_failures.join("\n"));
        match (check_failure, evidence_ok) {
            (Some(failure), false) => bail!("{failure}\n{evidence_failure}"),
            (Some(failure), true) => bail!("{failure}"),
            (None, false) => bail!("{evidence_failure}"),
            (None, true) => {}
        }
    }
    Ok(result)
}

pub(super) fn configured_evidence_targets_if_any(
    ctx: &RepoContext,
) -> Result<BTreeSet<jig_contract::TargetId>> {
    let evidence_gates = ctx
        .work_gates()
        .into_iter()
        .filter_map(|gate| match gate {
            crate::context::WorkGate::Evidence(gate) if gate.required => Some(gate),
            _ => None,
        })
        .collect::<Vec<_>>();
    if evidence_gates.is_empty() {
        return Ok(BTreeSet::new());
    }
    let catalog = RepositoryCatalog::from_context(ctx)?;
    let mut targets = BTreeSet::new();
    for gate in evidence_gates {
        targets.extend(resolve_evidence_targets(&catalog, &gate.selector)?);
    }
    Ok(targets)
}
