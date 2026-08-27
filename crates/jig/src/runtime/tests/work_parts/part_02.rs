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
