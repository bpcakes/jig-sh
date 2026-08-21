use super::*;
use crate::tool_defs::WORKER_RUN_TOOL;
use std::path::Path;

#[test]
fn cli_dispatch_requires_manifest_tool_declaration() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Fmt(crate::cli::ToolOpts {
                plan_id: None,
                no_receipt: false,
            }),
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Tool is not declared in .agent/jig-contract.json"));
}

#[test]
fn unavailable_schema_check_explains_disabled_config() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Schema(crate::cli::ToolOpts {
                plan_id: None,
                no_receipt: false,
            }),
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("jig.schema_check is not available"));
    assert!(error.contains("sqlx_enabled = false"));
    assert!(error.contains("jig update --recopy"));
}

#[test]
fn unavailable_go_checks_explain_backend_and_database_capabilities() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let lint_error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Lint(crate::cli::ToolOpts {
                plan_id: None,
                no_receipt: false,
            }),
        )),
    )
    .unwrap_err()
    .to_string();
    assert!(lint_error.contains("backend_language is not \"go\""));
    assert!(lint_error.contains("check clippy"));

    let sqlc_error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Sqlc(crate::cli::ToolOpts {
                plan_id: None,
                no_receipt: false,
            }),
        )),
    )
    .unwrap_err()
    .to_string();
    assert!(sqlc_error.contains("sqlc checks belong to a Go/PostgreSQL backend"));

    let go = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(go.path())
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
        .config("backend_language = \"go\"\ngo_database = \"none\"")
        .write();
    let ctx = RepoContext::load_from(go.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Sqlc(crate::cli::ToolOpts {
                plan_id: None,
                no_receipt: false,
            }),
        )),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("go_database is not \"postgres\""));
}

#[test]
fn unavailable_typescript_check_explains_missing_contract_tool() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .config(
            r#"
[[frontend_apps]]
name = "web"
dir = "apps/web"
coverage_threshold = 80
"#,
        )
        .required_commands(Vec::<String>::new())
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::TypeScriptLint(crate::cli::ToolOpts {
                plan_id: None,
                no_receipt: false,
            }),
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("jig.typescript_lint is not declared"));
    assert!(error.contains("jig update --recopy"));
    assert!(error.contains("project-owned [commands]"));
}

#[test]
fn work_goal_opens_durable_plan_and_prompt() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced and the configured gate passes".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: vec!["Do not change public routes".into()],
            checkpoints: vec!["Capture baseline gate status".into()],
            title: Some("API goal".into()),
            notes: Some("Prefer small commits.".into()),
        })),
    )
    .unwrap();

    let plan_id = output["plan"]["plan_id"].as_str().unwrap();
    let body_path = output["plan"]["body_path"].as_str().unwrap();
    let body = fs::read_to_string(temp.path().join(body_path)).unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert!(
        output["goal_prompt"]
            .as_str()
            .unwrap()
            .starts_with("/goal ")
    );
    assert!(output["goal_prompt"].as_str().unwrap().contains(plan_id));
    assert!(body.contains("# Goal Harness"));
    assert!(body.contains("Reduce API handler duplication"));
    assert!(body.contains("- scripts/jig work check"));
    assert!(body.contains("- [ ] Capture baseline gate status"));
    assert!(body.contains("custom: check (jig.custom_check)"));
    assert_eq!(
        output["commands"]["gates"],
        format!("scripts/jig work gates --plan-id {plan_id}")
    );
}

#[test]
fn work_start_validates_plan_body_before_starting_session() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let sessions_path = ctx.state_file("sessions.jsonl");
    let receipts_path = ctx.state_file("receipts.jsonl");
    let sessions_before = fs::read_to_string(&sessions_path).unwrap_or_default();
    let receipts_before = fs::read_to_string(&receipts_path).unwrap_or_default();

    for opts in [
        crate::cli::WorkStartOpts {
            title: "Conflicting body".into(),
            body: Some("inline".into()),
            body_file: Some(temp.path().join("plan.md")),
            print_plan_id: false,
        },
        crate::cli::WorkStartOpts {
            title: "Missing body file".into(),
            body: None,
            body_file: Some(temp.path().join("missing-plan.md")),
            print_plan_id: false,
        },
    ] {
        dispatch(
            &ctx,
            CommandKind::Work(crate::cli::WorkCommand::Start(opts)),
        )
        .unwrap_err();
    }

    assert_eq!(
        fs::read_to_string(&sessions_path).unwrap_or_default(),
        sessions_before
    );
    assert_eq!(
        fs::read_to_string(&receipts_path).unwrap_or_default(),
        receipts_before
    );
    assert_eq!(crate::state::current_session(&ctx).unwrap(), None);
}

