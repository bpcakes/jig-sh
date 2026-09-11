use super::*;

fn check(ctx: &RepoContext) -> Value {
    crate::runtime::call_tool(
        ctx,
        crate::tool_defs::tool::WORK_CHECK,
        json!({"plan_id": "plan_1"}),
    )
    .unwrap()
}

#[test]
fn missing_receipts_preserve_unavailable_authority_in_read_only_recovery() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, false);
    let config_path = ctx.root().join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["commands"]["web_test_command"] = toml::Value::String("touch .scratch/executed".into());
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    fs::create_dir(ctx.root().join(".scratch")).unwrap();
    fs::write(ctx.root().join(".gitignore"), ".scratch/\nweb/ignored\n").unwrap();
    let ctx = RepoContext::load_from_root(ctx.root().to_path_buf()).unwrap();
    let journal = ctx.state_file("receipts.jsonl");
    let before = fs::read(&journal).ok();
    for unavailable in [false, true] {
        if unavailable {
            fs::write(ctx.root().join("web/ignored"), "unobservable input\n").unwrap();
        }
        let response = crate::runtime::call_tool(
            &ctx,
            crate::tool_defs::tool::WORK_EVIDENCE,
            json!({"plan_id": "plan_1", "freshness_timeout_ms": 30_000}),
        )
        .unwrap();
        let target = &response["gates"][0]["targets"][0];
        assert_eq!(target["status"], "missing", "{response:#}");
        let recovery = &response["recovery"];
        if unavailable {
            assert_eq!(target["freshness"], "unknown", "{response:#}");
            assert!(
                target["freshness_reasons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|reason| reason["code"] == "unobservable_input"
                        && reason["path"] == "web/ignored")
            );
            assert_eq!(recovery["inspection"], "unavailable", "{response:#}");
            assert_eq!(recovery["execute"], json!([]));
            assert!(recovery["next_step"].is_null());
        } else {
            assert_eq!(target["freshness"], "missing", "{response:#}");
            assert_eq!(recovery["inspection"], "complete");
            assert_eq!(
                recovery["execute"],
                json!(["web:test".parse::<TargetId>().unwrap()])
            );
            assert_eq!(recovery["next_step"]["argv"][2], "check");
        }
        assert_eq!(recovery["preview_available"], !unavailable);
        let dashboard = crate::ui::RepoDashboardSource::new(
            RepoContext::load_from_root(ctx.root().to_path_buf()).unwrap(),
        )
        .with_freshness_timeout(Some(30_000));
        let snapshot = jig_ui::dashboard::DashboardSource::recorder(
            &dashboard,
            jig_ui::dashboard::RecorderRequest {
                mode: jig_ui::dashboard::RecorderMode::Refresh,
                timeline_limit: jig_ui::dashboard::TimelineLimit::new(25).unwrap(),
            },
            &|| false,
        )
        .unwrap();
        let report = snapshot.status_local.work.gates[0]
            .snapshot
            .as_ref()
            .unwrap();
        assert_eq!(serde_json::to_value(&report.recovery).unwrap(), *recovery);
        assert_eq!(
            fs::read(&journal).ok(),
            before,
            "inspection appended receipts"
        );
        assert!(
            !ctx.root().join(".scratch/executed").exists(),
            "inspection ran a check"
        );
    }
}

