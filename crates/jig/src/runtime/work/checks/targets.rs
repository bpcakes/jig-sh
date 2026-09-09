use super::super::gates::check_target_snapshot;
use super::*;
use crate::repository::{PlanRunRequest, plan_run, validate_current_repository_authority};

pub(super) fn check(
    ctx: &RepoContext,
    plan_id: &str,
    required: BTreeSet<jig_contract::TargetId>,
    execution: WorkCheckExecution,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let started = now_ms();
    let catalog = RepositoryCatalog::from_context(ctx)?;
    validate_current_repository_authority(ctx, catalog.config_digest())?;
    let before = check_target_snapshot(ctx, plan_id, &|| observer.cancelled())?;
    let mut scheduled = required
        .difference(&before.passing)
        .cloned()
        .collect::<BTreeSet<_>>();

    // The planner executes dependencies normally. Include every required
    // dependent of that closure so rerunning a shared dependency cannot leave
    // an older dependent proof behind.
    loop {
        let mut expanded = scheduled.clone();
        for target in &required {
            let action = catalog
                .action(target)
                .ok_or_else(|| anyhow!("unknown target {target}"))?;
            if scheduled.contains(target) {
                expanded.extend(action.depends_on.iter().cloned());
            } else if action
                .depends_on
                .iter()
                .any(|dependency| scheduled.contains(dependency))
            {
                expanded.insert(target.clone());
            }
        }
        if expanded == scheduled {
            break;
        }
        scheduled = expanded;
    }

    let mut result = json!({"plan": null, "run": null, "results": [], "failed_targets": []});
    let mut dispositions = std::collections::BTreeMap::new();
    let mut run_ok = true;
    let mut error = String::new();
    if !scheduled.is_empty() {
        let plan = plan_run(
            ctx,
            &catalog,
            PlanRunRequest {
                selectors: scheduled.iter().map(ToString::to_string).collect(),
                profile: None,
                affected_base: None,
                comparison: None,
                work_plan_id: Some(plan_id.to_owned()),
            },
        )?;
        let evidence = execution.execute_evidence(
            ctx,
            &catalog,
            plan.clone(),
            ExecuteCheckRunRequest {
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
                    if target.started_at_ms.is_some() {
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
    let after = check_target_snapshot(ctx, plan_id, &|| observer.cancelled())?;
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
            evidence: Some(json!({"schema": "jig.work_check_targets/v1", "targets": evidence})),
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
    result["target_validation_receipt_id"] = json!(receipt_id);
    Ok(result)
}
