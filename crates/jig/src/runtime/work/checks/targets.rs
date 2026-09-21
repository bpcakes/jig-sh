use super::super::gates::check_target_snapshot;
use super::*;
use crate::repository::{
    PlanRunRequest, plan_run_with_cancellation, validate_current_repository_authority,
};

pub(super) struct PlannedCheckOutcome {
    pub(super) result: Value,
    pub(super) after: super::super::gates::SelectedInvocationSnapshot,
}

pub(super) struct PlannedCheckInput<'a> {
    pub(super) reuse_after_resource_wait: bool,
    pub(super) plan_id: &'a str,
    pub(super) catalog: &'a RepositoryCatalog,
    pub(super) phase: crate::command::WorkCheckPhase,
    pub(super) before: &'a super::super::gates::SelectedInvocationSnapshot,
    pub(super) scheduled: &'a BTreeSet<jig_contract::TargetId>,
}

pub(super) fn check_planned(
    ctx: &RepoContext,
    selection: jig_contract::RunPlan,
    input: PlannedCheckInput<'_>,
    execution: WorkCheckExecution,
    observer: &mut dyn ExecutionControl,
) -> Result<PlannedCheckOutcome> {
    let started = now_ms();
    let PlannedCheckInput {
        reuse_after_resource_wait,
        plan_id,
        catalog,
        phase,
        before,
        scheduled,
    } = input;
    if selection.config_digest != catalog.config_digest() {
        bail!("selected phase authority changed before execution; select again");
    }
    validate_current_repository_authority(ctx, catalog.config_digest())?;
    let required = selection
        .targets
        .iter()
        .map(|target| target.target.clone())
        .collect::<BTreeSet<_>>();
    if !before.unavailable.is_empty() {
        bail!(
            "selected invocation authority is unavailable for: {}; resolve the reported freshness authority and select again",
            before
                .unavailable
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    let mut result = json!({"plan": null, "run": null, "results": [], "failed_targets": []});
    let mut dispositions = std::collections::BTreeMap::new();
    let mut run_ok = true;
    let mut error = String::new();
    if !scheduled.is_empty() {
        let plan = scheduled_plan(ctx, catalog, plan_id, &selection, scheduled, observer)?;
        let evidence = execution.execute_evidence(
            ctx,
            catalog,
            plan.clone(),
            ExecuteCheckRunRequest {
                reuse_after_resource_wait,
                alias_override: None,
                work_plan_id: Some(plan_id.to_owned()),
                record_receipts: true,
                fail_fast: false,
            },
            observer,
        )?;
        run_ok = evidence.run.result.conclusion == Some(RunConclusion::Success);
        if !run_ok {
            error = evidence_failure_message(&evidence.run.result);
        }
        for target in &evidence.run.result.targets {
            dispositions.insert(
                target.target.clone(),
                if target.reused_from.is_some() {
                    "reused"
                } else if target.started_at_ms.is_some() {
                    "selected"
                } else {
                    "deferred"
                },
            );
        }
        result["plan"] = json!(plan);
        result["run"] = json!(evidence.run.result);
        result["results"] = json!(evidence.results);
        result["failed_targets"] = json!(evidence.failed_targets);
        result["source_observations"] = json!(evidence.source_observations);
    }

    validate_current_repository_authority(ctx, catalog.config_digest())?;
    let after = super::super::gates::selected_invocation_snapshot(
        ctx,
        plan_id,
        catalog,
        &selection.targets,
        &|| observer.cancelled(),
    )?;
    let stable = before.fingerprint.is_some() && before.fingerprint == after.fingerprint;
    let ok = run_ok
        && stable
        && required.is_subset(&after.passing)
        && after.unavailable.is_empty()
        && !observer.cancelled();
    if !ok && error.is_empty() {
        error = "selected invocation evidence is not current and passing, or authority changed during validation".into();
    }
    let evidence = after
        .targets
        .iter()
        .map(|target| {
            let mut value = target.to_value();
            value["disposition"] = json!(if after.unavailable.contains(target.target()) {
                "unavailable"
            } else {
                dispositions
                    .get(target.target())
                    .copied()
                    .unwrap_or("reused")
            });
            value
        })
        .collect::<Vec<_>>();
    let mut batch_evidence = json!({"schema": "jig.work_check_targets/v1", "targets": evidence});
    let effective_time = (ctx.contract_version()
        >= jig_contract::freshness::TARGET_FRESHNESS_CONTRACT_VERSION)
        .then(|| result_effective_time_validity(&batch_evidence));
    if let Some(validity) = effective_time {
        batch_evidence["effective_valid_until_ms"] = json!(validity.effective_valid_until_ms);
        batch_evidence["effective_requires_time_validity"] =
            json!(validity.effective_requires_time_validity);
    }
    let receipt_id = record_receipt_with_cancellation(
        ctx,
        ReceiptInput {
            tool_name: tool::WORK_CHECK,
            args: json!({
                "plan_id": plan_id,
                "phase": phase.as_str(),
                "targets": required,
            }),
            invoked_command_key: None,
            plan_id: Some(plan_id.to_owned()),
            started_at_ms: started,
            ended_at_ms: now_ms(),
            exit_status: if ok { 0 } else { 1 },
            stdout: "",
            stderr: &error,
            evidence: Some(batch_evidence),
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: Some(if stable {
                Ok(after
                    .fingerprint
                    .clone()
                    .expect("stable fingerprint exists"))
            } else {
                Err("source fingerprint changed or was unavailable during validation".into())
            }),
        },
        &|| observer.cancelled(),
    )?;
    result["ok"] = json!(ok);
    result["error"] = if error.is_empty() {
        Value::Null
    } else {
        json!(error)
    };
    result["target_evidence"] = json!(evidence);
    if let Some(validity) = effective_time {
        result["effective_valid_until_ms"] = json!(validity.effective_valid_until_ms);
        result["effective_requires_time_validity"] =
            json!(validity.effective_requires_time_validity);
    }
    result["freshness_collection"] = json!(after.freshness_collection);
    result["target_validation_receipt_id"] = json!(receipt_id);
    Ok(PlannedCheckOutcome { result, after })
}

fn scheduled_plan(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    plan_id: &str,
    selection: &jig_contract::RunPlan,
    scheduled: &BTreeSet<jig_contract::TargetId>,
    observer: &dyn ExecutionControl,
) -> Result<jig_contract::RunPlan> {
    let arguments = selection
        .targets
        .iter()
        .filter(|target| scheduled.contains(&target.target) && !target.arguments.is_empty())
        .map(|target| (target.target.clone(), target.arguments.clone()))
        .collect();
    let plan = crate::repository::plan_focused_check_run_with_cancellation(
        ctx,
        catalog,
        PlanRunRequest {
            selectors: scheduled.iter().map(ToString::to_string).collect(),
            profile: None,
            affected_base: None,
            comparison: None,
            work_plan_id: Some(plan_id.to_owned()),
        },
        arguments,
        &|| observer.cancelled(),
    )?;
    let planned_targets = plan
        .targets
        .iter()
        .map(|target| target.target.clone())
        .collect::<BTreeSet<_>>();
    let matches_frozen_selection = plan.config_digest == selection.config_digest
        && plan.source == selection.source
        && planned_targets == *scheduled
        && plan.targets.iter().all(|planned| {
            selection.targets.iter().any(|selected| {
                planned.target == selected.target
                    && planned.intent == selected.intent
                    && planned.effects == selected.effects
                    && planned.runner == selected.runner
                    && planned.arguments == selected.arguments
                    && planned.inputs == selected.inputs
                    && planned.depends_on == selected.depends_on
                    && planned.timeout_seconds == selected.timeout_seconds
                    && planned.resources == selected.resources
                    && planned.result_parser == selected.result_parser
                    && planned.input_digest == selected.input_digest
                    && planned.prepared_native_input == selected.prepared_native_input
                    && planned.prepared_rust_input == selected.prepared_rust_input
            })
        });
    if !matches_frozen_selection {
        bail!("selected phase authority changed before scheduled execution; select again");
    }
    Ok(plan)
}

pub(super) fn check(
    ctx: &RepoContext,
    plan_id: &str,
    required: BTreeSet<jig_contract::TargetId>,
    force: bool,
    execution: WorkCheckExecution,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let started = now_ms();
    let catalog = RepositoryCatalog::from_context(ctx)?;
    validate_current_repository_authority(ctx, catalog.config_digest())?;
    let before = check_target_snapshot(ctx, plan_id, &required, &|| observer.cancelled())?;
    let empty = BTreeSet::new();
    let scheduled = super::super::check_schedule::schedule(
        &catalog,
        &required,
        if force { &empty } else { &before.passing },
    )?;

    let mut result = json!({"plan": null, "run": null, "results": [], "failed_targets": []});
    let mut dispositions = std::collections::BTreeMap::new();
    let mut run_ok = true;
    let mut error = String::new();
    if !scheduled.is_empty() {
        let plan = plan_run_with_cancellation(
            ctx,
            &catalog,
            PlanRunRequest {
                selectors: scheduled.iter().map(ToString::to_string).collect(),
                profile: None,
                affected_base: None,
                comparison: None,
                work_plan_id: Some(plan_id.to_owned()),
            },
            &|| observer.cancelled(),
        )?;
        let evidence = execution.execute_evidence(
            ctx,
            &catalog,
            plan.clone(),
            ExecuteCheckRunRequest {
                reuse_after_resource_wait: !force,
                alias_override: None,
                work_plan_id: Some(plan_id.to_owned()),
                record_receipts: true,
                fail_fast: false,
            },
            observer,
        )?;
        run_ok = evidence.run.result.conclusion == Some(RunConclusion::Success);
        if !run_ok {
            error = evidence_failure_message(&evidence.run.result);
        }
        for target in &evidence.run.result.targets {
            if let Some(receipt_id) = &target.receipt_id {
                dispositions.insert(
                    receipt_id.clone(),
                    if target.reused_from.is_some() {
                        "reused"
                    } else if target.started_at_ms.is_some() {
                        "executed"
                    } else {
                        "not_started"
                    },
                );
            }
        }
        result["plan"] = json!(plan);
        result["run"] = json!(evidence.run.result);
        result["results"] = json!(evidence.results);
        result["failed_targets"] = json!(evidence.failed_targets);
        result["source_observations"] = json!(evidence.source_observations);
    }

    // This is a new validation of original receipts, not an empty execution or
    // a replacement target receipt. Reassess every target after execution.
    validate_current_repository_authority(ctx, catalog.config_digest())?;
    let after = check_target_snapshot(ctx, plan_id, &required, &|| observer.cancelled())?;
    let stable = before.fingerprint.is_some() && before.fingerprint == after.fingerprint;
    let ok = run_ok && stable && required.is_subset(&after.passing) && !observer.cancelled();
    if !ok && error.is_empty() {
        error = "required target evidence is not current and passing, or source changed during validation".into();
    }
    let evidence: Vec<_> = after
        .targets
        .into_values()
        .map(|mut evidence| {
            evidence["disposition"] = json!(
                evidence["receipt_id"]
                    .as_str()
                    .and_then(|id| dispositions.get(id))
                    .copied()
                    .unwrap_or("reused")
            );
            evidence
        })
        .collect();
    let mut batch_evidence = json!({"schema": "jig.work_check_targets/v1", "targets": evidence});
    let effective_time = (ctx.contract_version()
        >= jig_contract::freshness::TARGET_FRESHNESS_CONTRACT_VERSION)
        .then(|| result_effective_time_validity(&batch_evidence));
    if let Some(validity) = effective_time {
        batch_evidence["effective_valid_until_ms"] = json!(validity.effective_valid_until_ms);
        batch_evidence["effective_requires_time_validity"] =
            json!(validity.effective_requires_time_validity);
    }
    let receipt_id = record_receipt_with_cancellation(
        ctx,
        ReceiptInput {
            tool_name: tool::WORK_CHECK,
            args: json!({"plan_id": plan_id, "targets": required}),
            invoked_command_key: None,
            plan_id: Some(plan_id.to_owned()),
            started_at_ms: started,
            ended_at_ms: now_ms(),
            exit_status: if ok { 0 } else { 1 },
            stdout: "",
            stderr: &error,
            evidence: Some(batch_evidence),
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: Some(if stable {
                Ok(after.fingerprint.expect("stable fingerprint exists"))
            } else {
                Err("source fingerprint changed or was unavailable during validation".into())
            }),
        },
        &|| observer.cancelled(),
    )?;
    result["ok"] = json!(ok);
    result["error"] = if error.is_empty() {
        Value::Null
    } else {
        json!(error)
    };
    result["target_evidence"] = json!(evidence);
    if let Some(validity) = effective_time {
        result["effective_valid_until_ms"] = json!(validity.effective_valid_until_ms);
        result["effective_requires_time_validity"] =
            json!(validity.effective_requires_time_validity);
    }
    if let Some(stats) = after.freshness_collection {
        result["freshness_collection"] = json!(stats);
    }
    result["target_validation_receipt_id"] = json!(receipt_id);
    Ok(result)
}
