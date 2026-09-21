use super::*;

#[test]
fn final_phase_does_not_publish_reuse_over_concurrent_current_plan_failure() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(
        temp.path(),
        r#"
[[work.gates]]
id = "legacy"
kind = "check"
tool = "jig.web_test"
paths = ["web/**"]
reuse = true
"#,
    );
    enable_v6_legacy_web_tool(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let open_plan = |title: &str| {
        crate::state::plans_open(
            &ctx,
            crate::state::PlanOpenRequest {
                title: title.into(),
                body: Some("Fixture plan".into()),
                body_file: None,
                base: None,
            },
        )
        .unwrap()["plan_id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let source_plan = open_plan("Source plan");
    let current_plan = open_plan("Current plan");
    fs::write(
        temp.path().join("web/example.ts"),
        "export const example = 'changed';\n",
    )
    .unwrap();

    let source = work_check_for_plan(&ctx, &source_plan, "final", false);
    assert_eq!(source["ok"], true, "{source:#}");
    assert_eq!(source["gate_evidence"][0]["status"], "executed");

    let mut failed_gate: crate::state::WorkCheckGateEvidence =
        serde_json::from_value(source["gate_evidence"][0].clone()).unwrap();
    failed_gate.status = "failed".into();
    failed_gate.exit_status = Some(7);
    failed_gate.reason = "concurrent current-plan failure".into();
    let fingerprint = crate::state::current_worktree_fingerprint(&ctx)
        .fingerprint
        .unwrap();
    let mut observer = crate::execution::NoopExecutionObserver;

    let error = crate::runtime::work::check_phase_with_pre_execution_test_hook(
        &ctx,
        crate::command::WorkCheckRequest {
            rust_focus: Default::default(),
            projection: Default::default(),
            plan_id: current_plan.clone(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: Some(crate::command::WorkCheckPhase::Final),
            explain: false,
        },
        &mut observer,
        || {
            crate::state::record_receipt(
                &ctx,
                crate::state::ReceiptInput {
                    tool_name: crate::tool_defs::tool::WORK_CHECK,
                    args: json!({"gates": ["legacy"], "tools": ["jig.web_test"]}),
                    invoked_command_key: None,
                    plan_id: Some(current_plan.clone()),
                    started_at_ms: 10,
                    ended_at_ms: 11,
                    exit_status: 7,
                    stdout: "",
                    stderr: "concurrent failure",
                    evidence: Some(
                        serde_json::to_value(crate::state::WorkCheckBatchEvidence {
                            effective_time: None,
                            schema: crate::state::WORK_CHECK_EVIDENCE_SCHEMA.into(),
                            changed_paths: Vec::new(),
                            changed_path_count: 0,
                            changed_paths_truncated: false,
                            changed_paths_digest: None,
                            valid_until_ms: None,
                            requires_time_validity: false,
                            gates: vec![failed_gate],
                        })
                        .unwrap(),
                    ),
                    session_override: None,
                    collect_git_metadata: false,
                    collect_worktree_fingerprint: false,
                    worktree_fingerprint_override: Some(Ok(fingerprint)),
                },
            )
            .unwrap();
        },
    )
    .unwrap_err();
    let error = format!("{error:#}");

    assert!(
        error.contains("current-plan evidence was recorded for: legacy"),
        "{error}"
    );
    let current_batches = read_receipts(temp.path())
        .into_iter()
        .filter(|receipt| {
            receipt["plan_id"] == current_plan
                && receipt["tool_name"] == crate::tool_defs::tool::WORK_CHECK
        })
        .collect::<Vec<_>>();
    assert_eq!(current_batches.len(), 1, "{current_batches:#?}");
    assert_eq!(
        current_batches[0]["evidence"]["gates"][0]["status"],
        "failed"
    );
}
