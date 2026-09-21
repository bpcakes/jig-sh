use super::*;
use crate::surface::ResponseSurface;
use crate::tool_defs::tool;

mod review_regressions;

fn agent(ctx: &RepoContext, name: &str, args: Value) -> Value {
    crate::runtime::call_tool_on_surface(ctx, name, args, ResponseSurface::AgentV1).unwrap()
}

fn check(ctx: &RepoContext) -> Value {
    agent(ctx, tool::WORK_CHECK, json!({"plan_id": "plan_1"}))
}

fn inspect(ctx: &RepoContext) -> Value {
    agent(
        ctx,
        tool::WORK_GATES,
        json!({"plan_id": "plan_1", "freshness_timeout_ms": 30_000}),
    )
}

fn assert_schema(ctx: &RepoContext, name: &str, value: &Value) {
    let schema = crate::tool_defs::tool_descriptors_for_surface(
        ctx.contract_version(),
        ctx.tool_specs(),
        ResponseSurface::AgentV1,
    )
    .into_iter()
    .find(|descriptor| descriptor["name"] == name)
    .unwrap()["outputSchema"]
        .clone();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(validator.is_valid(value), "{value:#}");
    let mut invalid = value.clone();
    invalid["unexpected"] = json!(true);
    assert!(!validator.is_valid(&invalid));
}

fn set_config(ctx: &RepoContext, edit: impl FnOnce(&mut toml::Value)) -> RepoContext {
    let path = ctx.root().join(".jig.toml");
    let mut config: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    edit(&mut config);
    fs::write(path, toml::to_string(&config).unwrap()).unwrap();
    RepoContext::load_from(ctx.root()).unwrap()
}

#[test]
fn compact_check_reports_execution_reuse_and_strict_cli_mcp_parity() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, true);
    let first = check(&ctx);
    assert_eq!(first["ok"], true, "{first:#}");
    assert_eq!(first["finish_ready"], true, "{first:#}");
    assert_eq!(first["activity_count"], 2);
    assert!(
        first["activity"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["disposition"] == "executed")
    );
    assert!(first.get("target_evidence").is_none());
    assert_schema(&ctx, tool::WORK_CHECK, &first);
    let reused = check(&ctx);
    assert!(
        reused["activity"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["disposition"] == "reused")
    );
    let before = fs::read(ctx.state_file("receipts.jsonl")).unwrap();
    crate::state::reset_work_gate_receipt_index_scan_count();
    let mut mcp = inspect(&ctx);
    assert_eq!(crate::state::work_gate_receipt_index_scan_count(), 1);
    assert_schema(&ctx, tool::WORK_GATES, &mcp);
    let mut cli = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            projection: ResponseSurface::AgentV1,
            plan_id: Some("plan_1".into()),
            freshness_timeout_ms: Some(30_000),
        })),
    )
    .unwrap();
    mcp.as_object_mut().unwrap().remove("observed_at_ms");
    cli.as_object_mut().unwrap().remove("observed_at_ms");
    assert_eq!(cli, mcp);
    let evidence = agent(
        &ctx,
        tool::WORK_EVIDENCE,
        json!({"plan_id": "plan_1", "freshness_timeout_ms": 30_000}),
    );
    assert_schema(&ctx, tool::WORK_EVIDENCE, &evidence);
    assert_eq!(evidence["gates"], mcp["gates"]);
    assert_eq!(
        evidence["evidence"]["argv"],
        json!(["scripts/jig", "work", "evidence", "--plan-id", "plan_1"])
    );
    assert_eq!(before, fs::read(ctx.state_file("receipts.jsonl")).unwrap());
    let standard =
        crate::runtime::call_tool(&ctx, tool::WORK_GATES, json!({"plan_id": "plan_1"})).unwrap();
    assert_eq!(standard["gates_ok"], true);
    assert!(standard.get("finish_ready").is_none());
}

