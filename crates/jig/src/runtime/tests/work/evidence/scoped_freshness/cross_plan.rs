use super::*;

fn open_plan(ctx: &RepoContext) -> String {
    crate::state::plans_open(
        ctx,
        crate::state::PlanOpenRequest {
            title: "Example follow-up validation".into(),
            body: None,
            body_file: None,
            base: Some("HEAD".into()),
        },
    )
    .unwrap()["plan_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn gates(ctx: &RepoContext, plan: &str) -> Value {
    crate::runtime::call_tool(
        ctx,
        crate::tool_defs::tool::WORK_GATES,
        json!({"plan_id": plan, "freshness_timeout_ms": 30_000}),
    )
    .unwrap()
}

fn check(ctx: &RepoContext, plan: &str) -> Value {
    dispatch(
        ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: plan.into(),
            gates: vec![],
            tools: vec![],
        })),
    )
    .unwrap()
}

fn finish(ctx: &RepoContext, plan: &str) {
    dispatch(
        ctx,
        CommandKind::Work(crate::cli::WorkCommand::Finish(
            crate::cli::WorkFinishOpts {
                plan_id: plan.into(),
                resolution: Some("Example checks complete".into()),
                outcome: Some("success".into()),
            },
        )),
    )
    .unwrap();
}

fn journal(ctx: &RepoContext) -> Vec<Value> {
    fs::read_to_string(ctx.state_file("receipts.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn target_receipts(records: &[Value]) -> Vec<Value> {
    records
        .iter()
        .filter(|r| r["target"].is_object())
        .cloned()
        .collect()
}

fn assert_reused_original(old: &Value, new: &Value) {
    assert_eq!(old["receipt_id"], new["receipt_id"]);
    assert_eq!(old["run_id"], new["run_id"]);
    assert_eq!(new["original_plan_id"], "plan_1");
    assert_eq!(new["disposition"], "reused");
}

#[test]
fn cross_plan_live_reuse_preserves_originals_and_binds_validation_to_consumer() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), true, true);
    let first = check(&ctx, "plan_1");
    assert_eq!(first["ok"], true, "{first:#}");
    let originals = target_receipts(&journal(&ctx));
    assert_eq!(originals.len(), 2);
    finish(&ctx, "plan_1");
    let next = open_plan(&ctx);
    fs::write(ctx.root().join("notes.md"), "Example follow-up notes\n").unwrap();
    let before = fs::read(ctx.state_file("receipts.jsonl")).unwrap();
    let report = gates(&ctx, &next);
    assert_eq!(report["overall"], "passed", "{report:#}");
    // Read-only discovery neither copies execution receipts nor records a binding.
    assert_eq!(before, fs::read(ctx.state_file("receipts.jsonl")).unwrap());
    let reused = check(&ctx, &next);
    assert_eq!(reused["ok"], true, "{reused:#}");
    assert!(reused["run"].is_null(), "{reused:#}");
    assert!(reused["results"].as_array().unwrap().is_empty());
    for (old, new) in first["target_evidence"]
        .as_array()
        .unwrap()
        .iter()
        .zip(reused["target_evidence"].as_array().unwrap())
    {
        assert_reused_original(old, new);
    }
    let records = journal(&ctx);
    assert_eq!(target_receipts(&records), originals);
    let binding = records
        .iter()
        .find(|r| r["id"] == reused["target_validation_receipt_id"])
        .unwrap();
    assert_eq!(binding["plan_id"], next);
    assert_eq!(binding["evidence"]["targets"], reused["target_evidence"]);
    assert!(binding["target"].is_null());
    assert!(binding["run_id"].is_null());
    let snapshot =
        crate::status::snapshot_with_freshness_timeout(&ctx, &|| false, Some(30_000)).unwrap();
    let dashboard_target = &snapshot["work"]["gates"][0]["snapshot"]["gates"][0]["targets"][0];
    assert_eq!(
        dashboard_target["original_plan_id"], "plan_1",
        "{snapshot:#}"
    );
    assert_eq!(
        dashboard_target["receipt_id"],
        first["target_evidence"][0]["receipt_id"]
    );
    assert_eq!(dashboard_target["status"], "passed");
    finish(&ctx, &next);
}

