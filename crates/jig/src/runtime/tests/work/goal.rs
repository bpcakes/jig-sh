use super::*;

#[test]
fn work_goal_json_preserves_blocked_exit_when_acceptance_is_infeasible() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = call_tool(
        &ctx,
        crate::tool_defs::tool::WORK_GOAL,
        json!({
            "objective": "Update ExampleProject without changing its public contract",
            "success": "The update passes compatibility checks",
            "validations": ["Run compatibility checks"],
            "constraints": ["Preserve the public contract"]
        }),
    )
    .unwrap();
    let body_path = output["plan"]["body_path"].as_str().unwrap();
    let body = fs::read_to_string(temp.path().join(body_path)).unwrap();
    let prompt = output["goal_prompt"].as_str().unwrap();

    for (surface, text) in [("body", body.as_str()), ("prompt", prompt)] {
        for required in [
            "Stop and report a blocker if acceptance or required checks cannot be satisfied",
            "without changing the objective, success condition, constraints, or configured gates",
            "or would require unsafe permissions",
            "Record the evidence and the decision or authority needed to proceed",
            "do not weaken checks or redefine success",
        ] {
            assert!(text.contains(required), "{surface} missing: {required}");
        }
    }
}

#[test]
fn work_goal_json_preserves_planning_scope_and_explicit_approval_checkpoints() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = call_tool(
        &ctx,
        crate::tool_defs::tool::WORK_GOAL,
        json!({
            "objective": "  Plan the ExampleProject migration; do not implement it.  ",
            "success": "A reviewed plan\nwith rollout and recovery",
            "validations": ["  Inspect old-reader compatibility  ", "Review recovery steps"],
            "constraints": ["Do not edit application code", "No deployment without approval"],
            "checkpoints": ["  Draft the migration plan  ", "Stop for user approval before implementation"],
            "notes": "Preserve the existing record IDs."
        }),
    )
    .unwrap();
    let body_path = output["plan"]["body_path"].as_str().unwrap();
    let body = fs::read_to_string(temp.path().join(body_path)).unwrap();
    let prompt = output["goal_prompt"].as_str().unwrap();

    for supplied in [
        "Plan the ExampleProject migration; do not implement it.",
        "A reviewed plan\nwith rollout and recovery",
        "- Inspect old-reader compatibility\n- Review recovery steps",
        "- Do not edit application code\n- No deployment without approval",
        "- [ ] Draft the migration plan\n- [ ] Stop for user approval before implementation",
        "Preserve the existing record IDs.",
    ] {
        assert!(
            body.contains(supplied),
            "missing supplied content: {supplied}"
        );
    }
    assert_eq!(body.matches("- [ ] ").count(), 2);
    assert!(prompt.starts_with("/goal "));
    assert!(prompt.contains(body_path));
    assert!(prompt.contains("A reviewed plan with rollout and recovery"));
    assert!(prompt.contains("explicit approval checkpoints"));
    assert!(prompt.contains("a planning objective remains planning"));
    assert!(prompt.contains("required authority"));
    assert!(body.contains("ordinary progress checkpoints do not require renewed permission"));
    assert!(body.contains("continue independent authorized work"));
    assert!(body.contains("Repeat or broaden checks only for changed inputs"));
    assert!(body.contains("actual worktree and evidence"));
}

#[test]
fn work_goal_json_accepts_missing_and_null_optional_lists() {
    for optional_fields in [json!({}), json!({"checkpoints": null, "constraints": null})] {
        let temp = tempdir().unwrap();
        write_fixture_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut request = json!({
            "objective": "Plan an ExampleProject update",
            "success": "The plan describes an observable acceptance check",
            "validations": ["Review the plan"]
        });
        request
            .as_object_mut()
            .unwrap()
            .extend(optional_fields.as_object().unwrap().clone());
        let output = call_tool(&ctx, crate::tool_defs::tool::WORK_GOAL, request).unwrap();
        let body_path = output["plan"]["body_path"].as_str().unwrap();
        let body = fs::read_to_string(temp.path().join(body_path)).unwrap();
        assert!(body.contains("- [ ] The plan describes an observable acceptance check"));
        assert_eq!(body.matches("- [ ] ").count(), 1);
        assert!(body.contains("custom: check (jig.custom_check)"));
        assert_eq!(output["ok"], true);
    }
}

#[test]
fn work_goal_json_rejects_invalid_validation_contract_before_opening_work() {
    for invalid_fields in [
        json!({"validations": null}),
        json!({"validations": []}),
        json!({"validations": [" "]}),
        json!({"checkpoints": [" "]}),
        json!({"constraints": [" "]}),
    ] {
        let temp = tempdir().unwrap();
        write_fixture_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let plans_path = ctx.state_file("plans.jsonl");
        let plans_before = fs::read(&plans_path).unwrap_or_default();
        let mut request = json!({
            "objective": "Plan an ExampleProject update",
            "success": "The plan is reviewed",
            "validations": ["Review the plan"]
        });
        request
            .as_object_mut()
            .unwrap()
            .extend(invalid_fields.as_object().unwrap().clone());
        assert!(call_tool(&ctx, crate::tool_defs::tool::WORK_GOAL, request).is_err());
        assert_eq!(crate::state::current_session(&ctx).unwrap(), None);
        assert_eq!(fs::read(&plans_path).unwrap_or_default(), plans_before);
    }
}
