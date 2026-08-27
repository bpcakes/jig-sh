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
        CommandKind::Check(crate::cli::CheckCommand::Fmt(crate::cli::ToolOpts {
            plan_id: None,
            no_receipt: false,
        })),
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
        CommandKind::Check(crate::cli::CheckCommand::Schema(crate::cli::ToolOpts {
            plan_id: None,
            no_receipt: false,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("jig.schema_check is not available"));
    assert!(error.contains("sqlx_enabled = false"));
    assert!(error.contains("jig update --recopy"));
}

#[test]
fn unavailable_typescript_check_explains_missing_contract_tool() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
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
        CommandKind::Check(crate::cli::CheckCommand::TypeScriptLint(
            crate::cli::ToolOpts {
                plan_id: None,
                no_receipt: false,
            },
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
            base: None,
            print_plan_id: false,
        },
        crate::cli::WorkStartOpts {
            title: "Missing body file".into(),
            body: None,
            body_file: Some(temp.path().join("missing-plan.md")),
            base: None,
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
fn work_start_resolves_explicit_baseline_before_writing_state() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::write(temp.path().join("tracked.txt"), "initial\n").unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plans_path = ctx.state_file("plans.jsonl");
    let sessions_path = ctx.state_file("sessions.jsonl");
    let receipts_path = ctx.state_file("receipts.jsonl");
    let before = [
        fs::read_to_string(&plans_path).unwrap_or_default(),
        fs::read_to_string(&sessions_path).unwrap_or_default(),
        fs::read_to_string(&receipts_path).unwrap_or_default(),
    ];

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Start(crate::cli::WorkStartOpts {
            title: "Invalid baseline".into(),
            body: Some("Must not be written".into()),
            body_file: None,
            base: Some("refs/heads/does-not-exist".into()),
            print_plan_id: false,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Failed to resolve explicit plan baseline ref"));
    assert_eq!(
        fs::read_to_string(&plans_path).unwrap_or_default(),
        before[0]
    );
    assert_eq!(
        fs::read_to_string(&sessions_path).unwrap_or_default(),
        before[1]
    );
    assert_eq!(
        fs::read_to_string(&receipts_path).unwrap_or_default(),
        before[2]
    );
    assert_eq!(crate::state::current_session(&ctx).unwrap(), None);

    let opened = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Start(crate::cli::WorkStartOpts {
            title: "Explicit baseline".into(),
            body: Some("Store the exact commit".into()),
            body_file: None,
            base: Some("HEAD".into()),
            print_plan_id: false,
        })),
    )
    .unwrap();
    assert_eq!(opened["plan"]["baseline"]["requested_ref"], "HEAD");
    assert_eq!(
        opened["plan"]["baseline"]["commit_oid"],
        crate::git_receipts::resolve_git_commit(temp.path(), "HEAD").unwrap()
    );
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
            gates: Vec::new(),
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
fn work_check_classifies_paths_preserves_scoped_freshness_and_reuses_exact_evidence() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
        .config(
            r#"
[commands]
frontend_check_command = "printf 'frontend checked\n'"
rust_check_command = "printf 'rust checked\n'"

[[work.gates]]
id = "frontend"
kind = "check"
tool = "jig.frontend_check"
paths = ["apps/**"]
reuse = true

[[work.gates]]
id = "frontend-secondary"
kind = "check"
tool = "jig.frontend_check"
paths = ["apps/**"]
reuse = true

[[work.gates]]
id = "rust"
kind = "check"
tool = "jig.rust_check"
paths = ["crates/**", "Cargo.toml"]
"#,
        )
        .required_commands(["frontend_check_command", "rust_check_command"])
        .tool(json!({
            "name": "jig.frontend_check",
            "kind": "command",
            "description": "Run frontend checks.",
            "command": "frontend_check_command"
        }))
        .tool(json!({
            "name": "jig.rust_check",
            "kind": "command",
            "description": "Run Rust checks.",
            "command": "rust_check_command"
        }))
        .write();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::create_dir_all(temp.path().join("crates/api/src")).unwrap();
    fs::write(
        temp.path().join("apps/web/main.ts"),
        "export const v = 1;\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("crates/api/src/lib.rs"),
        "pub const V: u8 = 1;\n",
    )
    .unwrap();
    init_git_repo(temp.path());
    // Dirty work that exists before the plan opens remains part of the
    // baseline-relative affected set.
    fs::write(
        temp.path().join("apps/web/main.ts"),
        "export const v = 2;\n",
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Frontend change".into(),
            body: Some("Change one frontend".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap().to_string();
    assert!(plan["baseline"]["commit_oid"].as_str().is_some());

    let checked = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: plan_id.clone(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();
    assert_eq!(checked["checks"].as_array().unwrap().len(), 2);
    assert_eq!(checked["checks"][0]["gate_id"], "frontend");
    assert_eq!(checked["checks"][1]["gate_id"], "frontend-secondary");
    assert_eq!(checked["gate_evidence"][0]["status"], "executed");
    assert_eq!(checked["gate_evidence"][1]["status"], "executed");
    assert_eq!(checked["gate_evidence"][2]["status"], "not_applicable");

    let same_plan = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: plan_id.clone(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();
    assert_eq!(
        same_plan["gate_evidence"][0]["status"], "executed",
        "reuse is cross-plan only"
    );
    assert_eq!(same_plan["gate_evidence"][1]["status"], "executed");

    fs::write(temp.path().join("notes.md"), "unrelated\n").unwrap();
    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some(plan_id.clone()),
        })),
    )
    .unwrap();
    assert_eq!(gates["overall"], "passed");
    assert_eq!(gates["gates"][0]["status"], "passed");
    assert_eq!(gates["gates"][1]["status"], "passed");
    assert_eq!(gates["gates"][2]["status"], "not_applicable");

    fs::write(
        temp.path().join("apps/web/main.ts"),
        "export const v = 3;\n",
    )
    .unwrap();
    let stale = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some(plan_id),
        })),
    )
    .unwrap();
    assert_eq!(stale["gates"][0]["status"], "stale");
    fs::write(
        temp.path().join("apps/web/main.ts"),
        "export const v = 2;\n",
    )
    .unwrap();

    let follow_up = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Frontend follow-up".into(),
            body: Some("Verify identical inputs".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let follow_up_id = follow_up["plan_id"].as_str().unwrap().to_string();
    crate::state::reset_reusable_work_check_scan_count();
    let reused = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: follow_up_id.clone(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();
    assert!(reused["checks"].as_array().unwrap().is_empty());
    assert_eq!(reused["gate_evidence"][0]["status"], "reused");
    assert_eq!(reused["gate_evidence"][1]["status"], "reused");
    assert_eq!(crate::state::reusable_work_check_scan_count(), 1);
    assert!(
        reused["gate_evidence"][0]["source_tool_receipt_id"]
            .as_str()
            .is_some()
    );
    let direct_source_plan = reused["gate_evidence"][0]["source_plan_id"]
        .as_str()
        .unwrap()
        .to_string();

    let second_follow_up = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Second frontend follow-up".into(),
            body: Some("Reuse the original direct proof through an inert attestation".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let reused_again = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: second_follow_up["plan_id"].as_str().unwrap().to_string(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();
    assert!(reused_again["checks"].as_array().unwrap().is_empty());
    assert_eq!(reused_again["gate_evidence"][0]["status"], "reused");
    assert_eq!(
        reused_again["gate_evidence"][0]["source_plan_id"],
        direct_source_plan
    );

    let follow_up_rerun = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: follow_up_id,
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();
    assert_eq!(follow_up_rerun["checks"].as_array().unwrap().len(), 2);
    assert_eq!(follow_up_rerun["gate_evidence"][0]["status"], "executed");
    assert_eq!(follow_up_rerun["gate_evidence"][1]["status"], "executed");
}

#[test]
fn newer_failed_exact_evidence_supersedes_an_older_reusable_pass() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
        .config(
            r#"
[commands]
scoped_check_command = "test ! -f .agent/reuse-must-fail"

[[work.gates]]
id = "scoped"
kind = "check"
tool = "jig.scoped_check"
paths = ["src/**"]
reuse = true
"#,
        )
        .required_commands(["scoped_check_command"])
        .tool(json!({
            "name": "jig.scoped_check",
            "kind": "command",
            "description": "Run a scoped fixture check.",
            "command": "scoped_check_command"
        }))
        .write();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("src/lib.rs"), "pub const V: u8 = 1;\n").unwrap();
    init_git_repo(temp.path());
    fs::write(temp.path().join("src/lib.rs"), "pub const V: u8 = 2;\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let passing_plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Passing evidence".into(),
            body: Some("Record a reusable direct pass".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: passing_plan["plan_id"].as_str().unwrap().to_string(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    fs::write(temp.path().join(".agent/reuse-must-fail"), "fail\n").unwrap();
    let failing_plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Failed evidence".into(),
            body: Some("Force the same input to fail".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: failing_plan["plan_id"].as_str().unwrap().to_string(),
            gates: vec!["scoped".into()],
            tools: Vec::new(),
        })),
    )
    .unwrap_err();

    let follow_up = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Follow-up evidence".into(),
            body: Some("Do not resurrect the older pass".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: follow_up["plan_id"].as_str().unwrap().to_string(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("jig.scoped_check"), "{error}");
}

#[test]
fn rust_tool_configuration_changes_select_only_the_owning_gate() {
    let _env = lock_env();
    for (changed_path, expected_gate) in [
        ("rustfmt.toml", "rust-fmt"),
        ("clippy.toml", "rust-clippy"),
        (".config/nextest.toml", "rust-tests"),
    ] {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
            .config(
                r#"
[commands]
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace"
rust_test_command = "cargo test --workspace"

[[work.gates]]
id = "rust-fmt"
kind = "check"
tool = "jig.fmt_check"
paths = ["crates/**", "Cargo.toml", "Cargo.lock", "rust-toolchain*", "rustfmt.toml", ".rustfmt.toml"]

[[work.gates]]
id = "rust-clippy"
kind = "check"
tool = "jig.clippy"
paths = ["crates/**", "Cargo.toml", "Cargo.lock", "rust-toolchain*", "clippy.toml", ".clippy.toml"]

[[work.gates]]
id = "rust-tests"
kind = "check"
tool = "jig.test"
paths = ["crates/**", "Cargo.toml", "Cargo.lock", "rust-toolchain*", "nextest.toml", ".config/nextest.toml"]
"#,
            )
            .required_commands([
                "rust_fmt_check_command",
                "rust_clippy_command",
                "rust_test_command",
            ])
            .tool(json!({
                "name": "jig.fmt_check",
                "kind": "command",
                "description": "Run Rust formatting checks.",
                "command": "rust_fmt_check_command"
            }))
            .tool(json!({
                "name": "jig.clippy",
                "kind": "command",
                "description": "Run Rust lint checks.",
                "command": "rust_clippy_command"
            }))
            .tool(json!({
                "name": "jig.test",
                "kind": "command",
                "description": "Run Rust tests.",
                "command": "rust_test_command"
            }))
            .write();
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(
            bin_dir.join("cargo"),
            "#!/bin/sh\ncase \"$1\" in fmt) label=rust-fmt ;; clippy) label=rust-clippy ;; test) label=rust-tests ;; *) exit 2 ;; esac\nprintf '%s\\n' \"$label\" >> .agent/executed\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(bin_dir.join("cargo"), fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = std::env::join_paths(std::iter::once(bin_dir.clone()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .unwrap();
        let _path = EnvVarGuard::set("PATH", path);
        fs::create_dir_all(temp.path().join("crates/example/src")).unwrap();
        fs::create_dir_all(temp.path().join(".config")).unwrap();
        fs::write(temp.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(temp.path().join("crates/example/src/lib.rs"), "").unwrap();
        fs::write(temp.path().join("rustfmt.toml"), "edition = \"2024\"\n").unwrap();
        fs::write(temp.path().join("clippy.toml"), "msrv = \"1.85\"\n").unwrap();
        fs::write(
            temp.path().join(".config/nextest.toml"),
            "[profile.default]\n",
        )
        .unwrap();
        init_git_repo(temp.path());
        fs::write(
            temp.path().join(changed_path),
            format!("# changed for {expected_gate}\n"),
        )
        .unwrap();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let plan = crate::state::plans_open(
            &ctx,
            crate::state::PlanOpenRequest {
                title: format!("Change {changed_path}"),
                body: Some("Verify generated Rust gate ownership".into()),
                body_file: None,
                base: None,
            },
        )
        .unwrap();

        let checked = dispatch(
            &ctx,
            CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
                plan_id: plan["plan_id"].as_str().unwrap().to_owned(),
                gates: Vec::new(),
                tools: Vec::new(),
            })),
        )
        .unwrap();

        assert_eq!(
            checked["checks"].as_array().unwrap().len(),
            1,
            "{checked:#}"
        );
        assert_eq!(checked["checks"][0]["gate_id"], expected_gate);
        assert_eq!(
            checked["gate_evidence"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|gate| gate["status"] == "not_applicable")
                .count(),
            2
        );
    }
}

#[test]
fn cross_plan_reuse_rejects_a_source_batch_with_worktree_mutation() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
        .config(
            r#"
[commands]
frontend_check_command = "printf mutation >> notes.md"

[[work.gates]]
id = "frontend"
kind = "check"
tool = "jig.frontend_check"
paths = ["apps/**"]
reuse = true
"#,
        )
        .required_commands(["frontend_check_command"])
        .tool(json!({
            "name": "jig.frontend_check",
            "kind": "command",
            "description": "Run a mutating frontend check.",
            "command": "frontend_check_command"
        }))
        .write();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join("apps/web/main.ts"),
        "export const v = 1;\n",
    )
    .unwrap();
    fs::write(temp.path().join("notes.md"), "notes\n").unwrap();
    init_git_repo(temp.path());
    fs::write(
        temp.path().join("apps/web/main.ts"),
        "export const v = 2;\n",
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let first = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Mutating source".into(),
            body: None,
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: first["plan_id"].as_str().unwrap().into(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    let follow_up = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Do not reuse mutation-invalid evidence".into(),
            body: None,
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let checked = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: follow_up["plan_id"].as_str().unwrap().into(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    assert_eq!(checked["checks"].as_array().unwrap().len(), 1);
    assert_eq!(checked["gate_evidence"][0]["status"], "executed");
}

#[test]
fn cross_plan_evidence_is_not_reused_without_explicit_opt_in() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
        .config(
            r#"
[commands]
frontend_check_command = "printf x >> .agent/check-runs"

[[work.gates]]
id = "frontend"
kind = "check"
tool = "jig.frontend_check"
paths = ["apps/**"]
"#,
        )
        .required_commands(["frontend_check_command"])
        .tool(json!({
            "name": "jig.frontend_check",
            "kind": "command",
            "description": "Run frontend checks.",
            "command": "frontend_check_command"
        }))
        .write();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join("apps/web/main.ts"),
        "export const v = 1;\n",
    )
    .unwrap();
    init_git_repo(temp.path());
    fs::write(
        temp.path().join("apps/web/main.ts"),
        "export const v = 2;\n",
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    for title in ["First plan", "Follow-up plan"] {
        let plan = crate::state::plans_open(
            &ctx,
            crate::state::PlanOpenRequest {
                title: title.into(),
                body: Some("Verify the same input independently".into()),
                body_file: None,
                base: None,
            },
        )
        .unwrap();
        let checked = dispatch(
            &ctx,
            CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
                plan_id: plan["plan_id"].as_str().unwrap().into(),
                gates: Vec::new(),
                tools: Vec::new(),
            })),
        )
        .unwrap();
        assert_eq!(checked["gate_evidence"][0]["status"], "executed");
    }

    assert_eq!(
        fs::read_to_string(temp.path().join(".agent/check-runs")).unwrap(),
        "xx"
    );
}

