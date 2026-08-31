impl PreparedCheck {
    fn should_run(&self) -> bool {
        match self {
            Self::Tool(_) => true,
            Self::Gate { force: true, .. } => true,
            Self::Gate {
                scope, reusable, ..
            } => {
                scope.error().is_none()
                    && reusable.is_none()
                    && scope.applicability()
                        != Some(crate::git_receipts::GateApplicability::NotApplicable)
            }
        }
    }
}

fn gate_evidence_from_scope(
    gate: &crate::context::WorkCheckGate,
    status: &str,
    scope: &GateScopeEvaluation,
    tool_receipt_id: Option<String>,
    exit_status: Option<i32>,
    forced: bool,
    reusable: Option<&ReusableWorkCheckEvidence>,
) -> WorkCheckGateEvidence {
    WorkCheckGateEvidence {
        gate_id: gate.id.clone(),
        tool: gate.tool.clone(),
        status: status.into(),
        applicability: scope
            .applicability()
            .map(crate::git_receipts::GateApplicability::as_str)
            .unwrap_or("unknown")
            .into(),
        required: gate.required,
        paths: gate.paths.clone(),
        paths_ignore: gate.paths_ignore.clone(),
        reuse: gate.reuse,
        forced,
        gate_signature: scope.gate_signature().to_string(),
        baseline_oid: scope.baseline_oid().map(str::to_string),
        reason: if forced {
            format!("gate was explicitly force-run; {}", scope.reason())
        } else {
            scope.reason().to_string()
        },
        changed_paths: Vec::new(),
        changed_path_count: 0,
        changed_paths_truncated: false,
        changed_paths_digest: None,
        matching_paths: scope.matching_paths().to_vec(),
        matching_path_count: scope.matching_path_count(),
        matching_paths_truncated: scope.matching_paths_truncated(),
        matching_paths_digest: scope.matching_paths_digest().map(str::to_string),
        scope_fingerprint: scope.scope_fingerprint().map(str::to_string),
        scope_error: scope.error().map(str::to_string),
        tool_receipt_id,
        exit_status,
        source_plan_id: reusable.map(|source| source.source_plan_id.clone()),
        source_batch_receipt_id: reusable.map(|source| source.source_batch_receipt_id.clone()),
        source_tool_receipt_id: reusable.map(|source| source.source_tool_receipt_id.clone()),
        valid_until_ms: reusable.and_then(|source| source.valid_until_ms),
        requires_time_validity: reusable.is_some_and(|source| source.requires_time_validity),
    }
}

fn gate_interruption_evidence(
    gate: &crate::context::WorkCheckGate,
    status: &str,
    scope: &GateScopeEvaluation,
    tool_receipt_id: Option<String>,
    exit_status: i32,
    forced: bool,
    interruption: &str,
) -> WorkCheckGateEvidence {
    let mut evidence = gate_evidence_from_scope(
        gate,
        status,
        scope,
        tool_receipt_id,
        Some(exit_status),
        forced,
        None,
    );
    evidence.reason = format!("{interruption}; {}", evidence.reason);
    evidence.scope_error = Some(interruption.to_string());
    evidence
}

fn work_check_fingerprint_evidence(
    before: &crate::state::CurrentWorktreeFingerprint,
    after: &crate::state::CurrentWorktreeFingerprint,
) -> std::result::Result<String, String> {
    let before = before
        .fingerprint
        .as_deref()
        .ok_or_else(|| fingerprint_error("before work check", before.error.as_deref()))?;
    let after = after
        .fingerprint
        .as_deref()
        .ok_or_else(|| fingerprint_error("after work check", after.error.as_deref()))?;

    if before == after {
        Ok(after.to_string())
    } else {
        Err(format!(
            "worktree changed during work check; before fingerprint {before}, after fingerprint {after}; rerun work check after generated changes settle"
        ))
    }
}

fn fingerprint_error(stage: &str, error: Option<&str>) -> String {
    match error {
        Some(error) => format!("Failed to collect worktree fingerprint {stage}: {error}"),
        None => format!("Failed to collect worktree fingerprint {stage}"),
    }
}