#[test]
fn work_goal_rejects_blank_required_fields() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let blank_validation = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced".into(),
            validations: vec!["   ".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(blank_validation.contains("--validation values cannot be empty"));

    let blank_objective = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: " \n\t ".into(),
            success: "duplication is reduced".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(blank_objective.contains("--objective cannot be empty"));

    let blank_success = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: " \n\t ".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(blank_success.contains("--success cannot be empty"));
}

#[test]
fn work_goal_normalizes_prompt_and_defaults_missing_checkpoints() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced\nand the configured gate passes".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap();

    let body_path = output["plan"]["body_path"].as_str().unwrap();
    let body = fs::read_to_string(temp.path().join(body_path)).unwrap();
    let prompt = output["goal_prompt"].as_str().unwrap();

    assert!(prompt.contains("duplication is reduced and the configured gate passes"));
    assert!(!prompt.contains("reduced\nand"));
    assert!(body.contains("duplication is reduced\nand the configured gate passes"));
    assert!(body.contains("- [ ] Read the relevant AGENTS.md files and repo guidance."));
}

#[test]
fn work_goal_rejects_blank_checkpoints_when_provided() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: vec!["   ".into()],
            title: None,
            notes: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("--checkpoint values cannot be empty"));
}

#[test]
fn work_goal_rejects_blank_constraints_when_provided() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: vec!["   ".into()],
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("--constraint values cannot be empty"));
}

#[test]
fn work_goal_truncates_generated_title_to_eighty_chars() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let objective =
        "Reduce API handler duplication while preserving every public route and fixture behavior";

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: objective.into(),
            success: "duplication is reduced".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap();

    let plan_id = output["plan"]["plan_id"].as_str().unwrap();
    let plans = fs::read_to_string(temp.path().join(".agent/state/plans.jsonl")).unwrap();
    let plan_line = plans
        .lines()
        .find(|line| line.contains(plan_id))
        .expect("goal plan event should be recorded");
    let title = serde_json::from_str::<Value>(plan_line).unwrap()["title"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(title.chars().count(), 80);
    assert!(title.ends_with("..."));
}

#[test]
fn work_goal_defaults_blank_title_to_generated_title() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: Some("   ".into()),
            notes: None,
        })),
    )
    .unwrap();

    let plan_id = output["plan"]["plan_id"].as_str().unwrap();
    let plans = fs::read_to_string(temp.path().join(".agent/state/plans.jsonl")).unwrap();
    let plan_line = plans
        .lines()
        .find(|line| line.contains(plan_id))
        .expect("goal plan event should be recorded");
    let title = serde_json::from_str::<Value>(plan_line).unwrap()["title"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(title, "Reduce API handler duplication");
}

#[test]
fn work_check_runs_configured_tools() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["checks"].as_array().unwrap().len(), 1);
    assert_eq!(output["checks"][0]["tool"], "jig.custom_check");
    assert!(output["checks"][0]["receipt_id"].as_str().is_some());
}

#[test]
fn work_check_rejects_unknown_plan_before_running_tools() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_missing".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Plan not found: plan_missing"));
    let receipts_path = temp.path().join(".agent/state/receipts.jsonl");
    let receipts = fs::read_to_string(receipts_path).unwrap_or_default();
    assert!(!receipts.contains("jig.custom_check"));
}

#[test]
fn work_check_rejects_closed_plan_before_running_tools() {
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
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Plan is already closed: plan_1"));
    let receipts_path = temp.path().join(".agent/state/receipts.jsonl");
    let receipts = fs::read_to_string(receipts_path).unwrap_or_default();
    assert!(!receipts.contains("jig.custom_check"));
}

