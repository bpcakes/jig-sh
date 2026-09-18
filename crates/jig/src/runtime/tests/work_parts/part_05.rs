fn retire_opts(plan_id: &str, disposition: &str, reason: &str) -> crate::cli::WorkRetireOpts {
    crate::cli::WorkRetireOpts {
        plan_id: plan_id.into(),
        disposition: disposition.into(),
        reason: reason.into(),
        superseded_by: None,
    }
}

fn retire(ctx: &RepoContext, opts: crate::cli::WorkRetireOpts) -> Result<Value> {
    dispatch(ctx, CommandKind::Work(crate::cli::WorkCommand::Retire(opts)))
}

#[test]
fn work_retire_closes_a_plan_whose_required_gates_are_unsatisfied() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_test_plan(&ctx);

    let finish_error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Finish(
            crate::cli::WorkFinishOpts {
                plan_id: plan_id.clone(),
                resolution: Some("done".into()),
                outcome: Some("success".into()),
            },
        )),
    )
    .unwrap_err()
    .to_string();

    let mut opts = retire_opts(&plan_id, "superseded", "  Replaced by the redesign.  ");
    opts.superseded_by = Some("plan_example_002".into());
    let output = retire(&ctx, opts).unwrap();

    assert!(finish_error.contains("Required work gates are not satisfied"));
    assert_eq!(output["ok"], true);
    assert_eq!(output["plan"]["plan_id"], plan_id);
    assert_eq!(output["plan"]["retirement"]["disposition"], "superseded");
    assert_eq!(
        output["plan"]["retirement"]["reason"],
        "Replaced by the redesign."
    );
    assert_eq!(
        output["plan"]["retirement"]["superseded_by"],
        "plan_example_002"
    );
    assert!(crate::state::ensure_plan_is_open(&ctx, &plan_id).is_err());
}

#[test]
fn work_retire_writes_no_gate_evidence_and_leaves_gates_blocked() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_test_plan(&ctx);

    retire(&ctx, retire_opts(&plan_id, "obsolete", "Obsolete work.")).unwrap();

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            freshness_timeout_ms: None,
            plan_id: Some(plan_id.clone()),
        })),
    )
    .unwrap();
    let receipts = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Receipts(
            crate::cli::WorkReceiptsOpts {
                plan_id: Some(plan_id),
                ..Default::default()
            },
        )),
    )
    .unwrap();

    assert_eq!(gates["plan_state"], "closed");
    assert_eq!(gates["overall"], "blocked");
    assert_eq!(gates["plan_retirement"]["disposition"], "obsolete");
    assert_eq!(gates["plan_retirement"]["reason"], "Obsolete work.");
    assert_eq!(gates["gates"][0]["status"], "missing");
    for receipt in receipts["receipts"].as_array().unwrap() {
        assert_ne!(receipt["tool_name"], json!(tool::WORK_CHECK), "{receipt:#}");
    }
}

#[test]
fn work_retire_rejects_unknown_closed_and_invalid_requests() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_test_plan(&ctx);

    let unknown = retire(&ctx, retire_opts("plan_missing", "cancelled", "Gone."))
        .unwrap_err()
        .to_string();
    let blank_reason = retire(&ctx, retire_opts(&plan_id, "cancelled", "   \n\t"))
        .unwrap_err()
        .to_string();
    let unknown_disposition = retire(&ctx, retire_opts(&plan_id, "finished", "Reason."))
        .unwrap_err()
        .to_string();
    let mut blank_reference = retire_opts(&plan_id, "superseded", "Reason.");
    blank_reference.superseded_by = Some("   ".into());
    let blank_superseded_by = retire(&ctx, blank_reference).unwrap_err().to_string();

    retire(&ctx, retire_opts(&plan_id, "duplicate", "Duplicate work.")).unwrap();
    let already_closed = retire(&ctx, retire_opts(&plan_id, "duplicate", "Again."))
        .unwrap_err()
        .to_string();

    assert!(unknown.contains("Plan not found: plan_missing"), "{unknown}");
    assert!(!unknown.contains("Required work gates"), "{unknown}");
    assert!(blank_reason.contains("nonblank --reason"), "{blank_reason}");
    assert!(
        unknown_disposition.contains("Unknown work plan disposition 'finished'"),
        "{unknown_disposition}"
    );
    assert!(
        unknown_disposition.contains("cancelled, superseded, duplicate, obsolete"),
        "{unknown_disposition}"
    );
    assert!(
        blank_superseded_by.contains("--superseded-by must name a plan or issue reference"),
        "{blank_superseded_by}"
    );
    assert!(
        already_closed.contains(&format!("Plan is already closed: {plan_id}")),
        "{already_closed}"
    );
}