#[test]
fn cross_plan_finish_rejects_newer_foreign_failure_after_successful_reuse() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, false);
    let first = check(&ctx, "plan_1");
    assert_eq!(first["ok"], true, "{first:#}");
    let next = open_plan(&ctx);
    let reused = check(&ctx, &next);
    assert_eq!(reused["ok"], true, "{reused:#}");
    assert!(reused["run"].is_null(), "{reused:#}");
    assert_reused_original(&first["target_evidence"][0], &reused["target_evidence"][0]);

    let foreign = open_plan(&ctx);
    let mut failure = target_receipts(&journal(&ctx)).pop().unwrap();
    failure["id"] = json!("receipt_example_newer_failure");
    failure["run_id"] = json!("run_example_newer_failure");
    failure["plan_id"] = json!(foreign);
    failure["ended_at_ms"] = json!(failure["ended_at_ms"].as_u64().unwrap() + 1);
    failure["exit_status"] = json!(1);
    append(&ctx, [failure.clone()]);
    let plans_before = fs::read(ctx.state_file("plans.jsonl")).unwrap();
    // Finish must independently recheck originals, not trust B's passing batch.
    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Finish(
            crate::cli::WorkFinishOpts {
                plan_id: next.clone(),
                resolution: Some("Example checks complete".into()),
                outcome: Some("success".into()),
            },
        )),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Required work gates are not satisfied"),
        "{error:#}"
    );
    crate::state::ensure_plan_is_open(&ctx, &next).unwrap();
    assert_eq!(
        plans_before,
        fs::read(ctx.state_file("plans.jsonl")).unwrap()
    );
    let report = gates(&ctx, &next);
    assert_eq!(
        report["gates"][0]["targets"][0]["status"], "failed",
        "{report:#}"
    );
    assert_eq!(
        report["gates"][0]["targets"][0]["receipt_id"],
        failure["id"]
    );
}

#[test]
fn cross_plan_changed_inputs_rerun_only_affected_independent_target() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, true);
    let first = check(&ctx, "plan_1");
    let next = open_plan(&ctx);
    fs::write(ctx.root().join("api/example.go"), "package changed\n").unwrap();
    let checked = check(&ctx, &next);
    assert_eq!(checked["ok"], true, "{checked:#}");
    assert_eq!(checked["run"]["targets"].as_array().unwrap().len(), 1);
    assert_eq!(checked["target_evidence"][0]["disposition"], "executed");
    assert_eq!(checked["target_evidence"][0]["original_plan_id"], next);
    assert_eq!(checked["target_evidence"][1]["disposition"], "reused");
    assert_eq!(
        checked["target_evidence"][1]["receipt_id"],
        first["target_evidence"][1]["receipt_id"]
    );
    assert_eq!(checked["target_evidence"][1]["original_plan_id"], "plan_1");
    assert!(check(&ctx, &next)["run"].is_null());
}

