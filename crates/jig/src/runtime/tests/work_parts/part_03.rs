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
    assert_eq!(batch["exit_status"], 9);
    assert_eq!(batch["evidence"]["gates"][0]["status"], "failed");
    assert_eq!(batch["evidence"]["gates"][0]["exit_status"], 9);
}

#[test]
fn work_refine_records_unknown_applicability_and_runs_later_gates() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .contract_version(5)
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