#[test]
fn compact_mixed_reuse_and_readiness_do_not_authorize_finish_after_drift() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, true);
    assert_eq!(check(&ctx)["finish_ready"], true);
    fs::write(ctx.root().join("api/example.go"), "package changed\n").unwrap();
    let stale = inspect(&ctx);
    assert_eq!(stale["finish_ready"], false);
    assert_eq!(stale["gates"][0]["status"], "stale");
    assert_eq!(stale["next_step"]["argv"][2], "check");
    let repaired = check(&ctx);
    assert_eq!(repaired["finish_ready"], true, "{repaired:#}");
    let activity = repaired["activity"].as_array().unwrap();
    assert!(
        activity
            .iter()
            .any(|row| row["subject"] == "api:test" && row["disposition"] == "executed")
    );
    assert!(
        activity
            .iter()
            .any(|row| row["subject"] == "web:test" && row["disposition"] == "reused")
    );
    fs::write(ctx.root().join("api/example.go"), "package drifted_again\n").unwrap();
    assert!(
        crate::runtime::call_tool(&ctx, tool::WORK_FINISH, json!({"plan_id": "plan_1"})).is_err()
    );
    crate::state::ensure_plan_is_open(&ctx, "plan_1").unwrap();
}

#[test]
fn compact_failed_checks_return_failure_without_hiding_gate_recovery() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, true);
    let ctx = set_config(&ctx, |config| {
        config["commands"]["api_test_command"] =
            toml::Value::String("printf failed >&2; exit 7".into());
    });
    let result = check(&ctx);
    assert_eq!(result["ok"], false, "{result:#}");
    assert_eq!(result["finish_ready"], false);
    assert!(
        result["activity"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["subject"] == "api:test" && row["status"] == "failed")
    );
    assert_eq!(result["next_step"]["argv"][2], "check");
    assert_schema(&ctx, tool::WORK_CHECK, &result);
    assert!(
        crate::runtime::call_tool(&ctx, tool::WORK_CHECK, json!({"plan_id": "plan_1"})).is_err()
    );
}

#[test]
fn compact_review_requirements_remain_visible_and_unexecuted() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, true);
    let ctx = set_config(&ctx, |config| {
        config["work"]["gates"].as_array_mut().unwrap().push(
            toml::from_str("id = 'remaining'\nkind = 'codex_review'\nskill = 'example-review'")
                .unwrap(),
        );
    });
    let result = check(&ctx);
    assert_eq!(result["ok"], true, "{result:#}");
    assert_eq!(result["finish_ready"], false);
    let remaining = result["gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "remaining")
        .unwrap();
    assert_eq!(remaining["kind"], "codex_review");
    assert_eq!(remaining["status"], "missing");
    assert_eq!(
        result["next_step"]["argv"],
        json!([
            "scripts/jig",
            "work",
            "review",
            "--plan-id",
            "plan_1",
            "--gate",
            "remaining"
        ])
    );
    let journal = fs::read_to_string(ctx.state_file("receipts.jsonl")).unwrap();
    assert!(!journal.lines().any(
        |line| serde_json::from_str::<Value>(line).unwrap()["tool_name"] == tool::WORK_REVIEW
    ));
}

#[test]
fn compact_external_evidence_stays_unsupported_without_automatic_effect_approval() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, true);
    let path = ctx.root().join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["actions"][0]["effects"] = json!(["process", "external"]);
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let ctx = set_config(&ctx, |config| {
        config["repository"]["actions"][0]["effects"] =
            toml::Value::try_from(vec!["process", "external"]).unwrap();
        config["commands"]["api_test_command"] =
            toml::Value::String("touch external-launched".into());
    });
    let result = inspect(&ctx);
    assert_eq!(result["finish_ready"], false);
    assert_eq!(result["gates"][0]["status"], "unsupported", "{result:#}");
    assert!(
        result["gates"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("not a read-only check"),
        "{result:#}"
    );
    assert_eq!(result["next_step"]["read_only"], true);
    let error = crate::runtime::call_tool_on_surface(
        &ctx,
        tool::WORK_CHECK,
        json!({"plan_id": "plan_1"}),
        ResponseSurface::AgentV1,
    )
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("not a read-only check"),
        "{error:#}"
    );
    assert!(!ctx.root().join("external-launched").exists());
}

