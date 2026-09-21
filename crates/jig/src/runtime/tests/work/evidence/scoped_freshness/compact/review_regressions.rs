use super::*;

#[test]
fn compact_recovery_inspects_unknown_targets_even_when_newer_failure_dominates() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(
        temp.path(),
        "[[work.gates]]\nid = 'verify'\nkind = 'evidence'\nprofile = 'verify'\n",
    );
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    assert_eq!(check(&ctx)["ok"], true);
    let records: Vec<Value> = fs::read_to_string(ctx.state_file("receipts.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .filter(|record: &Value| record["target"].is_object())
        .collect();
    let latest = records
        .iter()
        .map(|r| r["ended_at_ms"].as_u64().unwrap())
        .max()
        .unwrap();
    let mut unknown = records
        .iter()
        .find(|r| r["target"]["component"] == "web")
        .unwrap()
        .clone();
    unknown["id"] = json!("receipt_example_unknown");
    unknown["ended_at_ms"] = json!(latest + 1);
    unknown.as_object_mut().unwrap().remove("input_digest");
    let mut failed = records
        .iter()
        .find(|r| r["target"]["component"] == "api")
        .unwrap()
        .clone();
    failed["id"] = json!("receipt_example_failed");
    failed["ended_at_ms"] = json!(latest + 2);
    failed["exit_status"] = json!(7);
    append(&ctx, [unknown, failed]);
    let before = fs::read(ctx.state_file("receipts.jsonl")).unwrap();
    let summary = inspect(&ctx);
    assert_eq!(summary["gates"][0]["status"], "failed");
    assert_eq!(summary["gates"][0]["freshness"], "unknown");
    assert_eq!(summary["finish_ready"], false);
    assert_eq!(summary["next_step"]["read_only"], true, "{summary:#}");
    assert_eq!(summary["next_step"]["argv"][2], "gates");
    assert_eq!(before, fs::read(ctx.state_file("receipts.jsonl")).unwrap());
}

#[test]
fn compact_mixed_legacy_selection_retains_each_ungated_tool_and_failure() {
    for exit_status in [0, 7] {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .contract_version(5)
            .config(format!(
                "[commands]\nfirst_check_command = 'true'\nsecond_check_command = 'exit {exit_status}'\n\
                 [[work.gates]]\nid = 'first'\nkind = 'check'\ntool = 'jig.first'\n"
            ))
            .required_commands(["first_check_command", "second_check_command"])
            .tool(json!({"name":"jig.first", "kind":"command", "description":"Example gated check", "command":"first_check_command"}))
            .tool(json!({"name":"jig.second", "kind":"command", "description":"Example ungated check", "command":"second_check_command"}))
            .write();
        write_open_plan(temp.path());
        init_git_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let summary = agent(
            &ctx,
            tool::WORK_CHECK,
            json!({
                "plan_id":"plan_1", "tools":["jig.first", "jig.second"]
            }),
        );
        assert_eq!(summary["ok"], exit_status == 0);
        assert_eq!(summary["activity_count"], 2, "{summary:#}");
        assert_eq!(summary["activity_truncated"], false);
        let activity = summary["activity"].as_array().unwrap();
        assert_eq!(
            activity.iter().filter(|r| r["subject"] == "first").count(),
            1
        );
        let ungated = activity
            .iter()
            .find(|r| r["subject"] == "jig.second")
            .unwrap();
        assert_eq!(ungated["disposition"], "executed");
        assert_eq!(
            ungated["status"],
            if exit_status == 0 { "passed" } else { "failed" }
        );
    }
}