#[test]
fn work_check_collects_change_metadata_only_on_batch_receipt() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("outside-agent.txt"), "changed\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    let receipts_text = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl"))
        .expect("work check should write receipts");
    let receipts = receipts_text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let tool_receipt = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.custom_check")
        .expect("tool receipt should be recorded");
    let batch_receipt = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.work_check")
        .expect("work check batch receipt should be recorded");

    assert!(tool_receipt["worktree_fingerprint"].is_null());
    assert_eq!(tool_receipt["changed_paths"], json!([]));
    assert!(tool_receipt["changed_path_count"].is_null());
    assert_eq!(tool_receipt["diff_stat"]["files"], 0);
    assert!(batch_receipt["worktree_fingerprint"].as_str().is_some());
    assert_eq!(batch_receipt["changed_paths"], json!(["outside-agent.txt"]));
    assert_eq!(batch_receipt["changed_path_count"], 1);
    assert_eq!(batch_receipt["changed_paths_truncated"], false);
    assert!(batch_receipt["changed_paths_digest"].as_str().is_some());
    assert_eq!(
        batch_receipt["args"]["receipt_ids"][0],
        tool_receipt["id"].as_str().unwrap()
    );
}

#[test]
fn failed_work_check_records_metadata_on_batch_and_stops_later_tools() {
    let temp = tempdir().unwrap();
    write_fail_fast_check_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("outside-agent.txt"), "changed\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: vec!["jig.failing_check".into(), "jig.later_check".into()],
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("jig.failing_check failed with status 7"));
    assert!(error.contains("command key: failing_check_command"));
    assert!(!temp.path().join("later-check-ran.txt").exists());

    let receipts_text = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl"))
        .expect("failed work check should write child and batch receipts");
    let receipts = receipts_text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let tool_receipt = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.failing_check")
        .expect("failed tool receipt should be recorded");
    let batch_receipt = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.work_check")
        .expect("failed work check batch receipt should be recorded");

    assert_eq!(tool_receipt["exit_status"], 7);
    assert!(tool_receipt["worktree_fingerprint"].is_null());
    assert_eq!(tool_receipt["changed_paths"], json!([]));
    assert!(tool_receipt["changed_path_count"].is_null());
    assert!(tool_receipt["changed_paths_digest"].is_null());
    assert_eq!(tool_receipt["diff_stat"]["files"], 0);

    assert_eq!(batch_receipt["exit_status"], 7);
    assert_eq!(
        batch_receipt["args"]["tools"],
        json!(["jig.failing_check", "jig.later_check"])
    );
    assert_eq!(
        batch_receipt["args"]["receipt_ids"],
        json!([tool_receipt["id"].as_str().unwrap()])
    );
    assert_eq!(batch_receipt["changed_paths"], json!(["outside-agent.txt"]));
    assert_eq!(batch_receipt["changed_path_count"], 1);
    assert_eq!(batch_receipt["changed_paths_truncated"], false);
    assert!(batch_receipt["changed_paths_digest"].as_str().is_some());
    assert!(batch_receipt["worktree_fingerprint"].as_str().is_some());
    assert!(
        receipts
            .iter()
            .all(|receipt| receipt["tool_name"] != "jig.later_check")
    );
}