#[test]
fn default_work_check_skips_optional_gates_but_explicit_gate_force_runs() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
        .config(
            r#"
[commands]
required_check_command = "printf 'required\n'"
optional_check_command = "printf 'optional\n'"

[[work.gates]]
id = "required"
kind = "check"
tool = "jig.required_check"

[[work.gates]]
id = "optional"
kind = "check"
tool = "jig.optional_check"
required = false
"#,
        )
        .required_commands(["required_check_command", "optional_check_command"])
        .tool(json!({
            "name": "jig.required_check",
            "kind": "command",
            "description": "Required check.",
            "command": "required_check_command"
        }))
        .tool(json!({
            "name": "jig.optional_check",
            "kind": "command",
            "description": "Optional check.",
            "command": "optional_check_command"
        }))
        .write();
    write_open_plan(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let default = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();
    assert_eq!(default["checks"].as_array().unwrap().len(), 1);
    assert_eq!(default["checks"][0]["gate_id"], "required");

    let explicit = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            gates: vec!["optional".into()],
            tools: Vec::new(),
        })),
    )
    .unwrap();
    assert_eq!(explicit["checks"].as_array().unwrap().len(), 1);
    assert_eq!(explicit["checks"][0]["gate_id"], "optional");
}

#[test]
fn legacy_contract_default_work_check_still_runs_optional_gates() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .contract_version(4)
        .config(
            r#"
[commands]
required_check_command = "printf required >> .agent/legacy-runs"
optional_check_command = "printf optional >> .agent/legacy-runs"

[[work.gates]]
id = "required"
kind = "check"
tool = "jig.required_check"

[[work.gates]]
id = "optional"
kind = "check"
tool = "jig.optional_check"
required = false
"#,
        )
        .required_commands(["required_check_command", "optional_check_command"])
        .tools([
            json!({
                "name": "jig.required_check",
                "kind": "command",
                "description": "Required legacy check.",
                "command": "required_check_command"
            }),
            json!({
                "name": "jig.optional_check",
                "kind": "command",
                "description": "Optional legacy check.",
                "command": "optional_check_command"
            }),
        ])
        .write();
    write_open_plan(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let checked = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    assert_eq!(checked["checks"].as_array().unwrap().len(), 2);
    assert_eq!(checked["checks"][0]["tool"], "jig.required_check");
    assert_eq!(checked["checks"][1]["tool"], "jig.optional_check");
}