#[test]
fn compact_unknown_missing_authority_recommends_only_read_only_inspection() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, true);
    fs::write(ctx.root().join(".gitignore"), "api/ignored\n").unwrap();
    fs::write(ctx.root().join("api/ignored"), "unobservable input\n").unwrap();
    let before = fs::read(ctx.state_file("receipts.jsonl")).ok();
    let result = inspect(&ctx);
    assert_eq!(result["finish_ready"], false);
    let target = result["gates"][0]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["target"]["component"] == "api")
        .unwrap();
    assert_eq!(target["status"], "missing");
    assert_eq!(target["freshness"], "unknown");
    assert_eq!(result["next_step"]["read_only"], true);
    assert_eq!(result["next_step"]["argv"][2], "gates");
    assert_eq!(before, fs::read(ctx.state_file("receipts.jsonl")).ok());
}

#[test]
fn compact_recovery_preserves_execution_dependency_closure() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), true, true);
    assert_eq!(check(&ctx)["finish_ready"], true);
    fs::write(ctx.root().join("api/example.go"), "package changed\n").unwrap();
    let recovery = inspect(&ctx);
    assert_eq!(
        recovery["next_step"]["argv"],
        json!(["scripts/jig", "work", "check", "--plan-id", "plan_1"])
    );
    let result = check(&ctx);
    assert_eq!(result["finish_ready"], true);
    assert_eq!(result["activity_count"], 2);
    assert!(
        result["activity"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["disposition"] == "executed")
    );
}

#[test]
fn compact_preview_is_bounded_without_hiding_required_failure_from_readiness() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, true);
    let ctx = set_config(&ctx, |config| {
        let gates = config["work"]["gates"].as_array_mut().unwrap();
        gates.clear();
        for index in 0..60 {
            gates.push(toml::from_str(&format!(
                "id = 'review-{index}'\nkind = 'codex_review'\nskill = 'example-review'\nrequired = {}",
                index == 59
            )).unwrap());
        }
    });
    let result = inspect(&ctx);
    assert_eq!(result["finish_ready"], false);
    assert_eq!(result["gate_count"], 60);
    assert_eq!(
        result["gates"].as_array().unwrap().len(),
        crate::surface::work::MAX_ROWS
    );
    assert_eq!(result["gates_truncated"], true);
    assert_eq!(
        result["next_step"]["argv"]
            .as_array()
            .unwrap()
            .last()
            .unwrap(),
        "review-59"
    );
    assert_schema(&ctx, tool::WORK_GATES, &result);
}

#[test]
fn compact_legacy_not_applicable_is_distinct_and_does_not_launch_the_check() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["contract_version"] = json!(5);
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let ctx = set_config(&ctx, |config| {
        config["work"]["gates"][0].as_table_mut().unwrap().insert(
            "paths".into(),
            toml::Value::try_from(vec!["docs/**"]).unwrap(),
        );
        config["commands"]["custom_check_command"] = toml::Value::String("exit 99".into());
    });
    init_git_repo(ctx.root());
    let plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Example scoped summary".into(),
            body: None,
            body_file: None,
            base: Some("HEAD".into()),
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap();
    fs::write(ctx.root().join("example.txt"), "unrelated change\n").unwrap();
    let result = agent(&ctx, tool::WORK_CHECK, json!({"plan_id": plan_id}));
    assert_eq!(result["ok"], true, "{result:#}");
    assert_eq!(result["finish_ready"], true, "{result:#}");
    assert_eq!(result["activity"][0]["disposition"], "not_applicable");
    assert_eq!(result["gates"][0]["status"], "not_applicable");
    let records = fs::read_to_string(ctx.state_file("receipts.jsonl")).unwrap();
    assert!(!records.lines().any(
        |line| serde_json::from_str::<Value>(line).unwrap()["tool_name"] == "jig.custom_check"
    ));
}