#[test]
fn work_check_marks_batch_fingerprint_unknown_when_checks_mutate_worktree() {
    let temp = tempdir().unwrap();
    write_mutating_check_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "blocked");
    assert_eq!(gates["unknown_required"].as_array().unwrap().len(), 2);
    assert_eq!(gates["gates"][0]["status"], "unknown");
    assert!(
        gates["gates"][0]["receipt_worktree_fingerprint_error"]
            .as_str()
            .unwrap()
            .contains("worktree changed during work check")
    );
    assert!(
        gates["gates"][0]["receipt_worktree_fingerprint_error"]
            .as_str()
            .unwrap()
            .contains("before fingerprint")
    );
    assert!(
        gates["gates"][0]["receipt_worktree_fingerprint_error"]
            .as_str()
            .unwrap()
            .contains("after fingerprint")
    );
}

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
                plan_id: Some("plan_1".into()),
            },
        )),
    )
    .unwrap();

    assert_eq!(evidence["gates"].as_array().unwrap().len(), 2);
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
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    let passed = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
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
fn work_evidence_defaults_to_single_open_plan_and_reports_latest_passing_gate() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    let evidence = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Evidence(
            crate::cli::WorkEvidenceOpts { plan_id: None },
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
            crate::cli::WorkEvidenceOpts { plan_id: None },
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
            plan_id: "plan_1".into(),
            tools: Vec::new(),
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
        },
    )
    .unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Evidence(
            crate::cli::WorkEvidenceOpts { plan_id: None },
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
            crate::cli::WorkEvidenceOpts { plan_id: None },
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
            plan_id: plan_id.clone(),
            tools: Vec::new(),
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
            plan_id: plan_id.clone(),
            tools: Vec::new(),
        })),
    )
    .unwrap();
    fs::write(temp.path().join("changed.txt"), "changed\n").unwrap();

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
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
            plan_id: plan_id.clone(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
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

#[test]
fn work_review_records_structured_codex_review_findings() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_review_codex_stub(&codex_path);
    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Review(
            crate::cli::WorkReviewOpts {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
            },
        )),
    )
    .unwrap();

    assert_eq!(output["status"], "failed", "{output:#}");
    assert_eq!(output["reviews"][0]["gate_id"], "rust-error-handling");
    assert_eq!(output["reviews"][0]["actionable_count"], 1);
    assert_eq!(
        output["reviews"][0]["actionable_findings"][0]["severity"],
        "critical"
    );

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();
    assert_eq!(gates["gates"][0]["kind"], "codex_review");
    assert_eq!(gates["gates"][0]["status"], "failed");
    assert_eq!(gates["failed_required"][0], "rust-error-handling");

    let receipts = read_receipts(temp.path());
    let worker_receipt = receipts
        .iter()
        .find(|receipt| {
            receipt["tool_name"] == WORKER_RUN_TOOL
                && receipt["evidence"]["purpose"] == "work_review"
        })
        .expect("work review should record a worker receipt");
    assert_eq!(
        output["reviews"][0]["worker_receipt_id"],
        worker_receipt["id"]
    );
    assert_eq!(worker_receipt["evidence"]["provider"], "codex");
    assert_eq!(worker_receipt["evidence"]["runner"], "codex_exec");
    assert_eq!(worker_receipt["evidence"]["mode"], "review");
}

#[test]
fn work_review_surfaces_raw_counts_when_findings_are_truncated() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_many_findings_review_codex_stub(&codex_path);
    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Review(
            crate::cli::WorkReviewOpts {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
            },
        )),
    )
    .unwrap();

    let review = &output["reviews"][0];
    assert_eq!(review["status"], "failed", "{output:#}");
    assert_eq!(review["finding_count"], 105);
    assert_eq!(review["actionable_count"], 105);
    assert_eq!(review["retained_finding_count"], 100);
    assert_eq!(review["retained_actionable_count"], 100);
    assert_eq!(review["findings_truncated"], true);
    assert_eq!(review["actionable_findings_truncated"], true);
    assert_eq!(review["findings"].as_array().unwrap().len(), 100);
    assert_eq!(review["actionable_findings"].as_array().unwrap().len(), 100);

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();
    let gate = &gates["gates"][0];
    assert_eq!(gate["finding_count"], 105);
    assert_eq!(gate["actionable_count"], 105);
    assert_eq!(gate["retained_finding_count"], 100);
    assert_eq!(gate["retained_actionable_count"], 100);
    assert_eq!(gate["findings_truncated"], true);
    assert_eq!(gate["actionable_findings_truncated"], true);
}

#[test]
fn work_review_fails_when_codex_exits_nonzero_with_below_threshold_findings() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_low_finding_failed_review_codex_stub(&codex_path);
    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Review(
            crate::cli::WorkReviewOpts {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
            },
        )),
    )
    .unwrap();

    let review = &output["reviews"][0];
    assert_eq!(review["status"], "failed", "{output:#}");
    assert_eq!(review["actionable_count"], 0);

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();
    assert_eq!(gates["gates"][0]["status"], "failed", "{gates:#}");
    assert_eq!(gates["failed_required"][0], "rust-error-handling");
}

#[test]
fn work_review_records_invalid_output_when_codex_writes_no_structured_output() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_missing_review_output_codex_stub(&codex_path);
    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Review(
            crate::cli::WorkReviewOpts {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
            },
        )),
    )
    .unwrap();

    assert_eq!(
        output["reviews"][0]["status"], "invalid_output",
        "{output:#}"
    );
    assert!(
        output["reviews"][0]["parse_error"]
            .as_str()
            .unwrap()
            .contains("valid structured JSON")
    );
}