#[test]
fn work_retire_ends_only_a_session_that_durable_state_proves_owns_the_plan() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let owned = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Start(crate::cli::WorkStartOpts {
            title: "ExampleProject plan A".into(),
            body: Some("A".into()),
            body_file: None,
            base: None,
            print_plan_id: false,
        })),
    )
    .unwrap();
    let owned_plan = owned["plan"]["plan_id"].as_str().unwrap().to_string();
    let owned_session = owned["session"]["session_id"].as_str().unwrap().to_string();

    let unrelated = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Start(crate::cli::WorkStartOpts {
            title: "ExampleProject plan B".into(),
            body: Some("B".into()),
            body_file: None,
            base: None,
            print_plan_id: false,
        })),
    )
    .unwrap();
    let unrelated_session = unrelated["session"]["session_id"].as_str().unwrap().to_string();

    // The current session belongs to the other plan, so it must survive.
    let left_active = retire(&ctx, retire_opts(&owned_plan, "obsolete", "Obsolete.")).unwrap();
    assert_eq!(left_active["session"], Value::Null);
    assert_eq!(left_active["session_status"]["action"], "left_active");
    assert_eq!(
        left_active["session_status"]["owner_session_id"],
        owned_session
    );
    assert_eq!(
        left_active["session_status"]["current_session_id"],
        unrelated_session
    );
    assert_eq!(
        crate::state::current_session(&ctx).unwrap(),
        Some(unrelated_session.clone())
    );

    // Retiring the plan the current session actually opened ends that session.
    let unrelated_plan = unrelated["plan"]["plan_id"].as_str().unwrap().to_string();
    let ended = retire(&ctx, retire_opts(&unrelated_plan, "cancelled", "Cancelled.")).unwrap();

    assert_eq!(ended["session_status"]["action"], "ended");
    assert_eq!(ended["session"]["session_id"], unrelated_session);
    assert_eq!(crate::state::current_session(&ctx).unwrap(), None);
}

#[test]
fn work_finish_also_leaves_an_unrelated_current_session_active() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let first = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Start(crate::cli::WorkStartOpts {
            title: "ExampleProject plan A".into(),
            body: Some("A".into()),
            body_file: None,
            base: None,
            print_plan_id: false,
        })),
    )
    .unwrap();
    let first_plan = first["plan"]["plan_id"].as_str().unwrap().to_string();
    let first_session = first["session"]["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let second = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Start(crate::cli::WorkStartOpts {
            title: "ExampleProject plan B".into(),
            body: Some("B".into()),
            body_file: None,
            base: None,
            print_plan_id: false,
        })),
    )
    .unwrap();
    let second_session = second["session"]["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let output = crate::runtime::work::finish_after_required_gates_passed(
        &ctx,
        crate::command::WorkFinishRequest {
            plan_id: first_plan,
            resolution: Some("Delivered.".into()),
            outcome: Some("success".into()),
        },
        crate::runtime::work::RequiredGateProof::default(),
        &|| false,
    )
    .unwrap();

    assert_eq!(output["session"], Value::Null);
    assert_eq!(output["session_status"]["action"], "left_active");
    assert_eq!(output["session_status"]["owner_session_id"], first_session);
    assert_eq!(
        output["session_status"]["current_session_id"],
        second_session
    );
    assert_eq!(
        crate::state::current_session(&ctx).unwrap(),
        Some(second_session)
    );
}

#[test]
fn work_retire_appends_a_backward_compatible_close_event_and_receipt() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_test_plan(&ctx);

    let before = fs::read_to_string(ctx.state_file("plans.jsonl")).unwrap();
    let output = retire(
        &ctx,
        retire_opts(&plan_id, "cancelled", "Delivery was cancelled."),
    )
    .unwrap();
    let after = fs::read_to_string(ctx.state_file("plans.jsonl")).unwrap();

    assert!(after.starts_with(&before), "plan history was rewritten");
    let close: Value = serde_json::from_str(after.lines().next_back().unwrap()).unwrap();
    assert_eq!(close["event"], "close");
    assert_eq!(close["plan_id"], plan_id);
    assert_eq!(close["retirement"]["disposition"], "cancelled");
    assert_eq!(close["retirement"]["reason"], "Delivery was cancelled.");
    assert_eq!(close["retirement"].get("superseded_by"), None);
    // The historical free-text field stays populated for readers that only
    // know `resolution`.
    assert_eq!(close["resolution"], "cancelled: Delivery was cancelled.");

    let receipt_id = output["plan"]["receipt_id"].as_str().unwrap();
    let receipt = fs::read_to_string(ctx.state_file("receipts.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|receipt| receipt["id"] == receipt_id)
        .unwrap();
    assert_eq!(receipt["tool_name"], tool::PLANS_CLOSE);
    assert_eq!(receipt["args"]["operation"], "plan_retire");
    assert_eq!(receipt["args"]["disposition"], "cancelled");
    assert_eq!(receipt["args"]["reason"], "Delivery was cancelled.");
    assert_eq!(receipt["exit_status"], 0);
}

#[test]
fn work_retire_over_mcp_matches_the_cli_and_is_advertised() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_test_plan(&ctx);

    let descriptor = crate::tool_defs::tool_descriptors(ctx.contract_version(), ctx.tool_specs())
        .into_iter()
        .find(|descriptor| descriptor["name"] == tool::WORK_RETIRE)
        .unwrap();
    let validator = jsonschema::validator_for(&descriptor["inputSchema"]).unwrap();

    let output = call_tool(
        &ctx,
        tool::WORK_RETIRE,
        json!({
            "plan_id": plan_id,
            "disposition": "duplicate",
            "reason": "Duplicate of an existing plan.",
            "superseded_by": "plan_example_002"
        }),
    )
    .unwrap();

    assert!(validator.is_valid(&json!({
        "plan_id": plan_id,
        "disposition": "cancelled",
        "reason": "Cancelled."
    })));
    assert!(!validator.is_valid(&json!({"plan_id": plan_id, "disposition": "cancelled"})));
    assert!(!validator.is_valid(&json!({
        "plan_id": plan_id,
        "disposition": "finished",
        "reason": "Cancelled."
    })));
    assert!(!validator.is_valid(&json!({
        "plan_id": plan_id,
        "disposition": "cancelled",
        "reason": "  "
    })));
    assert_eq!(output["ok"], true);
    assert_eq!(output["plan"]["retirement"]["disposition"], "duplicate");
    assert_eq!(
        output["plan"]["retirement"]["superseded_by"],
        "plan_example_002"
    );
    assert_eq!(output["session_status"]["action"], "left_active");
}
