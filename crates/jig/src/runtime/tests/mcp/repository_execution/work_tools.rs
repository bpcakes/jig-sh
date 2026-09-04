use super::*;

#[test]
fn mcp_work_tools_deserialize_typed_arguments() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = call_tool(
        &ctx,
        tool::WORK_START,
        json!({
            "title": "Typed MCP request",
            "body": "Use serde for tool arguments"
        }),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert!(output["plan"]["plan_id"].as_str().is_some());
}

#[test]
fn mcp_work_append_rejects_blank_progress_without_mutating_plan() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_path = crate::state::plan_body_path(&ctx, "plan_1").unwrap();
    let body_before = fs::read_to_string(&plan_path).unwrap();
    let state_before = crate::state::state_summary(&ctx).unwrap();

    let error = call_tool(
        &ctx,
        tool::WORK_APPEND,
        json!({
            "plan_id": "plan_1",
            "body": " \n\t "
        }),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Progress text must not be empty"));
    assert_eq!(fs::read_to_string(plan_path).unwrap(), body_before);
    assert_eq!(crate::state::state_summary(&ctx).unwrap(), state_before);
}

#[test]
fn mcp_work_tools_tolerate_null_optional_defaults() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let check = call_tool(
        &ctx,
        tool::WORK_CHECK,
        json!({
            "plan_id": "plan_1",
            "tools": null
        }),
    )
    .unwrap();
    let receipts = call_tool(
        &ctx,
        tool::WORK_RECEIPTS,
        json!({
            "failed_only": null,
            "limit": null
        }),
    )
    .unwrap();
    let evidence = call_tool(
        &ctx,
        tool::WORK_EVIDENCE,
        json!({
            "plan_id": null
        }),
    )
    .unwrap();

    assert_eq!(check["ok"], true);
    assert_eq!(receipts["ok"], true);
    assert_eq!(evidence["command"], "work evidence");
    assert!(!receipts["receipts"].as_array().unwrap().is_empty());
}

#[test]
fn mcp_work_check_rejects_unknown_plan_before_running_tools() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = call_tool(
        &ctx,
        tool::WORK_CHECK,
        json!({
            "plan_id": "plan_missing",
            "tools": null
        }),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Plan not found: plan_missing"));
    let receipts_path = temp.path().join(".agent/state/receipts.jsonl");
    let receipts = fs::read_to_string(receipts_path).unwrap_or_default();
    assert!(!receipts.contains("jig.custom_check"));
}

#[test]
fn mcp_work_tools_reject_invalid_typed_arguments() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = call_tool(&ctx, tool::WORK_START, json!({ "body": "missing title" })).unwrap_err();
    let error = format!("{error:#}");

    assert!(error.contains("Invalid work tool arguments"));
    assert!(error.contains("missing field `title`"));
}

#[test]
fn mcp_inspect_reconciles_a_run_whose_worker_lease_disappeared() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:test"]})).unwrap();
    let plan: jig_contract::RunPlan = serde_json::from_value(planned["plan"].clone()).unwrap();
    let (abandoned, abandoned_lease) = crate::state::start_run(&ctx, plan, None).unwrap();
    drop(abandoned_lease);

    let inspected = call_tool(
        &ctx,
        tool::INSPECT,
        json!({"kind": "run", "run_id": abandoned.result.run_id}),
    )
    .unwrap();

    assert_eq!(inspected["result"]["run"]["result"]["status"], "completed");
    assert_eq!(
        inspected["result"]["run"]["result"]["conclusion"],
        "blocked"
    );
    assert!(
        inspected["result"]["run"]["result"]["targets"][0]["findings"][0]["message"]
            .as_str()
            .unwrap()
            .contains("worker lease is no longer held")
    );
}