#[test]
fn work_refine_runs_fixer_then_review_and_check_gates() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_review_codex_stub(&codex_path);
    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Refine(
            crate::cli::WorkRefineOpts {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
                max_iterations: 1,
            },
        )),
    )
    .unwrap();

    assert_eq!(output["status"], "passed", "{output:#}");
    assert_eq!(output["iterations"].as_array().unwrap().len(), 1);
    assert!(temp.path().join("fixed.txt").exists());
    assert_eq!(
        fs::read_to_string(temp.path().join("prompt-source.txt")).unwrap(),
        "stdin"
    );
    assert_eq!(output["review"]["status"], "passed");
    assert_eq!(output["checks"]["checks"][0]["result"]["exit_status"], 0);

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();
    assert_eq!(gates["overall"], "passed", "{gates:#}");
}

#[test]
fn work_refine_fails_when_review_gate_returns_invalid_output() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_invalid_review_codex_stub(&codex_path);
    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Refine(
            crate::cli::WorkRefineOpts {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
                max_iterations: 1,
            },
        )),
    )
    .unwrap();

    assert_eq!(output["status"], "failed", "{output:#}");
    assert_eq!(output["iterations"].as_array().unwrap().len(), 0);
    assert_eq!(output["failed_review_gates"][0], "rust-error-handling");
    assert_eq!(output["review"]["reviews"][0]["status"], "invalid_output");
    assert_eq!(output["review"]["reviews"][0]["actionable_count"], 0);
    assert!(
        output["review"]["reviews"][0]["parse_error"]
            .as_str()
            .unwrap()
            .contains("valid structured JSON")
    );

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();
    assert_eq!(gates["gates"][0]["status"], "invalid_output", "{gates:#}");
    assert_eq!(gates["failed_required"][0], "rust-error-handling");
    assert!(
        gates["gates"][0]["parse_error"]
            .as_str()
            .unwrap()
            .contains("valid structured JSON")
    );
}

#[test]
fn work_refine_reports_failed_checks_without_aborting() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo_with_check(temp.path(), "printf 'check failed\\n'; exit 9");
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_clean_review_codex_stub(&codex_path);
    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Refine(
            crate::cli::WorkRefineOpts {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
                max_iterations: 1,
            },
        )),
    )
    .unwrap();

    assert_eq!(output["status"], "failed", "{output:#}");
    assert_eq!(output["review"]["status"], "passed");
    assert_eq!(output["checks"]["checks"][0]["result"]["exit_status"], 9);
    assert!(
        output["checks"]["checks"][0]["receipt_id"]
            .as_str()
            .is_some()
    );
}

#[test]
fn work_refine_reports_remaining_findings_after_max_iterations() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_stubborn_review_codex_stub(&codex_path);
    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Refine(
            crate::cli::WorkRefineOpts {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
                max_iterations: 1,
            },
        )),
    )
    .unwrap();

    assert_eq!(output["status"], "failed", "{output:#}");
    assert_eq!(output["iterations"].as_array().unwrap().len(), 1);
    assert_eq!(
        output["remaining_actionable_findings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(output["failed_review_gates"][0], "rust-error-handling");
}

#[test]
fn work_refine_reports_fixer_failure_without_aborting() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_failing_refine_codex_stub(&codex_path);
    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Refine(
            crate::cli::WorkRefineOpts {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
                max_iterations: 1,
            },
        )),
    )
    .unwrap();

    assert_eq!(output["status"], "failed", "{output:#}");
    assert_eq!(output["fixer_failed"], true);
    assert_eq!(output["iterations"][0]["status"], "failed");
    assert_eq!(output["iterations"][0]["exit_status"], 42);
    assert!(output["iterations"][0]["receipt_id"].as_str().is_some());
    assert_eq!(
        output["remaining_actionable_findings"][0]["issue"],
        "post-failure review"
    );
}

#[test]
fn work_refine_requires_explicit_refinement_before_writing() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo_without_refinement(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_review_codex_stub(&codex_path);
    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Refine(
            crate::cli::WorkRefineOpts {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
                max_iterations: 1,
            },
        )),
    )
    .unwrap();

    assert_eq!(output["status"], "failed", "{output:#}");
    assert_eq!(output["refinement_required"], true);
    assert_eq!(output["iterations"].as_array().unwrap().len(), 0);
    assert!(!temp.path().join("fixed.txt").exists());
}