#[test]
fn cross_plan_newest_outcomes_block_older_passes_and_survive_archive() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, false);
    let original = original_records(&ctx)[&"web:test".parse().unwrap()].clone();
    append(&ctx, [original.clone()]);
    let next = open_plan(&ctx);
    assert_eq!(gates(&ctx, &next)["overall"], "passed");
    for (index, (field, value, expected)) in [
        ("exit_status", json!(1), "failed"),
        (
            "target_freshness",
            json!({"schema_version": 99}),
            "unsupported",
        ),
        ("target_freshness", Value::Null, "unknown"),
        ("run_id", Value::Null, "unknown"),
        ("expired", json!(0), "stale"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut newer = original.clone();
        newer["id"] = json!(format!("receipt_newest_{index}"));
        newer["plan_id"] = json!(format!("plan_other_{index}"));
        newer["ended_at_ms"] = json!(100 + index);
        if field == "expired" {
            newer["valid_until_ms"] = value.clone();
            newer["target_freshness"]["effective_valid_until_ms"] = value;
            newer["target_freshness"]["effective_requires_time_validity"] = json!(true);
        } else {
            newer[field] = value;
        }
        append(&ctx, [newer.clone()]);
        let report = gates(&ctx, &next);
        assert_eq!(report["overall"], "blocked", "{report:#}");
        assert_eq!(
            report["gates"][0]["targets"][0]["status"], expected,
            "{report:#}"
        );
        assert_eq!(report["gates"][0]["targets"][0]["receipt_id"], newer["id"]);
    }
    crate::state::receipts_archive(
        &ctx,
        crate::state::StateArchiveRequest {
            before: "1000".into(),
            dry_run: false,
        },
    )
    .unwrap();
    let after = gates(&ctx, &next);
    assert_eq!(
        after["gates"][0]["targets"][0]["receipt_id"],
        "receipt_newest_4"
    );
    assert_eq!(after["gates"][0]["targets"][0]["status"], "stale");
}

#[test]
fn cross_plan_archive_keeps_closed_plan_original_dependency_graph_without_open_plans() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), true, false);
    let originals = original_records(&ctx);
    append(&ctx, originals.values().cloned());
    finish(&ctx, "plan_1");
    let archived = crate::state::receipts_archive(
        &ctx,
        crate::state::StateArchiveRequest {
            before: "1000".into(),
            dry_run: false,
        },
    )
    .unwrap();
    assert_eq!(archived["protected_receipts_retained"], 2, "{archived:#}");
    let next = open_plan(&ctx);
    assert_eq!(gates(&ctx, &next)["overall"], "passed");
    assert!(check(&ctx, &next)["run"].is_null());
    for original in originals.values() {
        assert!(journal(&ctx).contains(original));
    }
}

#[test]
fn cross_plan_legacy_contract_stays_plan_local() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(
        temp.path(),
        "[[work.gates]]\nid = \"verify\"\nkind = \"evidence\"\nprofile = \"verify\"\n",
    );
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    check(&ctx, "plan_1");
    let next = open_plan(&ctx);
    assert_eq!(gates(&ctx, &next)["gates"][0]["status"], "missing");
    let result = check(&ctx, &next);
    assert_eq!(result["run"]["targets"].as_array().unwrap().len(), 2);
}

#[test]
fn cross_plan_native_comparison_authority_is_not_promoted_to_another_plan() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), true, true);
    native::configure_native(
        &ctx,
        "version=1\n[[rules]]\nid=\"source\"\ninclude=[\"api/**\"]\nmax_lines=100\n",
    );
    run_git(ctx.root(), &["add", "."]);
    run_git(ctx.root(), &["commit", "-qm", "Example native authority"]);
    let ctx = RepoContext::load_from(ctx.root()).unwrap();
    let original_plan = open_plan(&ctx);
    assert_eq!(check(&ctx, &original_plan)["ok"], true);
    let same_baseline_plan = open_plan(&ctx);
    let before = gates(&ctx, &same_baseline_plan);
    assert_eq!(before["overall"], "blocked", "{before:#}");
    assert_eq!(
        before["gates"][0]["targets"][0]["status"], "missing",
        "{before:#}"
    );
    let checked = check(&ctx, &same_baseline_plan);
    assert_eq!(checked["run"]["targets"].as_array().unwrap().len(), 2);
    assert_eq!(checked["ok"], true);
    // A native check in the new plan must not make the original plan rerun.
    assert_eq!(gates(&ctx, &original_plan)["overall"], "passed");
    assert!(check(&ctx, &original_plan)["run"].is_null());
    fs::write(ctx.root().join("api/example.go"), "package changed\n").unwrap();
    run_git(ctx.root(), &["add", "api/example.go"]);
    run_git(ctx.root(), &["commit", "-qm", "Example changed baseline"]);
    let changed_baseline_plan = open_plan(&ctx);
    assert_eq!(gates(&ctx, &changed_baseline_plan)["overall"], "blocked");
}
