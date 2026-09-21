use super::*;

#[test]
fn native_gate_preview_forces_fresh_targets_and_prerequisites_without_execution() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(
        temp.path(),
        r#"
[[work.gates]]
id = "full"
kind = "evidence"
target = "web:test"
"#,
    );
    enable_v6_iteration_profile(temp.path());
    let config_path = temp.path().join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let dependency = json!([{"component":"api", "action":"test"}]);
    config["repository"]["actions"][1]
        .as_table_mut()
        .unwrap()
        .insert(
            "depends_on".into(),
            toml::Value::try_from(&dependency).unwrap(),
        );
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["actions"][1]["depends_on"] = dependency;
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let first =
        crate::runtime::call_tool(&ctx, tool::WORK_CHECK, json!({"plan_id":"plan_1"})).unwrap();
    assert_eq!(first["ok"], true, "{first:#}");
    let launch_path = temp.path().join(".agent/launch.log");
    assert_eq!(fs::read_to_string(&launch_path).unwrap(), "api\nweb\n");
    let snapshot = || {
        [
            ".agent/state/receipts.jsonl",
            ".agent/state/runs.jsonl",
            ".agent/state/plans.jsonl",
            ".agent/state/sessions.jsonl",
            ".agent/launch.log",
            ".jig.toml",
            ".agent/jig-contract.json",
            "api/example.go",
            "web/example.ts",
        ]
        .map(|path| (path, fs::read(temp.path().join(path)).unwrap_or_default()))
    };
    let before = snapshot();
    let source_before =
        crate::state::current_worktree_fingerprint_with_cancellation(&ctx, &|| false)
            .unwrap()
            .fingerprint
            .unwrap();

    let ordinary = crate::runtime::call_tool(
        &ctx,
        tool::WORK_CHECK,
        json!({"plan_id":"plan_1", "explain":true}),
    )
    .unwrap();
    let ordinary_invocations = ordinary["selected_invocations"].as_array().unwrap();
    assert_eq!(ordinary_invocations.len(), 2, "{ordinary:#}");
    assert!(
        ordinary_invocations
            .iter()
            .all(|entry| entry["disposition"] == "reused"
                && entry["evidence_validity"]["status"] == "passed"),
        "{ordinary:#}"
    );
    assert_eq!(
        snapshot(),
        before,
        "ordinary preview must not write source, journals, or launch records"
    );

    let result = crate::runtime::call_tool(
        &ctx,
        tool::WORK_CHECK,
        json!({
            "plan_id": "plan_1", "gates": ["full"], "explain": true
        }),
    )
    .unwrap();
    assert_eq!(result["ok"], true, "{result:#}");
    assert_eq!(result["phase"], "final");
    let invocations = result["selected_invocations"].as_array().unwrap();
    assert_eq!(invocations.len(), 2);
    assert_eq!(
        invocations
            .iter()
            .map(|entry| entry["target"].clone())
            .collect::<Vec<_>>(),
        vec![
            json!({"component":"api", "action":"test"}),
            json!({"component":"web", "action":"test"}),
        ]
    );
    assert!(
        invocations
            .iter()
            .all(|entry| entry["disposition"] == "selected"
                && entry["evidence_validity"]["status"] == "passed"),
        "forced preview must select fresh targets and their prerequisites: {result:#}"
    );
    assert!(result["selected_checks"].as_array().unwrap().is_empty());
    assert_eq!(
        snapshot(),
        before,
        "forced preview must not write source, journals, or launch records"
    );
    assert_eq!(
        crate::state::current_worktree_fingerprint_with_cancellation(&ctx, &|| false)
            .unwrap()
            .fingerprint
            .as_deref(),
        Some(source_before.as_str())
    );

    let executed = crate::runtime::call_tool(
        &ctx,
        tool::WORK_CHECK,
        json!({"plan_id":"plan_1", "gates":["full"]}),
    )
    .unwrap();
    assert_eq!(executed["ok"], true, "{executed:#}");
    assert_eq!(
        fs::read_to_string(launch_path).unwrap(),
        "api\nweb\napi\nweb\n"
    );
}

#[test]
fn compact_phase_projection_is_rejected_before_execution_or_receipts() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    enable_v6_iteration_profile(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let receipts = temp.path().join(".agent/state/receipts.jsonl");
    let before = fs::read(&receipts).unwrap_or_default();
    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: crate::surface::ResponseSurface::AgentV1,
            plan_id: "plan_1".into(),
            gates: vec![],
            tools: vec![],
            phase: Some("iteration".into()),
            explain: false,
        })),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("not supported by projection agent-v1"),
        "{error:#}"
    );
    assert!(!temp.path().join(".agent/launch.log").exists());
    assert_eq!(fs::read(receipts).unwrap_or_default(), before);
}