fn write_review_codex_stub(path: &Path) {
    // Review stubs use .agent sentinel files to model state changes between
    // review and refine iterations inside one fixture repo.
    write_codex_stub(
        path,
        r#"#!/bin/sh
if [ "$1" = "exec" ] && [ "$2" = "review" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  if [ -f .agent/clean-review ]; then
    printf '{"summary":"clean","findings":[]}\n' > "$out"
  else
    printf '{"summary":"needs work","findings":[{"severity":"critical","path":"src.rs","line":1,"issue":"missing context","evidence":"bare propagation","recommendation":"add context"}]}\n' > "$out"
  fi
  exit 0
fi
mkdir -p .agent
touch .agent/clean-review
if [ "$*" = "--ask-for-approval never exec --sandbox workspace-write --ephemeral -" ]; then
  printf 'stdin' > prompt-source.txt
fi
cat >/dev/null
printf 'fixed\n' > fixed.txt
printf 'refined\n'
"#,
    );
}

fn read_receipts(root: &Path) -> Vec<Value> {
    fs::read_to_string(root.join(".agent/state/receipts.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect()
}

fn write_invalid_review_codex_stub(path: &Path) {
    write_codex_stub(
        path,
        r#"#!/bin/sh
if [ "$1" = "exec" ] && [ "$2" = "review" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  printf 'not json\n' > "$out"
  exit 0
fi
exit 0
"#,
    );
}

fn write_many_findings_review_codex_stub(path: &Path) {
    write_codex_stub(
        path,
        r#"#!/bin/sh
if [ "$1" = "exec" ] && [ "$2" = "review" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  printf '{"summary":"many findings","findings":[' > "$out"
  i=1
  while [ "$i" -le 105 ]; do
    if [ "$i" -gt 1 ]; then
      printf ',' >> "$out"
    fi
    printf '{"severity":"critical","path":"src.rs","line":1,"issue":"issue %s","evidence":"bare propagation","recommendation":"add context"}' "$i" >> "$out"
    i=$((i + 1))
  done
  printf ']}\n' >> "$out"
  exit 0
fi
exit 0
"#,
    );
}

fn write_missing_review_output_codex_stub(path: &Path) {
    write_codex_stub(
        path,
        r#"#!/bin/sh
if [ "$1" = "exec" ] && [ "$2" = "review" ]; then
  printf 'review finished without file output\n'
  exit 0
fi
exit 0
"#,
    );
}

fn write_clean_review_codex_stub(path: &Path) {
    write_codex_stub(
        path,
        r#"#!/bin/sh
if [ "$1" = "exec" ] && [ "$2" = "review" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  printf '{"summary":"clean","findings":[]}\n' > "$out"
  exit 0
fi
exit 0
"#,
    );
}

fn write_low_finding_failed_review_codex_stub(path: &Path) {
    write_codex_stub(
        path,
        r#"#!/bin/sh
if [ "$1" = "exec" ] && [ "$2" = "review" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  printf '{"summary":"tool failed with nonblocking finding","findings":[{"severity":"suggestion","path":"src.rs","line":1,"issue":"minor style","evidence":"style only","recommendation":"cleanup later"}]}\n' > "$out"
  exit 2
fi
exit 2
"#,
    );
}

fn write_stubborn_review_codex_stub(path: &Path) {
    write_codex_stub(
        path,
        r#"#!/bin/sh
if [ "$1" = "exec" ] && [ "$2" = "review" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  printf '{"summary":"still needs work","findings":[{"severity":"critical","path":"src.rs","line":1,"issue":"still missing context","evidence":"bare propagation","recommendation":"add context"}]}\n' > "$out"
  exit 0
fi
cat >/dev/null
printf 'attempted refine\n'
"#,
    );
}

fn write_failing_refine_codex_stub(path: &Path) {
    write_codex_stub(
        path,
        r#"#!/bin/sh
if [ "$1" = "exec" ] && [ "$2" = "review" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  if [ -f .agent/refine-failed ]; then
    printf '{"summary":"still needs work","findings":[{"severity":"critical","path":"src.rs","line":1,"issue":"post-failure review","evidence":"partial fixer state","recommendation":"repair partial edits"}]}\n' > "$out"
  else
    printf '{"summary":"needs work","findings":[{"severity":"critical","path":"src.rs","line":1,"issue":"missing context","evidence":"bare propagation","recommendation":"add context"}]}\n' > "$out"
  fi
  exit 0
fi
mkdir -p .agent
touch .agent/refine-failed
cat >/dev/null
printf 'refine failed\n' >&2
exit 42
"#,
    );
}

#[test]
fn work_gates_use_direct_receipt_when_prior_batch_ended_in_same_millisecond() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let fingerprint = crate::state::current_worktree_fingerprint(&ctx)
        .fingerprint
        .expect("git fixture should produce fingerprint");

    record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: tool::WORK_CHECK,
            args: json!({ "plan_id": "plan_1", "tools": ["jig.custom_check"] }),
            plan_id: "plan_1",
            started_at_ms: 100,
            ended_at_ms: 200,
            worktree_fingerprint: Some("stale-fingerprint".into()),
        },
    );
    let direct_receipt_id = record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: "jig.custom_check",
            args: json!({}),
            plan_id: "plan_1",
            started_at_ms: 200,
            ended_at_ms: 200,
            worktree_fingerprint: Some(fingerprint),
        },
    );

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "passed");
    assert_eq!(gates["gates"][0]["status"], "passed");
    assert_eq!(gates["gates"][0]["freshness"], "fresh");
    assert_eq!(gates["gates"][0]["freshness_receipt_id"], direct_receipt_id);
}

#[test]
fn work_gates_use_legacy_batch_receipt_without_receipt_ids() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let fingerprint = crate::state::current_worktree_fingerprint(&ctx)
        .fingerprint
        .expect("git fixture should produce fingerprint");

    record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: "jig.custom_check",
            args: json!({}),
            plan_id: "plan_1",
            started_at_ms: 100,
            ended_at_ms: 110,
            worktree_fingerprint: None,
        },
    );
    let legacy_batch_receipt_id = record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: tool::WORK_CHECK,
            args: json!({ "plan_id": "plan_1", "tools": ["jig.custom_check"] }),
            plan_id: "plan_1",
            started_at_ms: 100,
            ended_at_ms: 120,
            worktree_fingerprint: Some(fingerprint),
        },
    );

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "passed");
    assert_eq!(gates["gates"][0]["status"], "passed");
    assert_eq!(gates["gates"][0]["freshness"], "fresh");
    assert_eq!(
        gates["gates"][0]["freshness_receipt_id"],
        legacy_batch_receipt_id
    );
}