#[test]
fn legacy_explicit_tools_preserve_selector_order_and_duplicates_without_gate_expansion() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .contract_version(4)
        .config(
            r#"
[commands]
legacy_check_command = "printf x >> .agent/legacy-explicit-runs"

[[work.gates]]
id = "first"
kind = "check"
tool = "jig.legacy_check"

[[work.gates]]
id = "second"
kind = "check"
tool = "jig.legacy_check"
"#,
        )
        .required_commands(["legacy_check_command"])
        .tool(json!({
            "name": "jig.legacy_check",
            "kind": "command",
            "description": "Legacy explicit check.",
            "command": "legacy_check_command"
        }))
        .write();
    write_open_plan(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let checked = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: vec!["jig.legacy_check".into(), "jig.legacy_check".into()],
        })),
    )
    .unwrap();

    assert_eq!(checked["checks"].as_array().unwrap().len(), 2);
    assert!(
        checked["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|check| { check["tool"] == "jig.legacy_check" && check.get("gate_id").is_none() })
    );
    assert_eq!(
        fs::read_to_string(temp.path().join(".agent/legacy-explicit-runs")).unwrap(),
        "xx"
    );
}

#[test]
fn customized_generated_rust_command_is_conservatively_unconditional() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
        .config(
            r#"
rust_test_command = "scripts/test.sh"

[[work.gates]]
id = "rust-tests"
kind = "check"
tool = "jig.test"
paths = ["crates/**", "Cargo.toml"]
"#,
        )
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": "jig.test",
            "kind": "command",
            "description": "Run customized tests.",
            "command": "rust_test_command"
        }))
        .write();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::create_dir_all(temp.path().join("docs")).unwrap();
    fs::write(
        temp.path().join("scripts/test.sh"),
        "#!/bin/sh\nprintf run >> .agent/custom-test-runs\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(
            temp.path().join("scripts/test.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    fs::write(temp.path().join("docs/guide.md"), "before\n").unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Customized test wrapper".into(),
            body: Some("The wrapper may depend on any repository path.".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap().to_string();
    fs::write(temp.path().join("docs/guide.md"), "after\n").unwrap();

    let checked = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: plan_id.clone(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();
    assert_eq!(checked["gate_evidence"][0]["status"], "executed");
    assert_eq!(
        checked["gate_evidence"][0]["matching_paths"],
        json!(["docs/guide.md"])
    );
    assert!(
        checked["gate_evidence"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("classified conservatively")
    );

    fs::write(
        temp.path().join("scripts/test.sh"),
        "#!/bin/sh\nprintf changed >> .agent/custom-test-runs\n",
    )
    .unwrap();
    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some(plan_id),
        })),
    )
    .unwrap();
    assert_eq!(gates["gates"][0]["status"], "stale", "{gates:#}");
}