fn result_time_validity(result: &Value) -> (Option<u64>, bool) {
    let nested_result = result.get("result");
    let run = result.get("run");
    let nested_run = nested_result.and_then(|value| value.get("run"));
    let objects = [Some(result), nested_result, run, nested_run];
    let direct = objects
        .into_iter()
        .flatten()
        .filter_map(|value| value.get("valid_until_ms").and_then(Value::as_u64));
    let targets = objects.into_iter().flatten().flat_map(|value| {
        value
            .get("targets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|target| target.get("valid_until_ms").and_then(Value::as_u64))
    });
    let valid_until_ms = direct.chain(targets).min();
    let requires_time_validity = objects.into_iter().flatten().any(|value| {
        value
            .get("requires_time_validity")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }) || valid_until_ms.is_some();
    (valid_until_ms, requires_time_validity)
}

fn revalidate_gate_scopes(
    ctx: &RepoContext,
    plan_id: &str,
    initial: &[(crate::context::WorkCheckGate, GateScopeEvaluation)],
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<(), String> {
    if initial.is_empty() {
        return Ok(());
    }
    let final_context = PlanGateContext::load_with_cancellation(ctx, plan_id, cancelled)
        .map_err(|error| format!("Failed to reload work gate scopes after checks: {error:#}"))?;
    for (gate, before) in initial {
        let after = final_context.evaluate_with_cancellation(ctx, gate, cancelled);
        if &after != before {
            return Err(format!(
                "work gate '{}' scope changed during work check; rerun after repository inputs settle",
                gate.id
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod compatibility_tests {
    use serde_json::json;

    use super::{EMPTY_CHECK_SELECTION_MESSAGE, result_time_validity};

    #[test]
    fn empty_selection_guidance_is_truthful_for_legacy_and_current_contracts() {
        assert!(EMPTY_CHECK_SELECTION_MESSAGE.contains("check gate"));
        assert!(EMPTY_CHECK_SELECTION_MESSAGE.contains("--gate"));
        assert!(EMPTY_CHECK_SELECTION_MESSAGE.contains("--tool"));
        assert!(!EMPTY_CHECK_SELECTION_MESSAGE.contains("required"));
        assert!(!EMPTY_CHECK_SELECTION_MESSAGE.contains("optional"));
    }

    #[test]
    fn batch_validity_uses_the_earliest_nested_target_boundary() {
        assert_eq!(
            result_time_validity(&json!({
                "run": {
                    "targets": [
                        {"valid_until_ms": 80},
                        {"valid_until_ms": 40}
                    ]
                }
            })),
            (Some(40), true)
        );
        assert_eq!(
            result_time_validity(&json!({
                "result": {
                    "run": {"targets": [{"valid_until_ms": 60}]},
                    "valid_until_ms": 50
                }
            })),
            (Some(50), true)
        );
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use tempfile::tempdir;

    use super::*;
    use crate::context::WorkGate;

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {}", args.join(" "));
    }

    #[test]
    fn scope_revalidation_rejects_inputs_changed_after_initial_classification() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("src")).unwrap();
        fs::write(temp.path().join("src/lib.rs"), "pub const V: u8 = 1;\n").unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
            .config(
                r#"
[[work.gates]]
id = "source"
kind = "check"
tool = "jig.contract_check"
paths = ["src/**"]
"#,
            )
            .tool(serde_json::json!({
                "name": "jig.contract_check",
                "kind": "native",
                "description": "Check Jig contract."
            }))
            .write();
        git(temp.path(), &["init", "-q"]);
        git(temp.path(), &["config", "user.name", "Fixture"]);
        git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        git(temp.path(), &["add", "."]);
        git(temp.path(), &["commit", "-m", "baseline", "-q"]);
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let plan = crate::state::plans_open(
            &ctx,
            crate::state::PlanOpenRequest {
                title: "Scope stability".into(),
                body: Some("Verify scope revalidation".into()),
                body_file: None,
                base: None,
            },
        )
        .unwrap();
        let plan_id = plan["plan_id"].as_str().unwrap();
        fs::write(temp.path().join("src/lib.rs"), "pub const V: u8 = 2;\n").unwrap();
        let initial_context = PlanGateContext::load(&ctx, plan_id).unwrap();
        let WorkGate::Check(gate) = ctx.work_gates().remove(0) else {
            panic!("expected check gate");
        };
        let initial = initial_context.evaluate(&ctx, &gate);

        fs::write(temp.path().join("src/lib.rs"), "pub const V: u8 = 3;\n").unwrap();
        let final_scope = PlanGateContext::load(&ctx, plan_id)
            .unwrap()
            .evaluate(&ctx, &gate);
        assert_ne!(initial, final_scope, "gate scope must track source bytes");
        let error =
            revalidate_gate_scopes(&ctx, plan_id, &[(gate, initial)], &|| false).unwrap_err();

        assert!(error.contains("scope changed during work check"), "{error}");
    }
}