#[test]
fn read_only_recovery_predicts_independent_execution_and_reuse() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, true);
    let config_path = ctx.root().join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["commands"]["api_test_command"] =
        toml::Value::String("printf 'api\n' >> .scratch/invocations".into());
    config["commands"]["web_test_command"] =
        toml::Value::String("printf 'web\n' >> .scratch/invocations".into());
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    fs::write(ctx.root().join(".gitignore"), ".scratch/\n").unwrap();
    fs::create_dir(ctx.root().join(".scratch")).unwrap();
    let ctx = RepoContext::load_from_root(ctx.root().to_path_buf()).unwrap();
    let first = check(&ctx);
    assert_eq!(first["ok"], true, "{first:#}");
    fs::write(ctx.root().join("api/example.go"), "package changed\n").unwrap();
    let journal = ctx.root().join(".agent/state/receipts.jsonl");
    let before = fs::read(&journal).unwrap();
    let invocations = ctx.root().join(".scratch/invocations");
    let executed_before = fs::read(&invocations).unwrap();
    let response = crate::runtime::call_tool(
        &ctx,
        crate::tool_defs::tool::WORK_EVIDENCE,
        json!({"plan_id": "plan_1", "freshness_timeout_ms": 30_000}),
    )
    .unwrap();
    let dashboard = crate::ui::RepoDashboardSource::new(
        RepoContext::load_from_root(ctx.root().to_path_buf()).unwrap(),
    )
    .with_freshness_timeout(Some(30_000));
    let snapshot = jig_ui::dashboard::DashboardSource::recorder(
        &dashboard,
        jig_ui::dashboard::RecorderRequest {
            mode: jig_ui::dashboard::RecorderMode::Refresh,
            timeline_limit: jig_ui::dashboard::TimelineLimit::new(25).unwrap(),
        },
        &|| false,
    )
    .unwrap();
    let dashboard_report = snapshot.status_local.work.gates[0]
        .snapshot
        .as_ref()
        .unwrap();
    assert_eq!(
        serde_json::to_value(&dashboard_report.recovery).unwrap(),
        response["recovery"],
        "dashboard must retain native recovery commands and execution/reuse advice"
    );
    assert_eq!(
        fs::read(&journal).unwrap(),
        before,
        "inspection appended receipts"
    );
    assert_eq!(
        fs::read(&invocations).unwrap(),
        executed_before,
        "inspection ran a check"
    );
    let recovery = &response["recovery"];
    assert_eq!(recovery["preview_available"], true, "{response:#}");
    assert_eq!(
        recovery["execute"],
        json!(["api:test".parse::<TargetId>().unwrap()])
    );
    assert_eq!(
        recovery["reuse"],
        json!(["web:test".parse::<TargetId>().unwrap()])
    );
    assert_eq!(
        recovery["next_step"]["argv"],
        json!(["scripts/jig", "work", "check", "--plan-id", "plan_1"])
    );
    let api = response["gates"][0]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["target"]["component"] == "api")
        .unwrap();
    assert!(
        api["freshness_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason["code"] == "direct_input_changed"
                && reason["path"] == "api/example.go"),
        "{api:#}"
    );
    let checked = check(&ctx);
    assert_eq!(checked["ok"], true, "{checked:#}");
    assert_eq!(
        &fs::read(&invocations).unwrap()[executed_before.len()..],
        b"api\n"
    );
    let actual: Vec<_> = checked["run"]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|target| target["started_at_ms"].is_u64())
        .map(|target| target["target"].clone())
        .collect();
    assert_eq!(json!(actual), recovery["execute"]);
    let web = checked["target_evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["target"]["component"] == "web")
        .unwrap();
    assert_eq!(web["disposition"], "reused", "{checked:#}");
}

#[test]
fn native_target_recovery_preserves_plan_comparison_without_overrides() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, true);
    super::native::configure_native(
        &ctx,
        "version=1\n[[rules]]\nid=\"source\"\ninclude=[\"api/**\"]\nmax_lines=20\n",
    );
    run_git(ctx.root(), &["add", "."]);
    run_git(
        ctx.root(),
        &["commit", "-qm", "Example native recovery authority"],
    );
    let ctx = RepoContext::load_from_root(ctx.root().to_path_buf()).unwrap();
    let plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Example recovery".into(),
            body: None,
            body_file: None,
            base: Some("HEAD".into()),
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap();
    let result = run_repository_target_for_plan(&ctx, "test", plan_id);
    assert_eq!(result["ok"], true, "{result:#}");
    fs::write(ctx.root().join("api/example.go"), "package changed\n").unwrap();
    let response = crate::runtime::call_tool(
        &ctx,
        crate::tool_defs::tool::WORK_GATES,
        json!({"plan_id": plan_id, "freshness_timeout_ms": 30_000}),
    )
    .unwrap();
    let target = response["recovery"]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["target"]["component"] == "api")
        .unwrap();
    let argv: Vec<_> = target["refresh"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg.as_str().unwrap())
        .collect();
    assert_eq!(
        argv,
        ["scripts/jig", "check", "api:test", "--plan-id", plan_id]
    );
    let refreshed = run_repository_target_for_plan(&ctx, argv[2], argv[4]);
    assert_eq!(refreshed["ok"], true, "{refreshed:#}");
    let after = crate::runtime::call_tool(
        &ctx,
        crate::tool_defs::tool::WORK_GATES,
        json!({"plan_id": plan_id, "freshness_timeout_ms": 30_000}),
    )
    .unwrap();
    let native = after["gates"][0]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["target"]["component"] == "api")
        .unwrap();
    assert_eq!(native["status"], "passed", "{after:#}");
}

#[test]
fn selected_legacy_tool_explains_native_gate_mismatch_without_creating_target_evidence() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, true);
    let manifest_path = ctx.root().join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["tools"] = json!([{
        "name":"jig.example_check", "kind":"command", "description":"Example check",
        "command":"api_test_command"
    }]);
    manifest["actions"][0]["legacy_aliases"] = json!(["jig.example_check"]);
    let config_path = ctx.root().join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["repository"]["actions"][0]
        .as_table_mut()
        .unwrap()
        .insert(
            "legacy_aliases".into(),
            toml::Value::try_from(vec!["jig.example_check"]).unwrap(),
        );
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let ctx = RepoContext::load_from_root(ctx.root().to_path_buf()).unwrap();
    let checked = crate::runtime::call_tool(
        &ctx,
        crate::tool_defs::tool::WORK_CHECK,
        json!({"plan_id":"plan_1", "tools":["jig.example_check"]}),
    )
    .unwrap();
    assert_eq!(checked["ok"], true, "{checked:#}");
    assert!(
        checked["native_evidence_note"]
            .as_str()
            .unwrap()
            .contains("cannot satisfy configured native target gates")
    );
    assert!(checked.get("target_evidence").is_none());
    let report = work_gates(&ctx);
    assert_eq!(report["gates"][0]["status"], "missing", "{report:#}");
}