#[test]
fn explicit_gate_runs_when_legacy_plan_scope_is_unknown_but_cannot_close() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
        .config(
            r#"
[commands]
frontend_check_command = "printf x >> .agent/forced-runs"

[[work.gates]]
id = "frontend"
kind = "check"
tool = "jig.frontend_check"
paths = ["apps/**"]
"#,
        )
        .required_commands(["frontend_check_command"])
        .tool(json!({
            "name": "jig.frontend_check",
            "kind": "command",
            "description": "Run frontend check.",
            "command": "frontend_check_command"
        }))
        .write();
    write_open_plan(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let checked = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            gates: vec!["frontend".into()],
            tools: Vec::new(),
        })),
    )
    .unwrap();
    assert_eq!(checked["checks"].as_array().unwrap().len(), 1);
    assert_eq!(checked["gate_evidence"][0]["status"], "executed");
    assert_eq!(checked["gate_evidence"][0]["applicability"], "unknown");
    assert_eq!(checked["gate_evidence"][0]["forced"], true);

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();
    assert_eq!(gates["overall"], "blocked");
    assert_eq!(gates["gates"][0]["status"], "unknown");
}

#[test]
fn work_check_runs_four_atomic_checks_for_only_the_changed_frontend_app() {
    let temp = tempdir().unwrap();
    let operations = ["lint", "typecheck", "build", "coverage"];
    let mut commands_config = String::from(
        r#"
[[frontend_apps]]
name = "storefront"
dir = "apps/storefront"
coverage_threshold = 80

[[frontend_apps]]
name = "admin"
dir = "apps/admin"
coverage_threshold = 80

[commands]
harness_command = "true"
"#,
    );
    let mut gates_config = String::from(
        r#"
[[work.gates]]
id = "jig-contract"
kind = "check"
tool = "jig.harness_check"
"#,
    );
    let mut required_commands = vec!["harness_command".to_string()];
    let mut tools = vec![json!({
        "name": "jig.harness_check",
        "kind": "command",
        "description": "Validate Jig wiring."
        ,"command": "harness_command"
    })];
    for app in ["storefront", "admin"] {
        for operation in operations {
            let command_key = format!("typescript_{app}_{operation}_command");
            let tool_name = format!("jig.typescript_{app}_{operation}");
            commands_config.push_str(&format!(
                "{command_key} = \"scripts/check-webapps.sh app-check apps/{app} {operation}\"\n"
            ));
            gates_config.push_str(&format!(
                "\n[[work.gates]]\nid = \"typescript-{app}-{operation}\"\nkind = \"check\"\ntool = \"{tool_name}\"\npaths = [\"apps/{app}/**\", \"packages/**\"]\n"
            ));
            required_commands.push(command_key.clone());
            tools.push(json!({
                "name": tool_name,
                "kind": "command",
                "description": format!("Run {operation} for {app}."),
                "command": command_key,
            }));
        }
    }
    let config = format!("{commands_config}\n{gates_config}");
    crate::test_env::TestRepoBuilder::new(temp.path())
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
        .config(config)
        .required_commands(required_commands)
        .tools(tools)
        .write();
    fs::create_dir_all(temp.path().join("apps/storefront")).unwrap();
    fs::create_dir_all(temp.path().join("apps/admin")).unwrap();
    fs::create_dir_all(temp.path().join("packages/ui")).unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/check-webapps.sh"),
        "#!/bin/sh\n[ \"$1\" = app-check ] || exit 2\nprintf \"%s-%s\\n\" \"$(basename \"$2\")\" \"$3\" >> .agent/executed\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(
            temp.path().join("scripts/check-webapps.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    fs::write(
        temp.path().join("apps/storefront/main.ts"),
        "export const v = 1;\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/admin/main.ts"),
        "export const v = 1;\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("packages/ui/index.ts"),
        "export const ui = 1;\n",
    )
    .unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Storefront change".into(),
            body: Some("Change only storefront".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap().to_string();
    fs::write(
        temp.path().join("apps/storefront/main.ts"),
        "export const v = 2;\n",
    )
    .unwrap();

    let checked = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id,
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    let executed = fs::read_to_string(temp.path().join(".agent/executed")).unwrap();
    for operation in operations {
        assert!(
            executed.contains(&format!("storefront-{operation}")),
            "{executed}"
        );
        assert!(
            !executed.contains(&format!("admin-{operation}")),
            "{executed}"
        );
    }
    assert_eq!(checked["checks"].as_array().unwrap().len(), 5);
    assert_eq!(
        checked["gate_evidence"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|gate| gate["status"] == "not_applicable")
            .count(),
        4
    );

    fs::write(
        temp.path().join("packages/ui/index.ts"),
        "export const ui = 2;\n",
    )
    .unwrap();
    let shared = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: checked["plan_id"].as_str().unwrap().to_string(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();
    assert_eq!(shared["checks"].as_array().unwrap().len(), 9);
    assert!(
        shared["gate_evidence"]
            .as_array()
            .unwrap()
            .iter()
            .all(|gate| gate["status"] == "executed")
    );
}

#[test]
fn work_check_emits_one_balanced_phase_per_tool_with_aggregate_positions() {
    #[derive(Default)]
    struct PhaseObserver(Vec<(String, String, usize, usize)>);

    impl crate::execution::ExecutionObserver for PhaseObserver {
        fn event(&mut self, event: crate::execution::ExecutionEvent<'_>) {
            match event {
                crate::execution::ExecutionEvent::PhaseStarted { label, position } => {
                    self.0.push((
                        "started".into(),
                        label.into(),
                        position.current(),
                        position.total(),
                    ))
                }
                crate::execution::ExecutionEvent::PhaseFinished { label, .. } => {
                    self.0.push(("finished".into(), label.into(), 0, 0));
                }
                _ => {}
            }
        }
    }

    impl crate::execution::ExecutionCancellation for PhaseObserver {}

    let temp = tempdir().unwrap();
    write_mutating_check_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = PhaseObserver::default();

    crate::runtime::dispatch_with_observer(
        &ctx,
        RuntimeCommand::Work(crate::command::WorkCommand::Check(
            crate::command::WorkCheckRequest {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
                tools: Vec::new(),
            },
        )),
        &mut observer,
    )
    .unwrap();

    assert_eq!(
        observer.0,
        [
            ("started".into(), "jig.first_check".into(), 1, 2),
            ("finished".into(), "jig.first_check".into(), 0, 0),
            ("started".into(), "jig.mutating_check".into(), 2, 2),
            ("finished".into(), "jig.mutating_check".into(), 0, 0),
        ]
    );
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
            gates: Vec::new(),
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
            gates: Vec::new(),
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
            gates: Vec::new(),
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
            gates: Vec::new(),
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
fn failed_path_aware_check_is_indexed_as_failed_gate_evidence() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
        .config(
            r#"
[commands]
failing_command = "exit 7"

[[work.gates]]
id = "frontend"
kind = "check"
tool = "jig.failing"
paths = ["apps/**"]
"#,
        )
        .required_commands(["failing_command"])
        .tool(json!({
            "name": "jig.failing",
            "kind": "command",
            "description": "Fail for regression coverage.",
            "command": "failing_command"
        }))
        .write();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join("apps/web/main.ts"),
        "export const v = 1;\n",
    )
    .unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Failing frontend".into(),
            body: Some("Prove failed evidence".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap().to_string();
    fs::write(
        temp.path().join("apps/web/main.ts"),
        "export const v = 2;\n",
    )
    .unwrap();

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: plan_id.clone(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap_err();
    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some(plan_id),
        })),
    )
    .unwrap();

    assert_eq!(gates["gates"][0]["status"], "failed");
    assert_eq!(gates["gates"][0]["evidence_status"], "failed");
}