#[test]
fn work_gates_use_exact_batch_receipt_id_when_batches_interleave() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let fingerprint = crate::state::current_worktree_fingerprint(&ctx)
        .fingerprint
        .expect("git fixture should produce fingerprint");

    let tool_receipt_id = record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: "jig.custom_check",
            args: json!({}),
            plan_id: "plan_1",
            started_at_ms: 100,
            ended_at_ms: 110,
            worktree_fingerprint: None,
        },
    );
    let batch_receipt_id = record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: tool::WORK_CHECK,
            args: json!({
                "plan_id": "plan_1",
                "tools": ["jig.custom_check"],
                "receipt_ids": [tool_receipt_id],
            }),
            plan_id: "plan_1",
            started_at_ms: 100,
            ended_at_ms: 120,
            worktree_fingerprint: Some(fingerprint),
        },
    );
    record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: tool::WORK_CHECK,
            args: json!({
                "plan_id": "plan_1",
                "tools": ["jig.custom_check"],
                "receipt_ids": ["receipt_other_tool"],
            }),
            plan_id: "plan_1",
            started_at_ms: 90,
            ended_at_ms: 130,
            worktree_fingerprint: Some("stale-fingerprint".into()),
        },
    );

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "passed");
    assert_eq!(gates["gates"][0]["status"], "passed");
    assert_eq!(gates["gates"][0]["freshness"], "fresh");
    assert_eq!(gates["gates"][0]["freshness_receipt_id"], batch_receipt_id);
}

#[test]
fn work_gates_keep_failed_checks_failed_when_freshness_is_unknown() {
    let temp = tempdir().unwrap();
    write_failing_check_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("jig.custom_check failed with status 7"));

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "blocked");
    assert_eq!(gates["gates"][0]["status"], "failed");
    assert_eq!(gates["gates"][0]["freshness"], "unknown");
    assert_eq!(gates["failed_required"][0], "custom");
}

#[test]
fn old_flat_memory_tool_names_are_not_supported() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = call_tool(&ctx, "jig.session_start", json!({}))
        .unwrap_err()
        .to_string();

    assert!(error.contains("Unsupported tool: jig.session_start"));
}
