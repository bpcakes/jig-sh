use super::*;

#[test]
fn work_gate_evaluations_scan_receipts_once_for_multiple_gates() {
    let temp = tempdir().unwrap();
    write_mutating_check_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    crate::state::reset_work_gate_receipt_index_scan_count();
    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            projection: crate::surface::ResponseSurface::Standard,
            freshness_timeout_ms: None,
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();

    assert_eq!(gates["gates"].as_array().unwrap().len(), 2);
    assert_eq!(crate::state::work_gate_receipt_index_scan_count(), 1);

    crate::state::reset_work_gate_receipt_index_scan_count();
    let evidence = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Evidence(
            crate::cli::WorkEvidenceOpts {
                projection: crate::surface::ResponseSurface::Standard,
                freshness_timeout_ms: None,
                plan_id: Some("plan_1".into()),
            },
        )),
    )
    .unwrap();

    assert_eq!(evidence["gates"].as_array().unwrap().len(), 2);
    assert_eq!(crate::state::work_gate_receipt_index_scan_count(), 1);
}

#[test]
fn status_gate_batch_scans_receipts_once_for_multiple_open_plans() {
    let temp = tempdir().unwrap();
    write_mutating_check_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let second = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Second plan".into(),
            body: Some("Validate shared gate indexing.".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let second_id = second["plan_id"].as_str().unwrap().to_string();
    let plan_ids = vec!["plan_1".to_string(), second_id.clone()];

    crate::state::reset_work_gate_receipt_index_scan_count();
    let snapshots =
        super::super::open_plan_gate_snapshots_with_cancellation(&ctx, &plan_ids, &|| false, None)
            .unwrap();

    assert_eq!(snapshots.len(), 2);
    assert!(snapshots.contains_key("plan_1"));
    assert!(snapshots.contains_key(&second_id));
    assert_eq!(crate::state::work_gate_receipt_index_scan_count(), 1);
}

#[test]
fn work_gates_reports_missing_and_passing_required_gates() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let missing = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            projection: crate::surface::ResponseSurface::Standard,
            freshness_timeout_ms: None,
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();
    assert_eq!(missing["overall"], "blocked");
    assert_eq!(missing["ok"], true);
    assert_eq!(missing["gates_ok"], false);
    assert_eq!(missing["gates"][0]["id"], "custom");
    assert_eq!(missing["gates"][0]["status"], "missing");
    assert_eq!(missing["missing_required"][0], "custom");

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: crate::surface::ResponseSurface::Standard,
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: None,
            explain: false,
        })),
    )
    .unwrap();

    let passed = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            projection: crate::surface::ResponseSurface::Standard,
            freshness_timeout_ms: None,
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();
    assert_eq!(passed["overall"], "passed");
    assert_eq!(passed["ok"], true);
    assert_eq!(passed["gates_ok"], true);
    assert_eq!(passed["plan_state"], "open");
    assert_eq!(passed["gates"][0]["status"], "passed");
    assert!(passed["gates"][0]["receipt_id"].as_str().is_some());
}

#[test]
fn work_gates_report_a_renamed_check_tool_as_unsupported() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "tool = \"jig.custom_check\"",
        "tool = \"jig.renamed_check\"",
    );
    fs::write(config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            projection: crate::surface::ResponseSurface::Standard,
            freshness_timeout_ms: None,
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "blocked");
    assert_eq!(gates["gates"][0]["kind"], "check");
    assert_eq!(gates["gates"][0]["status"], "unsupported");
    assert!(
        gates["gates"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("jig.renamed_check")
    );
}

#[test]
fn empty_open_plan_gate_batch_skips_fingerprint_collection() {
    use std::cell::Cell;

    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let cancellation_checks = Cell::new(0);

    let snapshots = super::super::open_plan_gate_snapshots_with_cancellation(
        &ctx,
        &[],
        &|| {
            let current = cancellation_checks.get();
            cancellation_checks.set(current + 1);
            current > 0
        },
        None,
    )
    .unwrap();

    assert!(snapshots.is_empty());
    assert_eq!(cancellation_checks.get(), 1);
}

#[test]
fn work_evidence_defaults_to_single_open_plan_and_reports_latest_passing_gate() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: crate::surface::ResponseSurface::Standard,
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: None,
            explain: false,
        })),
    )
    .unwrap();

    let evidence = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Evidence(
            crate::cli::WorkEvidenceOpts {
                projection: crate::surface::ResponseSurface::Standard,
                freshness_timeout_ms: None,
                plan_id: None,
            },
        )),
    )
    .unwrap();

    assert_eq!(evidence["command"], "work evidence");
    assert_eq!(evidence["ok"], true);
    assert_eq!(evidence["plan_id"], "plan_1");
    assert_eq!(evidence["plan_state"], "open");
    assert_eq!(
        evidence["latest_passing_gates"][0]["tool"],
        "jig.custom_check"
    );
    assert_eq!(evidence["latest_passing_gates"][0]["gate_id"], "custom");
    assert_eq!(
        evidence["latest_passing_gates"][0]["matches_current_worktree"],
        true
    );
    assert!(
        evidence["latest_passing_gates"][0]["changed_paths"]
            .as_array()
            .is_some()
    );
    assert!(
        evidence["latest_passing_gates"][0]["changed_path_count"]
            .as_u64()
            .is_some()
    );
    assert_eq!(
        evidence["latest_passing_gates"][0]["changed_paths_truncated"],
        false
    );
}

#[test]
fn work_evidence_gate_health_reflects_blocked_gates() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let evidence = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Evidence(
            crate::cli::WorkEvidenceOpts {
                projection: crate::surface::ResponseSurface::Standard,
                freshness_timeout_ms: None,
                plan_id: None,
            },
        )),
    )
    .unwrap();

    assert_eq!(evidence["overall"], "blocked");
    assert_eq!(evidence["ok"], true);
    assert_eq!(evidence["gates_ok"], false);
    assert_eq!(evidence["missing_required"][0], "custom");
}