#[test]
fn path_aware_check_uses_an_empty_tree_baseline_before_the_first_commit() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
        .config(
            r#"
[commands]
initial_check_command = "printf 'initial checked\n'"

[[work.gates]]
id = "initial-rust"
kind = "check"
tool = "jig.initial_check"
paths = ["src/**"]
"#,
        )
        .required_commands(["initial_check_command"])
        .tool(json!({
            "name": "jig.initial_check",
            "kind": "command",
            "description": "Check initial files.",
            "command": "initial_check_command"
        }))
        .write();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("src/lib.rs"), "pub fn initial() {}\n").unwrap();
    run_git(temp.path(), &["init"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Initial repository".into(),
            body: Some("Validate files before the first commit".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap().to_string();

    assert!(plan["baseline"]["commit_oid"].is_null());
    assert!(plan["baseline"]["empty_tree_oid"].as_str().is_some());
    assert!(plan["baseline"]["error"].is_null());
    let checked = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id,
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();
    assert_eq!(checked["gate_evidence"][0]["status"], "executed");
    assert_eq!(
        checked["gate_evidence"][0]["matching_paths"],
        json!([".agent/jig-contract.json", ".jig.toml", "src/lib.rs"])
    );
}

#[test]
fn multi_plan_gate_snapshots_batch_baselines_and_share_the_current_fingerprint() {
    let temp = tempdir().unwrap();
    write_fail_fast_check_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    crate::state::seed_open_plan_for_test(
        &ctx,
        "plan_2",
        "Second test plan",
        "# Second test plan\n",
    )
    .unwrap();
    init_git_repo(temp.path());
    crate::state::reset_plan_baseline_scan_count();
    crate::git_receipts::reset_worktree_fingerprint_collection_count();

    let snapshots = super::super::work::open_plan_gate_snapshots_with_cancellation(
        &ctx,
        &["plan_1".into(), "plan_2".into()],
        &|| false,
    )
    .unwrap();

    assert_eq!(snapshots.len(), 2);
    assert_eq!(crate::state::plan_baseline_scan_count(), 1);
    assert_eq!(
        crate::git_receipts::worktree_fingerprint_collection_count(),
        1
    );
}

#[test]
fn multi_plan_gate_snapshots_share_same_baseline_change_and_policy_proofs() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
        .config(
            r#"
rust_test_command = "cargo test"

[[work.gates]]
id = "rust-tests"
kind = "check"
tool = "jig.test"
paths = ["src/**"]
"#,
        )
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": "jig.test",
            "kind": "command",
            "description": "Run tests.",
            "command": "rust_test_command"
        }))
        .write();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("src/lib.rs"), "pub fn before() {}\n").unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let first = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "First same-baseline plan".into(),
            body: Some("Validate shared proof collection.".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let second = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Second same-baseline plan".into(),
            body: Some("Validate shared proof collection again.".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let plan_ids = vec![
        first["plan_id"].as_str().unwrap().to_string(),
        second["plan_id"].as_str().unwrap().to_string(),
    ];
    fs::write(temp.path().join("src/lib.rs"), "pub fn after() {}\n").unwrap();
    crate::git_receipts::reset_gate_scope_collection_counts();

    let snapshots =
        super::super::work::open_plan_gate_snapshots_with_cancellation(&ctx, &plan_ids, &|| false)
            .unwrap();

    assert_eq!(snapshots.len(), 2);
    assert_eq!(crate::git_receipts::plan_change_collection_count(), 1);
    assert_eq!(crate::git_receipts::gate_scope_input_collection_count(), 1);
}

#[test]
fn cancelled_collect_all_work_check_stops_unstarted_tools() {
    struct CancelWhenStarted(std::path::PathBuf);

    impl crate::execution::ExecutionObserver for CancelWhenStarted {}

    impl crate::execution::ExecutionCancellation for CancelWhenStarted {
        fn cancelled(&self) -> bool {
            self.0.exists()
        }
    }

    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
[commands]
first_check_command = "printf started > first-check-started; sleep 30"
later_check_command = "printf ran > later-check-ran"

[[work.gates]]
id = "first"
kind = "check"
tool = "jig.first_check"

[[work.gates]]
id = "later"
kind = "check"
tool = "jig.later_check"
"#,
        )
        .required_commands(["first_check_command", "later_check_command"])
        .tool(json!({
            "name": "jig.first_check",
            "kind": "command",
            "description": "Run the first fixture check.",
            "command": "first_check_command"
        }))
        .tool(json!({
            "name": "jig.later_check",
            "kind": "command",
            "description": "Run the later fixture check.",
            "command": "later_check_command"
        }))
        .write();
    write_open_plan(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = CancelWhenStarted(temp.path().join("first-check-started"));

    let error = super::super::work::check_tools_collect_failures_with_observer(
        &ctx,
        "plan_1",
        vec!["jig.first_check".into(), "jig.later_check".into()],
        &mut observer,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("cancelled"), "{error}");
    assert!(!temp.path().join("later-check-ran").exists());
    let receipts = read_receipts(temp.path());
    let child = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.first_check")
        .expect("started cancelled check should record a child receipt");
    assert_eq!(child["evidence"]["status"], "cancelled");
    let batch = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.work_check")
        .expect("cancelled work check should record its batch receipt");
    assert_eq!(batch["args"]["receipt_ids"], json!([child["id"]]));
    assert!(
        receipts
            .iter()
            .all(|receipt| receipt["tool_name"] != "jig.later_check")
    );
}

#[test]
fn cancelled_gate_rerun_supersedes_every_selected_gate_from_an_older_pass() {
    struct CancelWhenStarted(std::path::PathBuf);

    impl crate::execution::ExecutionObserver for CancelWhenStarted {}

    impl crate::execution::ExecutionCancellation for CancelWhenStarted {
        fn cancelled(&self) -> bool {
            self.0.exists()
        }
    }

    let temp = tempdir().unwrap();
    let control = tempdir().unwrap();
    let cancel_mode = control.path().join("cancel-mode");
    let started = control.path().join("first-started");
    TestRepoBuilder::new(temp.path())
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
        .config(format!(
            r#"
[commands]
first_check_command = "if [ -f '{cancel_mode}' ]; then printf started > '{started}'; sleep 30; fi"
later_check_command = "printf later"

[[work.gates]]
id = "first"
kind = "check"
tool = "jig.first_check"

[[work.gates]]
id = "later"
kind = "check"
tool = "jig.later_check"
"#,
            cancel_mode = cancel_mode.display(),
            started = started.display(),
        ))
        .required_commands(["first_check_command", "later_check_command"])
        .tool(json!({
            "name": "jig.first_check",
            "kind": "command",
            "description": "Run the first fixture check.",
            "command": "first_check_command"
        }))
        .tool(json!({
            "name": "jig.later_check",
            "kind": "command",
            "description": "Run the later fixture check.",
            "command": "later_check_command"
        }))
        .write();
    write_open_plan(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();
    fs::write(&cancel_mode, "cancel\n").unwrap();
    let mut observer = CancelWhenStarted(started);

    let error = crate::runtime::dispatch_with_observer(
        &ctx,
        RuntimeCommand::Work(crate::command::WorkCommand::Check(
            crate::command::WorkCheckRequest {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
                tools: Vec::new(),
            },
        )),
        &mut observer,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("cancelled"));

    let batch = read_receipts(temp.path())
        .into_iter()
        .rev()
        .find(|receipt| receipt["tool_name"] == "jig.work_check")
        .unwrap();
    assert_eq!(batch["args"]["gates"], json!(["first", "later"]));
    assert_eq!(batch["evidence"]["gates"][0]["status"], "cancelled");
    assert_eq!(batch["evidence"]["gates"][1]["status"], "unknown");

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();
    assert_eq!(gates["gates"][0]["status"], "failed", "{gates:#}");
    assert_eq!(gates["gates"][1]["status"], "unknown", "{gates:#}");
    assert!(
        gates["gates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|gate| { gate["freshness_receipt_id"] == batch["id"] })
    );
}

#[test]
fn cancelled_collect_all_work_check_stops_after_a_native_tool() {
    #[derive(Default)]
    struct CancelAfterNativePhase {
        native_finished: bool,
    }

    impl crate::execution::ExecutionObserver for CancelAfterNativePhase {
        fn event(&mut self, event: crate::execution::ExecutionEvent<'_>) {
            if matches!(
                event,
                crate::execution::ExecutionEvent::PhaseFinished {
                    label: crate::tool_defs::tool::CONTRACT_CHECK,
                    ..
                }
            ) {
                self.native_finished = true;
            }
        }
    }

    impl crate::execution::ExecutionCancellation for CancelAfterNativePhase {
        fn cancelled(&self) -> bool {
            self.native_finished
        }
    }

    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
[commands]
later_check_command = "printf ran > later-check-ran"
"#,
        )
        .required_commands(["later_check_command"])
        .tool(json!({
            "name": crate::tool_defs::tool::CONTRACT_CHECK,
            "kind": "native",
            "description": "Check the fixture contract."
        }))
        .tool(json!({
            "name": "jig.later_check",
            "kind": "command",
            "description": "Run the later fixture check.",
            "command": "later_check_command"
        }))
        .write();
    write_open_plan(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = CancelAfterNativePhase::default();

    let error = super::super::work::check_tools_collect_failures_with_observer(
        &ctx,
        "plan_1",
        vec![
            crate::tool_defs::tool::CONTRACT_CHECK.into(),
            "jig.later_check".into(),
        ],
        &mut observer,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("cancelled"), "{error}");
    assert!(!temp.path().join("later-check-ran").exists());
    let receipts = read_receipts(temp.path());
    assert!(
        receipts
            .iter()
            .any(|receipt| receipt["tool_name"] == crate::tool_defs::tool::CONTRACT_CHECK)
    );
    assert!(
        receipts
            .iter()
            .all(|receipt| receipt["tool_name"] != "jig.later_check")
    );
}

#[test]
fn timed_out_work_check_records_child_and_batch_failure_receipts() {
    let temp = tempdir().unwrap();
    write_timeout_check_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let started = std::time::Instant::now();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("timed out"), "{error}");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    let receipts = read_receipts(temp.path());
    let child = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.timeout_check")
        .expect("timed-out configured check should record a child receipt");
    assert_eq!(child["exit_status"], 1);
    assert!(
        child["stderr_preview"]
            .as_str()
            .unwrap()
            .contains("timed out")
    );
    assert_eq!(child["evidence"]["kind"], "supervised_command");
    assert_eq!(child["evidence"]["status"], "error");

    let batch = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.work_check")
        .expect("failed work check should record a batch receipt");
    assert_eq!(batch["exit_status"], 1);
    assert!(
        batch["stderr_preview"]
            .as_str()
            .unwrap()
            .contains("timed out")
    );
    assert_eq!(batch["args"]["receipt_ids"][0], child["id"]);
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
            gates: Vec::new(),
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
        super::super::open_plan_gate_snapshots_with_cancellation(&ctx, &plan_ids, &|| false)
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
            gates: Vec::new(),
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
fn empty_open_plan_gate_batch_skips_fingerprint_collection() {
    use std::cell::Cell;

    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let cancellation_checks = Cell::new(0);

    let snapshots = super::super::open_plan_gate_snapshots_with_cancellation(&ctx, &[], &|| {
        let current = cancellation_checks.get();
        cancellation_checks.set(current + 1);
        current > 0
    })
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
            plan_id: "plan_1".into(),
            gates: Vec::new(),
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
            gates: Vec::new(),
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
            base: None,
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
            gates: Vec::new(),
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
            gates: Vec::new(),
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
            gates: Vec::new(),
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
    #[derive(Default)]
    struct PhaseObserver(Vec<(String, String, usize, usize)>);

    impl crate::execution::ExecutionObserver for PhaseObserver {
        fn event(&mut self, event: crate::execution::ExecutionEvent<'_>) {
            match event {
                crate::execution::ExecutionEvent::PhaseStarted { label, position } => {
                    self.0.push((
                        "started".into(),
                        label.into(),
                        position.current(),
                        position.total(),
                    ))
                }
                crate::execution::ExecutionEvent::PhaseFinished { label, .. } => {
                    self.0.push(("finished".into(), label.into(), 0, 0));
                }
                _ => {}
            }
        }
    }

    impl crate::execution::ExecutionCancellation for PhaseObserver {}

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_review_codex_stub(&codex_path);
    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = PhaseObserver::default();

    let output = crate::runtime::dispatch_with_observer(
        &ctx,
        RuntimeCommand::Work(crate::command::WorkCommand::Review(
            crate::command::WorkReviewRequest {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
            },
        )),
        &mut observer,
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
    assert_eq!(
        observer.0,
        [
            ("started".into(), "rust-error-handling".into(), 1, 1),
            ("finished".into(), "rust-error-handling".into(), 0, 0),
        ]
    );
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
fn work_refine_keeps_edit_and_iteration_evidence_after_transcript_overflow() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_verbose_refine_codex_stub(&codex_path);
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
    assert!(output["iterations"][0]["receipt_id"].as_str().is_some());
    assert_eq!(
        fs::read_to_string(temp.path().join("fixed.txt")).unwrap(),
        "fixed\n"
    );
    let receipts = read_receipts(temp.path());
    let worker_receipt = receipts
        .iter()
        .find(|receipt| {
            receipt["tool_name"] == WORKER_RUN_TOOL
                && receipt["evidence"]["purpose"] == "work_refine"
        })
        .expect("verbose refinement should record its worker receipt");
    assert_eq!(worker_receipt["evidence"]["stderr_truncated"], true);
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

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();
    let failed_check = gates["gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "custom")
        .unwrap();
    assert_eq!(failed_check["status"], "failed", "{gates:#}");
    assert_eq!(failed_check["exit_status"], 9, "{gates:#}");

    let batch = read_receipts(temp.path())
        .into_iter()
        .rev()
        .find(|receipt| receipt["tool_name"] == "jig.work_check")
        .unwrap();
    assert_eq!(batch["exit_status"], 0);
    assert_eq!(batch["evidence"]["gates"][0]["status"], "failed");
    assert_eq!(batch["evidence"]["gates"][0]["exit_status"], 9);
}

#[test]
fn work_refine_records_unknown_applicability_and_runs_later_gates() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
        .config(
            r#"
[commands]
unknown_check_command = "printf 'must not run\n'"
later_check_command = "printf ran > later-check-ran.txt"

[[work.gates]]
id = "rust-error-handling"
kind = "codex_review"
skill = "jig-rust:rust-error-handling-review"
severity = "high"

[[work.gates]]
id = "unknown-scope"
kind = "check"
tool = "jig.unknown_check"
paths = ["src/**"]

[[work.gates]]
id = "later"
kind = "check"
tool = "jig.later_check"

[[work.refinements]]
id = "test-refinement"
skill = "jig-rust:rust-simplify"
"#,
        )
        .required_commands(["unknown_check_command", "later_check_command"])
        .tool(json!({
            "name": "jig.unknown_check",
            "kind": "command",
            "description": "Unknown-scope check.",
            "command": "unknown_check_command"
        }))
        .tool(json!({
            "name": "jig.later_check",
            "kind": "command",
            "description": "Later check.",
            "command": "later_check_command"
        }))
        .write();
    write_open_plan(temp.path());
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
    assert_eq!(output["checks"]["gate_evidence"][0]["status"], "unknown");
    assert_eq!(output["checks"]["gate_evidence"][1]["status"], "executed");
    assert!(temp.path().join("later-check-ran.txt").exists());

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();
    let unknown = gates["gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "unknown-scope")
        .unwrap();
    assert_eq!(unknown["status"], "unknown", "{gates:#}");
    assert_eq!(unknown["exit_status"], serde_json::Value::Null, "{gates:#}");
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
if [ "$#" -ne 9 ] || [ "$1 $2 $3 $4 $5 $6 $7" != "--ask-for-approval never exec --sandbox workspace-write --ephemeral -o" ] || [ -z "$8" ] || [ "$9" != "-" ]; then
  echo "unexpected refine args: $*" >&2
  exit 2
fi
printf 'stdin' > prompt-source.txt
printf 'refined\n' > "$8"
cat >/dev/null
printf 'fixed\n' > fixed.txt
"#,
    );
}

fn write_verbose_refine_codex_stub(path: &Path) {
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
if [ "$#" -ne 9 ] || [ "$1 $2 $3 $4 $5 $6 $7" != "--ask-for-approval never exec --sandbox workspace-write --ephemeral -o" ] || [ -z "$8" ] || [ "$9" != "-" ]; then
  echo "unexpected verbose refine args: $*" >&2
  exit 2
fi
cat >/dev/null
printf 'refined\n' > "$8"
printf 'fixed\n' > fixed.txt
head -c 4194305 /dev/zero >&2
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
            gates: Vec::new(),
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
