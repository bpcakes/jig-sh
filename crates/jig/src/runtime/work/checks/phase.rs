use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::command::WorkCheckPhase;
use crate::repository::{
    PlanRunRequest, plan_run_with_cancellation, validate_current_repository_authority,
};
use serde::Serialize;

#[derive(Serialize)]
struct SelectedInvocationReport {
    target: jig_contract::TargetId,
    invocation: jig_contract::PlannedTarget,
    selection_reasons: Vec<jig_contract::SelectionReason>,
    evidence_validity: Value,
    disposition: String,
}

#[derive(Serialize)]
struct WorkCheckPhaseReport {
    phase: WorkCheckPhase,
    explain: bool,
    selected_invocations: Vec<SelectedInvocationReport>,
    selected_checks: Vec<Value>,
    final_requirements: Vec<Value>,
    pending_final_requirements: Vec<Value>,
    final_gates_ok: bool,
    final_recovery: Value,
}

pub(super) fn check_phase(
    ctx: &RepoContext,
    opts: WorkCheckRequest,
    execution: WorkCheckExecution,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    check_phase_with_pre_execution(ctx, opts, execution, observer, || {})
}

fn check_phase_with_pre_execution(
    ctx: &RepoContext,
    opts: WorkCheckRequest,
    execution: WorkCheckExecution,
    observer: &mut dyn ExecutionControl,
    before_execution: impl FnOnce(),
) -> Result<Value> {
    let phase = opts.phase.unwrap_or(WorkCheckPhase::Final);
    let catalog = RepositoryCatalog::from_context(ctx)?;
    let (focus_arguments, focus_fallbacks) =
        super::rust_focus::prepare_arguments(ctx, &catalog, &opts)?;
    validate_current_repository_authority(ctx, catalog.config_digest())?;
    let selected_source =
        current_worktree_fingerprint_with_cancellation(ctx, &|| observer.cancelled())?;

    let (selected_checks, explicit_targets) = phase_selection(ctx, &catalog, &opts, phase)?;
    let prepared_checks = if selected_checks.is_empty() {
        None
    } else {
        Some(prepare_check_batch(
            ctx,
            &opts.plan_id,
            selected_checks,
            Some(catalog.config_digest()),
            observer,
        )?)
    };
    let selected_check_report = prepared_checks
        .as_ref()
        .map(preview_check_batch)
        .unwrap_or_default();
    let selected_checks_ok = selected_check_report.iter().all(|check| {
        matches!(
            check["disposition"].as_str(),
            Some("reused" | "not_applicable")
        )
    });

    let selection = selected_phase_plan(
        ctx,
        &catalog,
        &opts,
        &explicit_targets,
        focus_arguments,
        observer,
    )?;
    if phase == WorkCheckPhase::Final
        && selection.is_none()
        && prepared_checks.is_none()
        && !opts.explain
    {
        bail!(EMPTY_CHECK_SELECTION_MESSAGE);
    }

    let before = selection
        .as_ref()
        .map(|plan| {
            super::super::gates::selected_invocation_snapshot(
                ctx,
                &opts.plan_id,
                &catalog,
                &plan.targets,
                &|| observer.cancelled(),
            )
        })
        .transpose()?;
    let required = selection
        .as_ref()
        .map(|plan| {
            plan.targets
                .iter()
                .map(|target| target.target.clone())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let empty = BTreeSet::new();
    let scheduled = before
        .as_ref()
        .map(|snapshot| {
            // Explicit gates force their targets and prerequisites, just as
            // the corresponding unphased execution does in targets::check.
            let passing = if opts.gates.is_empty() {
                &snapshot.passing
            } else {
                &empty
            };
            super::super::check_schedule::schedule(&catalog, &required, passing)
        })
        .transpose()?
        .unwrap_or_default();

    if opts.explain {
        validate_current_repository_authority(ctx, catalog.config_digest())?;
        let final_report = super::super::gates::gate_report_with_cancellation(
            ctx,
            &opts.plan_id,
            &|| observer.cancelled(),
            crate::repository::freshness::RECORDING_TIMEOUT_MS,
        )?;
        let final_report = final_report.to_value();
        validate_current_repository_authority(ctx, catalog.config_digest())?;
        ensure_source_unchanged(ctx, &selected_source, observer)?;
        let mut result = json!({
            "ok": true,
            "rust_focus_fallbacks": focus_fallbacks,
            "selected_ok": selected_checks_ok && before.as_ref().is_none_or(|snapshot| {
                    required.is_subset(&snapshot.passing) && snapshot.unavailable.is_empty()
                }),
            "plan_id": opts.plan_id,
            "checks": [],
            "gate_evidence": [],
            "receipt_id": null,
            "target_validation_receipt_id": null,
        });
        attach_phase_report(
            &mut result,
            phase,
            true,
            selection.as_ref(),
            before.as_ref(),
            &scheduled,
            None,
            selected_check_report,
            final_report,
        )?;
        return Ok(result);
    }

    if let Some(snapshot) = &before
        && !snapshot.unavailable.is_empty()
    {
        bail!(
            "selected invocation authority is unavailable for: {}; resolve the reported freshness authority and select again",
            snapshot
                .unavailable
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    before_execution();
    ensure_source_unchanged(ctx, &selected_source, observer)?;
    let legacy_result = if let Some(prepared) = prepared_checks {
        Some(super::batch::check_prepared_with_failure_mode(
            ctx,
            &opts.plan_id,
            prepared,
            now_ms(),
            FailureMode::Collect,
            execution,
            observer,
        )?)
    } else {
        None
    };
    let source_after_legacy = legacy_result
        .as_ref()
        .map(|_| current_worktree_fingerprint_with_cancellation(ctx, &|| observer.cancelled()))
        .transpose()?;
    let legacy_source_changed = source_after_legacy
        .as_ref()
        .is_some_and(|current| source_authority_changed_or_unavailable(&selected_source, current));
    let validation_source = source_after_legacy.as_ref().unwrap_or(&selected_source);
    let (native_result, after) = if legacy_source_changed {
        let after = selection
            .as_ref()
            .map(|plan| {
                super::super::gates::selected_invocation_snapshot(
                    ctx,
                    &opts.plan_id,
                    &catalog,
                    &plan.targets,
                    &|| observer.cancelled(),
                )
            })
            .transpose()?;
        let result = after.as_ref().map(deferred_native_result);
        (result, after)
    } else {
        let outcome = selection
            .clone()
            .map(|plan| {
                targets::check_planned(
                    ctx,
                    plan,
                    targets::PlannedCheckInput {
                        reuse_after_resource_wait: opts.gates.is_empty(),
                        plan_id: &opts.plan_id,
                        catalog: &catalog,
                        phase,
                        before: before
                            .as_ref()
                            .expect("a selected phase plan has a pre-execution snapshot"),
                        scheduled: &scheduled,
                    },
                    execution,
                    observer,
                )
            })
            .transpose()?;
        match outcome {
            Some(outcome) => (Some(outcome.result), Some(outcome.after)),
            None => (None, None),
        }
    };

    validate_current_repository_authority(ctx, catalog.config_digest())?;
    let final_report = super::super::gates::gate_report_with_cancellation(
        ctx,
        &opts.plan_id,
        &|| observer.cancelled(),
        crate::repository::freshness::RECORDING_TIMEOUT_MS,
    )?;
    let final_report = final_report.to_value();
    validate_current_repository_authority(ctx, catalog.config_digest())?;
    ensure_source_unchanged(ctx, validation_source, observer)?;

    let legacy_ok = legacy_result
        .as_ref()
        .is_none_or(|result| result["ok"] == true)
        && selected_check_evidence_is_current(&selected_check_report, &final_report);
    let native_ok = native_result
        .as_ref()
        .is_none_or(|result| result["ok"] == true);
    let selected_ok = !legacy_source_changed
        && legacy_ok
        && native_ok
        && after.as_ref().is_none_or(|snapshot| {
            required.is_subset(&snapshot.passing) && snapshot.unavailable.is_empty()
        });
    let mut result = legacy_result.unwrap_or_else(|| {
        json!({
            "plan_id": opts.plan_id,
            "checks": [],
            "gate_evidence": [],
            "receipt_id": null,
        })
    });
    if let Some(native) = native_result.as_ref() {
        for key in [
            "plan",
            "run",
            "results",
            "failed_targets",
            "source_observations",
            "target_evidence",
            "effective_valid_until_ms",
            "effective_requires_time_validity",
            "freshness_collection",
            "target_validation_receipt_id",
        ] {
            if let Some(value) = native.get(key) {
                result[key] = value.clone();
            }
        }
    }
    let mut errors = [
        result.get("error"),
        native_result.as_ref().and_then(|v| v.get("error")),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .filter(|error| !error.is_empty())
    .map(str::to_owned)
    .collect::<Vec<_>>();
    if legacy_source_changed {
        errors.push(
            "legacy final checks changed repository source authority; rerun the phase against the updated worktree"
                .into(),
        );
    }
    result["error"] = if errors.is_empty() {
        Value::Null
    } else {
        json!(errors.join("\n"))
    };
    result["ok"] = json!(selected_ok);
    result["selected_ok"] = json!(selected_ok);
    result["rust_focus_fallbacks"] = json!(focus_fallbacks);
    attach_phase_report(
        &mut result,
        phase,
        false,
        selection.as_ref(),
        after.as_ref(),
        &scheduled,
        native_result.as_ref(),
        selected_check_report,
        final_report,
    )?;
    Ok(result)
}

#[cfg(test)]
pub(in crate::runtime) fn check_phase_with_pre_execution_test_hook(
    ctx: &RepoContext,
    opts: WorkCheckRequest,
    observer: &mut dyn ExecutionControl,
    before_execution: impl FnOnce(),
) -> Result<Value> {
    check_phase_with_pre_execution(
        ctx,
        opts,
        WorkCheckExecution::WaitForLease,
        observer,
        before_execution,
    )
}

fn ensure_source_unchanged(
    ctx: &RepoContext,
    selected: &crate::state::CurrentWorktreeFingerprint,
    observer: &dyn ExecutionControl,
) -> Result<()> {
    let current = current_worktree_fingerprint_with_cancellation(ctx, &|| observer.cancelled())?;
    if source_authority_changed_or_unavailable(selected, &current) {
        bail!(
            "repository source authority changed during phase selection or validation; select again"
        );
    }
    Ok(())
}

fn source_authority_changed_or_unavailable(
    selected: &crate::state::CurrentWorktreeFingerprint,
    current: &crate::state::CurrentWorktreeFingerprint,
) -> bool {
    match (&selected.fingerprint, &current.fingerprint) {
        (Some(selected), Some(current)) => selected != current,
        _ => true,
    }
}

fn deferred_native_result(snapshot: &super::super::gates::SelectedInvocationSnapshot) -> Value {
    let evidence = snapshot
        .targets
        .iter()
        .map(|target| {
            let mut value = target.to_value();
            value["disposition"] = json!("deferred");
            value
        })
        .collect::<Vec<_>>();
    json!({
        "ok": false,
        "plan": null,
        "run": null,
        "results": [],
        "failed_targets": [],
        "error": null,
        "target_evidence": evidence,
        "freshness_collection": snapshot.freshness_collection,
        "target_validation_receipt_id": null,
    })
}

fn selected_phase_plan(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    opts: &WorkCheckRequest,
    explicit_targets: &BTreeSet<jig_contract::TargetId>,
    focus_arguments: BTreeMap<jig_contract::TargetId, jig_contract::ActionArguments>,
    observer: &dyn ExecutionControl,
) -> Result<Option<jig_contract::RunPlan>> {
    let plan_id = &opts.plan_id;
    let phase = opts.phase.unwrap_or(WorkCheckPhase::Final);
    let configured_selection = opts.gates.is_empty() && opts.tools.is_empty();
    match phase {
        WorkCheckPhase::Iteration => {
            if catalog.contract_version() < 6 {
                bail!(
                    "iteration work checks require repository contract version 6 or later; upgrade the repository contract before configuring work.iteration_profile"
                );
            }
            let profile = ctx.work_iteration_profile().ok_or_else(|| {
                anyhow!(
                    "work.iteration_profile is not configured; add it with an existing [[repository.profiles]] id or use --phase final"
                )
            })?;
            let spec = catalog.profile(profile).ok_or_else(|| {
                anyhow!(
                    "work.iteration_profile references unknown repository profile '{profile}'; configure an existing profile id"
                )
            })?;
            if spec.targets.is_empty() {
                bail!(
                    "work.iteration_profile '{}' contains no targets; configure at least one read-only check target",
                    spec.id
                );
            }
            crate::repository::plan_focused_check_run_with_cancellation(
                ctx,
                catalog,
                PlanRunRequest {
                    selectors: Vec::new(),
                    profile: Some(profile.to_string()),
                    affected_base: None,
                    comparison: None,
                    work_plan_id: Some(plan_id.to_owned()),
                },
                focus_arguments,
                &|| observer.cancelled(),
            )
            .map(Some)
        }
        WorkCheckPhase::Final => {
            let roots = if configured_selection {
                super::configured::configured_evidence_targets_if_any(ctx)?
            } else {
                explicit_targets.clone()
            };
            if roots.is_empty() {
                return Ok(None);
            }
            plan_run_with_cancellation(
                ctx,
                catalog,
                PlanRunRequest {
                    selectors: roots.iter().map(ToString::to_string).collect(),
                    profile: None,
                    affected_base: None,
                    comparison: None,
                    work_plan_id: Some(plan_id.to_owned()),
                },
                &|| observer.cancelled(),
            )
            .map(Some)
        }
    }
}

fn phase_selection(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    opts: &WorkCheckRequest,
    phase: WorkCheckPhase,
) -> Result<(Vec<SelectedCheck>, BTreeSet<jig_contract::TargetId>)> {
    if phase == WorkCheckPhase::Iteration {
        return Ok((Vec::new(), BTreeSet::new()));
    }
    if opts.gates.is_empty() {
        return Ok((selected_checks(ctx, &[], &opts.tools)?, BTreeSet::new()));
    }
    if !opts.tools.is_empty() {
        bail!("Work check accepts either gate ids or tool names, not both");
    }
    let configured = ctx.work_gates();
    let mut checks = Vec::new();
    let mut targets = BTreeSet::new();
    for id in opts.gates.iter().collect::<BTreeSet<_>>() {
        let gate = configured
            .iter()
            .find(|gate| gate.id() == id)
            .ok_or_else(|| anyhow!("Unknown configured check gate id: {id}"))?;
        match gate {
            crate::context::WorkGate::Check(gate) => {
                validate_check_tool(ctx, &gate.tool, "Work check")?;
                checks.push(SelectedCheck::Gate {
                    gate: gate.clone(),
                    force: true,
                });
            }
            crate::context::WorkGate::Evidence(gate) => {
                targets.extend(resolve_evidence_targets(catalog, &gate.selector)?);
            }
            crate::context::WorkGate::CodexReview(_) => bail!(
                "Gate {id} is a review gate; use work review --plan-id {} --gate {id}",
                opts.plan_id
            ),
            crate::context::WorkGate::Unsupported(_) => {
                bail!("Unsupported configured check gate id: {id}")
            }
        }
    }
    Ok((checks, targets))
}

fn preview_check_batch(batch: &PreparedCheckBatch) -> Vec<Value> {
    batch
        .checks
        .iter()
        .map(
            |check| match classify_prepared_check(check, FailureMode::Collect) {
                PreparedCheckAction::Evidence(evidence) => json!({
                    "tool": check.tool(),
                    "gate_id": evidence.gate_id,
                    "disposition": match evidence.status.as_str() {
                        "unknown" => "unavailable",
                        other => other,
                    },
                    "evidence": evidence,
                }),
                PreparedCheckAction::Run(runnable) => json!({
                    "tool": runnable.name,
                    "gate_id": runnable.gate_id,
                    "disposition": "selected",
                    "evidence": null,
                }),
                PreparedCheckAction::Abort { .. } => {
                    unreachable!("collect mode reports unavailable checks without aborting")
                }
            },
        )
        .collect()
}

impl PreparedCheck {
    fn tool(&self) -> &str {
        match self {
            Self::Gate { gate, .. } => &gate.tool,
            Self::Tool(tool) => tool,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn attach_phase_report(
    result: &mut Value,
    phase: WorkCheckPhase,
    explain: bool,
    selection: Option<&jig_contract::RunPlan>,
    snapshot: Option<&super::super::gates::SelectedInvocationSnapshot>,
    scheduled: &BTreeSet<jig_contract::TargetId>,
    execution_result: Option<&Value>,
    selected_checks: Vec<Value>,
    final_value: Value,
) -> Result<()> {
    let execution_evidence = execution_result
        .and_then(|value| value["target_evidence"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| {
            let target =
                serde_json::from_value::<jig_contract::TargetId>(value.get("target")?.clone())
                    .ok()?;
            Some((target, value.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let snapshot_evidence = snapshot
        .into_iter()
        .flat_map(|snapshot| snapshot.targets.iter())
        .map(|target| (target.target().clone(), target.to_value()))
        .collect::<BTreeMap<_, _>>();
    let selected_invocations = selection
        .into_iter()
        .flat_map(|plan| plan.targets.iter())
        .map(|invocation| {
            let evidence = execution_evidence
                .get(&invocation.target)
                .or_else(|| snapshot_evidence.get(&invocation.target))
                .cloned()
                .unwrap_or(Value::Null);
            let disposition = evidence
                .get("disposition")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    if snapshot
                        .is_some_and(|snapshot| snapshot.unavailable.contains(&invocation.target))
                    {
                        "unavailable".to_owned()
                    } else if scheduled.contains(&invocation.target) {
                        "selected".to_owned()
                    } else {
                        "reused".to_owned()
                    }
                });
            SelectedInvocationReport {
                target: invocation.target.clone(),
                invocation: invocation.clone(),
                selection_reasons: invocation.reasons.clone(),
                evidence_validity: evidence,
                disposition,
            }
        })
        .collect::<Vec<_>>();

    let final_requirements = final_value["gates"].as_array().cloned().unwrap_or_default();
    let pending_final_requirements = pending_final_requirements(&final_requirements);
    let report = WorkCheckPhaseReport {
        phase,
        explain,
        selected_invocations,
        selected_checks,
        final_requirements,
        pending_final_requirements,
        final_gates_ok: final_value["gates_ok"].as_bool().unwrap_or(false),
        final_recovery: final_value["recovery"].clone(),
    };
    let Value::Object(fields) = serde_json::to_value(report)? else {
        unreachable!("work-check phase report serializes as an object")
    };
    result
        .as_object_mut()
        .ok_or_else(|| anyhow!("work check result was not a JSON object"))?
        .extend(fields);
    Ok(())
}

fn selected_check_evidence_is_current(selected_checks: &[Value], final_report: &Value) -> bool {
    selected_checks
        .iter()
        .filter_map(|check| check["gate_id"].as_str())
        .all(|selected| {
            final_report["gates"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|gate| gate["id"] == selected)
                .is_some_and(|gate| {
                    matches!(
                        gate["status"].as_str(),
                        Some("passed" | "reused" | "not_applicable")
                    )
                })
        })
}

fn pending_final_requirements(final_requirements: &[Value]) -> Vec<Value> {
    final_requirements
        .iter()
        .filter(|gate| {
            gate["required"] == true
                && !matches!(
                    gate["status"].as_str(),
                    Some("passed" | "reused" | "not_applicable")
                )
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        pending_final_requirements, selected_check_evidence_is_current,
        source_authority_changed_or_unavailable,
    };
    use crate::state::CurrentWorktreeFingerprint;
    use serde_json::{Value, json};

    #[test]
    fn pending_final_requirements_preserve_every_required_failure_status() {
        let requirements = [
            ("passed", true),
            ("reused", true),
            ("not_applicable", true),
            ("missing", true),
            ("failed", true),
            ("stale", true),
            ("unknown", true),
            ("unsupported", true),
            ("failed", false),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (status, required))| {
            json!({"id": format!("gate-{index}"), "status": status, "required": required})
        })
        .collect::<Vec<Value>>();

        assert_eq!(
            pending_final_requirements(&requirements)
                .iter()
                .map(|gate| gate["status"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["missing", "failed", "stale", "unknown", "unsupported"]
        );
    }

    #[test]
    fn unavailable_source_authority_is_never_stable() {
        let missing = CurrentWorktreeFingerprint {
            fingerprint: None,
            error: Some("unavailable".into()),
        };
        let known = CurrentWorktreeFingerprint {
            fingerprint: Some("sha256:known".into()),
            error: None,
        };

        assert!(source_authority_changed_or_unavailable(&missing, &missing));
        assert!(source_authority_changed_or_unavailable(&missing, &known));
        assert!(!source_authority_changed_or_unavailable(&known, &known));
    }

    #[test]
    fn selected_gate_requires_current_post_execution_evidence() {
        let selected = [json!({"gate_id": "example"})];

        assert!(selected_check_evidence_is_current(
            &selected,
            &json!({"gates": [{"id": "example", "status": "passed"}]})
        ));
        assert!(!selected_check_evidence_is_current(
            &selected,
            &json!({"gates": [{"id": "example", "status": "unknown"}]})
        ));
    }
}