#[test]
fn work_evidence_reports_closed_plan_state() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: crate::surface::ResponseSurface::Standard,
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: None,
            explain: false,
        })),
    )
    .unwrap();
    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Finish(
            crate::cli::WorkFinishOpts {
                plan_id: "plan_1".into(),
                resolution: Some("done".into()),
                outcome: Some("success".into()),
            },
        )),
    )
    .unwrap();

    let evidence = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Evidence(
            crate::cli::WorkEvidenceOpts {
                projection: crate::surface::ResponseSurface::Standard,
                freshness_timeout_ms: None,
                plan_id: Some("plan_1".into()),
            },
        )),
    )
    .unwrap();

    assert_eq!(evidence["overall"], "passed");
    assert_eq!(evidence["plan_state"], "closed");
}

#[test]
fn work_evidence_requires_plan_id_when_multiple_plans_are_open() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Second plan".into(),
            body: Some("Second plan body".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Evidence(
            crate::cli::WorkEvidenceOpts {
                projection: crate::surface::ResponseSurface::Standard,
                freshness_timeout_ms: None,
                plan_id: None,
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Multiple open work plans"));
    assert!(error.contains("Pass --plan-id to choose"));
}

#[test]
fn work_evidence_without_open_plan_points_to_work_status() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    crate::state::plans_close(
        &ctx,
        crate::state::PlanCloseRequest {
            plan_id: "plan_1".into(),
            resolution: Some("done".into()),
        },
    )
    .unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Evidence(
            crate::cli::WorkEvidenceOpts {
                projection: crate::surface::ResponseSurface::Standard,
                freshness_timeout_ms: None,
                plan_id: None,
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("No open work plans"));
    assert!(error.contains("scripts/jig work status"));
}

#[test]
fn work_gates_defaults_to_single_open_plan() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            projection: crate::surface::ResponseSurface::Standard,
            freshness_timeout_ms: None,
            plan_id: None,
        })),
    )
    .unwrap();

    assert_eq!(gates["plan_id"], "plan_1");
    assert_eq!(gates["overall"], "blocked");
    assert_eq!(gates["missing_required"][0], "custom");
}

#[test]
fn work_gates_rejects_unknown_plan() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            projection: crate::surface::ResponseSurface::Standard,
            freshness_timeout_ms: None,
            plan_id: Some("plan_missing".into()),
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Plan not found: plan_missing"));
}

#[test]
fn work_finish_rejects_missing_required_gates() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_test_plan(&ctx);

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Finish(
            crate::cli::WorkFinishOpts {
                plan_id,
                resolution: Some("done".into()),
                outcome: Some("success".into()),
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Required work gates are not satisfied"));
    assert!(error.contains("Missing: [custom]"));
}

#[test]
fn work_finish_rejects_unknown_plan_before_checking_gates() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Finish(
            crate::cli::WorkFinishOpts {
                plan_id: "plan_missing".into(),
                resolution: Some("done".into()),
                outcome: Some("success".into()),
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Plan not found: plan_missing"));
    assert!(!error.contains("Required work gates are not satisfied"));
}

#[test]
fn work_finish_allows_passing_required_gates() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_test_plan(&ctx);

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: crate::surface::ResponseSurface::Standard,
            plan_id: plan_id.clone(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: None,
            explain: false,
        })),
    )
    .unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Finish(
            crate::cli::WorkFinishOpts {
                plan_id: plan_id.clone(),
                resolution: Some("done".into()),
                outcome: Some("success".into()),
            },
        )),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["plan"]["plan_id"], plan_id);
}

mod closure;

#[test]
fn work_gates_reject_stale_required_gate_receipts() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_test_plan(&ctx);

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: crate::surface::ResponseSurface::Standard,
            plan_id: plan_id.clone(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: None,
            explain: false,
        })),
    )
    .unwrap();
    fs::write(temp.path().join("changed.txt"), "changed\n").unwrap();

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            projection: crate::surface::ResponseSurface::Standard,
            freshness_timeout_ms: None,
            plan_id: Some(plan_id.clone()),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "blocked");
    assert_eq!(gates["gates"][0]["status"], "stale");
    assert_eq!(gates["gates"][0]["freshness"], "stale");
    assert_eq!(gates["stale_required"][0], "custom");

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Finish(
            crate::cli::WorkFinishOpts {
                plan_id,
                resolution: Some("done".into()),
                outcome: Some("success".into()),
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Stale: [custom]"));
}

#[test]
fn work_gates_reject_unknown_required_gate_freshness() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_test_plan(&ctx);

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: crate::surface::ResponseSurface::Standard,
            plan_id: plan_id.clone(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: None,
            explain: false,
        })),
    )
    .unwrap();

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            projection: crate::surface::ResponseSurface::Standard,
            freshness_timeout_ms: None,
            plan_id: Some(plan_id.clone()),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "blocked");
    assert_eq!(gates["gates"][0]["status"], "unknown");
    assert_eq!(gates["gates"][0]["freshness"], "unknown");
    assert_eq!(gates["unknown_required"][0], "custom");

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Finish(
            crate::cli::WorkFinishOpts {
                plan_id,
                resolution: Some("done".into()),
                outcome: Some("success".into()),
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Unknown: [custom]"));
}

#[test]
fn work_config_rejects_unsupported_gate_kind() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .config(
            r#"
[[work.gates]]
id = "custom"
kind = "unsupported-kind"
"#,
        )
        .write();
    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();

    assert!(error.contains("Unsupported work gate kind 'unsupported-kind'"));
}
