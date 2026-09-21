use super::*;

mod cross_plan;
mod legacy_source_drift;
mod prerequisites;
mod reuse_race;
mod selection;
mod source_drift;

#[derive(Default)]
struct IterationWaitObserver {
    wait_notice: Option<std::sync::mpsc::SyncSender<()>>,
}

impl crate::execution::ExecutionObserver for IterationWaitObserver {
    fn event(&mut self, event: crate::execution::ExecutionEvent<'_>) {
        if let crate::execution::ExecutionEvent::Output { bytes, .. } = event
            && bytes
                .windows("Waiting for another repository execution".len())
                .any(|window| window == b"Waiting for another repository execution")
            && let Some(notice) = self.wait_notice.take()
        {
            notice.send(()).unwrap();
        }
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

impl crate::execution::ExecutionCancellation for IterationWaitObserver {}

fn work_check(ctx: &RepoContext, phase: &str, explain: bool) -> Value {
    work_check_for_plan(ctx, "plan_1", phase, explain)
}

fn work_check_for_plan(ctx: &RepoContext, plan_id: &str, phase: &str, explain: bool) -> Value {
    dispatch(
        ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: Default::default(),
            plan_id: plan_id.into(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: Some(phase.into()),
            explain,
        })),
    )
    .unwrap()
}

#[test]
#[allow(clippy::cognitive_complexity)]
fn iteration_preview_execution_reuse_and_final_completion_are_distinct() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(
        temp.path(),
        r#"
[[work.gates]]
id = "full"
kind = "evidence"
profile = "verify"
"#,
    );
    enable_v6_iteration_profile(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let receipts_path = temp.path().join(".agent/state/receipts.jsonl");
    let receipts_before = fs::read(&receipts_path).unwrap_or_default();
    let state_before = ["runs.jsonl", "plans.jsonl", "sessions.jsonl"].map(|name| {
        (
            name,
            fs::read(temp.path().join(".agent/state").join(name)).unwrap_or_default(),
        )
    });

    let preview = work_check(&ctx, "iteration", true);

    assert_eq!(preview["ok"], true, "{preview:#}");
    assert_eq!(preview["selected_ok"], false, "{preview:#}");
    assert_eq!(preview["phase"], "iteration");
    assert_eq!(preview["explain"], true);
    assert_eq!(preview["selected_invocations"].as_array().unwrap().len(), 1);
    assert_eq!(
        preview["selected_invocations"][0]["target"],
        json!({"component": "api", "action": "test"})
    );
    assert_eq!(
        preview["selected_invocations"][0]["disposition"],
        "selected"
    );
    assert_eq!(preview["final_gates_ok"], false);
    assert_eq!(preview["pending_final_requirements"][0]["id"], "full");
    assert!(!temp.path().join(".agent/launch.log").exists());
    assert_eq!(
        fs::read(&receipts_path).unwrap_or_default(),
        receipts_before
    );
    for (name, before) in state_before {
        assert_eq!(
            fs::read(temp.path().join(".agent/state").join(name)).unwrap_or_default(),
            before,
            "explain mutated {name}"
        );
    }

    let iteration = work_check(&ctx, "iteration", false);

    assert_eq!(iteration["ok"], true, "{iteration:#}");
    assert_eq!(
        iteration["selected_invocations"][0]["invocation"],
        preview["selected_invocations"][0]["invocation"]
    );
    assert_eq!(iteration["selected_ok"], true);
    assert_eq!(iteration["final_gates_ok"], false);
    assert_eq!(iteration["pending_final_requirements"][0]["id"], "full");
    assert_eq!(
        fs::read_to_string(temp.path().join(".agent/launch.log")).unwrap(),
        "api\n"
    );
    let first_receipt = iteration["selected_invocations"][0]["evidence_validity"]["receipt_id"]
        .as_str()
        .unwrap()
        .to_string();
    let first_run = iteration["selected_invocations"][0]["evidence_validity"]["run_id"]
        .as_str()
        .unwrap()
        .to_string();

    let repeated = work_check(&ctx, "iteration", false);
    assert_eq!(repeated["ok"], true, "{repeated:#}");
    assert_eq!(repeated["selected_invocations"][0]["disposition"], "reused");
    assert_eq!(
        repeated["selected_invocations"][0]["evidence_validity"]["receipt_id"],
        first_receipt
    );
    assert_eq!(
        repeated["selected_invocations"][0]["evidence_validity"]["run_id"],
        first_run
    );
    assert_eq!(
        read_receipts(temp.path())
            .iter()
            .filter(|receipt| receipt["target"].is_object())
            .count(),
        1,
        "reuse must not manufacture another target receipt"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join(".agent/launch.log")).unwrap(),
        "api\n"
    );

    let finish_error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Finish(
            crate::cli::WorkFinishOpts {
                plan_id: "plan_1".into(),
                resolution: Some("premature".into()),
                outcome: None,
            },
        )),
    )
    .unwrap_err()
    .to_string();
    assert!(
        finish_error.to_ascii_lowercase().contains("missing"),
        "{finish_error}"
    );

    let final_check = work_check(&ctx, "final", false);
    assert_eq!(final_check["ok"], true, "{final_check:#}");
    assert_eq!(final_check["final_gates_ok"], true, "{final_check:#}");
    assert!(
        final_check["pending_final_requirements"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fs::read_to_string(temp.path().join(".agent/launch.log")).unwrap(),
        "api\nweb\n"
    );

    let finished = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Finish(
            crate::cli::WorkFinishOpts {
                plan_id: "plan_1".into(),
                resolution: Some("verified".into()),
                outcome: None,
            },
        )),
    )
    .unwrap();
    assert_eq!(finished["ok"], true, "{finished:#}");
}

#[test]
fn iteration_failure_is_not_a_pass_and_newest_failure_hides_older_success() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    enable_v6_iteration_profile(temp.path());
    init_git_repo(temp.path());
    let config_path = temp.path().join(".jig.toml");
    let passing_config = fs::read_to_string(&config_path).unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let first = work_check(&ctx, "iteration", false);
    assert_eq!(first["ok"], true, "{first:#}");

    let failing_config = passing_config.replace(
        "api_test_command = \"printf 'api tests passed\\n'; printf 'api\\n' >> .agent/launch.log\"",
        "api_test_command = \"printf 'api tests failed\\n'; printf 'api\\n' >> .agent/launch.log; exit 7\"",
    );
    assert_ne!(failing_config, passing_config);
    fs::write(&config_path, failing_config).unwrap();
    let failing_ctx = RepoContext::load_from(temp.path()).unwrap();
    let failed = work_check(&failing_ctx, "iteration", false);
    assert_eq!(failed["ok"], false, "{failed:#}");
    assert_eq!(failed["selected_ok"], false);
    assert_eq!(
        failed["selected_invocations"][0]["evidence_validity"]["status"],
        "failed"
    );

    fs::write(&config_path, passing_config).unwrap();
    let restored_ctx = RepoContext::load_from(temp.path()).unwrap();
    let repaired = work_check(&restored_ctx, "iteration", false);
    assert_eq!(repaired["ok"], true, "{repaired:#}");
    assert_eq!(
        repaired["selected_invocations"][0]["disposition"], "selected",
        "the newer failed outcome must hide the older passing receipt"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join(".agent/launch.log")).unwrap(),
        "api\napi\napi\n"
    );
}

#[test]
fn cancelled_iteration_never_reports_a_pass() {
    struct CancelWhenMarkerExists(std::path::PathBuf);

    impl crate::execution::ExecutionObserver for CancelWhenMarkerExists {}

    impl crate::execution::ExecutionCancellation for CancelWhenMarkerExists {
        fn cancelled(&self) -> bool {
            self.0.exists()
        }
    }

    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    enable_v6_iteration_profile(temp.path());
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "api_test_command = \"printf 'api tests passed\\n'; printf 'api\\n' >> .agent/launch.log\"",
        "api_test_command = \"printf 'api\\n' >> .agent/launch.log; touch .agent/cancel-iteration; sleep 30\"",
    );
    fs::write(&config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = CancelWhenMarkerExists(temp.path().join(".agent/cancel-iteration"));

    let error = crate::runtime::dispatch_with_observer(
        &ctx,
        crate::command::RuntimeCommand::Work(crate::command::WorkCommand::Check(
            crate::command::WorkCheckRequest {
                rust_focus: Default::default(),
                projection: Default::default(),
                plan_id: "plan_1".into(),
                gates: Vec::new(),
                tools: Vec::new(),
                phase: Some(crate::command::WorkCheckPhase::Iteration),
                explain: false,
            },
        )),
        &mut observer,
    )
    .unwrap_err();
    let error = format!("{error:#}");

    assert!(error.to_ascii_lowercase().contains("cancel"), "{error}");
    assert_eq!(
        fs::read_to_string(temp.path().join(".agent/launch.log")).unwrap(),
        "api\n"
    );
    assert!(!read_receipts(temp.path()).iter().any(|receipt| {
        receipt["target"] == json!({"component": "api", "action": "test"})
            && receipt["exit_status"] == 0
    }));
}

#[test]
fn iteration_preflight_rejects_missing_unknown_and_empty_profiles() {
    for (label, configure, expected) in [
        ("missing", false, "work.iteration_profile is not configured"),
        ("unknown", true, "unknown repository profile 'missing'"),
    ] {
        let temp = tempdir().unwrap();
        write_v6_evidence_fixture_repo(temp.path(), "");
        if configure {
            let path = temp.path().join(".jig.toml");
            let config = fs::read_to_string(&path).unwrap().replace(
                "[commands]",
                "[work]\niteration_profile = \"missing\"\n\n[commands]",
            );
            fs::write(path, config).unwrap();
        }
        init_git_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let error = dispatch(
            &ctx,
            CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
                rust_focus: Default::default(),
                projection: Default::default(),
                plan_id: "plan_1".into(),
                gates: Vec::new(),
                tools: Vec::new(),
                phase: Some("iteration".into()),
                explain: label == "missing",
            })),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains(expected), "{label}: {error}");
        assert!(!temp.path().join(".agent/launch.log").exists());
    }

    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "[commands]",
        "[work]\niteration_profile = \"iteration\"\n\n[commands]",
    );
    fs::write(
        &config_path,
        format!("{config}\n[[repository.profiles]]\nid = \"iteration\"\ntargets = []\n"),
    )
    .unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["profiles"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "iteration", "targets": []}));
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: Default::default(),
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: Some("iteration".into()),
            explain: false,
        })),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("must select at least one target"), "{error}");
}

#[test]
fn iteration_preflight_rejects_legacy_contracts_effectful_actions_and_unbound_arguments() {
    let legacy = tempdir().unwrap();
    write_fixture_repo(legacy.path());
    let config_path = legacy.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "[commands]",
        "[work]\niteration_profile = \"iteration\"\n\n[commands]",
    );
    fs::write(&config_path, config).unwrap();
    let ctx = RepoContext::load_from(legacy.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: Default::default(),
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: Some("iteration".into()),
            explain: false,
        })),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("contract version 6 or later"), "{error}");

    let effectful = tempdir().unwrap();
    write_v6_evidence_fixture_repo(effectful.path(), "");
    enable_v6_iteration_profile(effectful.path());
    let config_path = effectful.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replacen(
            "inputs = [\"api/**\"]",
            "inputs = [\"api/**\"]\ndepends_on = [{ component = \"web\", action = \"test\" }]",
            1,
        )
        .replace(
            "target = { component = \"web\", action = \"test\" }\nintent = \"check\"\neffects = [\"read_only\", \"process\"]",
            "target = { component = \"web\", action = \"test\" }\nintent = \"generate\"\neffects = [\"worktree\", \"process\"]",
        );
    fs::write(&config_path, config).unwrap();
    let manifest_path = effectful.path().join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["actions"][0]["depends_on"] = json!([{"component": "web", "action": "test"}]);
    manifest["actions"][1]["intent"] = json!("generate");
    manifest["actions"][1]["effects"] = json!(["worktree", "process"]);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    init_git_repo(effectful.path());
    let ctx = RepoContext::load_from(effectful.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: Default::default(),
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: Some("iteration".into()),
            explain: true,
        })),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("not a read-only check"), "{error}");
    assert!(!effectful.path().join(".agent/launch.log").exists());

    let argument = tempdir().unwrap();
    write_v6_evidence_fixture_repo(argument.path(), "");
    enable_v6_iteration_profile(argument.path());
    let config_path = argument.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replacen(
            "runner = { kind = \"command\", command = \"api_test_command\" }",
            "runner = { kind = \"argv\", program = \"printf\", args = [\"%s\\\\n\", { argument = \"name\" }] }",
            1,
        )
        .replace("kind = \"command\"", "kind = \"shell\"")
        .replacen(
            "inputs = [\"api/**\"]",
            "arguments = { name = { type = \"string\", required = true, max_bytes = 200 } }\ninputs = [\"api/**\"]",
            1,
        );
    fs::write(&config_path, &config).unwrap();
    let config_value: toml::Value = toml::from_str(&config).unwrap();
    let manifest_path = argument.path().join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["contract_version"] = json!(8);
    manifest["actions"] = serde_json::to_value(&config_value["repository"]["actions"]).unwrap();
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    init_git_repo(argument.path());
    let ctx = RepoContext::load_from(argument.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: Default::default(),
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: Some("iteration".into()),
            explain: false,
        })),
    )
    .unwrap_err();
    let error = format!("{error:#}");
    assert!(error.contains("argument 'name'"), "{error}");
    assert!(!argument.path().join(".agent/launch.log").exists());
}

#[test]
fn iteration_explain_rejects_unknown_and_closed_plans_without_receipts() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    enable_v6_iteration_profile(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let receipts_path = temp.path().join(".agent/state/receipts.jsonl");
    let before = fs::read(&receipts_path).unwrap_or_default();

    let unknown = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: Default::default(),
            plan_id: "plan_missing".into(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: Some("iteration".into()),
            explain: true,
        })),
    )
    .unwrap_err()
    .to_string();
    assert!(unknown.contains("Plan not found"), "{unknown}");
    assert_eq!(fs::read(&receipts_path).unwrap_or_default(), before);

    crate::state::plans_close(
        &ctx,
        crate::state::PlanCloseRequest {
            plan_id: "plan_1".into(),
            resolution: Some("closed for test".into()),
        },
    )
    .unwrap();
    let before_closed_explain = fs::read(&receipts_path).unwrap_or_default();
    let closed = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: Default::default(),
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: Some("iteration".into()),
            explain: true,
        })),
    )
    .unwrap_err()
    .to_string();
    assert!(closed.contains("already closed"), "{closed}");
    assert_eq!(
        fs::read(&receipts_path).unwrap_or_default(),
        before_closed_explain
    );
}

#[test]
fn iteration_selection_is_independent_of_legacy_only_final_gates() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(
        temp.path(),
        r#"
[[work.gates]]
id = "web-check"
kind = "check"
tool = "jig.web_test"
"#,
    );
    enable_v6_iteration_profile(temp.path());
    enable_v6_legacy_web_tool(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let preview = work_check(&ctx, "final", true);
    assert_eq!(preview["ok"], true, "{preview:#}");
    assert_eq!(preview["selected_ok"], false, "{preview:#}");
    assert_eq!(preview["selected_checks"][0]["disposition"], "selected");
    assert!(!temp.path().join(".agent/launch.log").exists());

    let iteration = work_check(&ctx, "iteration", false);
    assert_eq!(iteration["ok"], true, "{iteration:#}");
    assert_eq!(iteration["final_gates_ok"], false);
    assert_eq!(iteration["pending_final_requirements"][0]["kind"], "check");
    assert_eq!(
        fs::read_to_string(temp.path().join(".agent/launch.log")).unwrap(),
        "api\n"
    );

    let final_check = work_check(&ctx, "final", false);
    assert_eq!(final_check["ok"], true, "{final_check:#}");
    assert_eq!(final_check["final_gates_ok"], true, "{final_check:#}");
    assert!(
        final_check["selected_invocations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(final_check["selected_checks"][0]["tool"], "jig.web_test");
    assert_eq!(
        fs::read_to_string(temp.path().join(".agent/launch.log")).unwrap(),
        "api\nweb\n"
    );
}

#[test]
fn phase_report_keeps_required_review_optional_and_not_applicable_gates_distinct() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(
        temp.path(),
        r#"
[[work.gates]]
id = "inactive-web"
kind = "check"
tool = "jig.web_test"
paths = ["never/**"]

[[work.gates]]
id = "required-review"
kind = "codex_review"
skill = "example-review"

[[work.gates]]
id = "optional-web"
kind = "evidence"
target = "web:test"
required = false
"#,
    );
    enable_v6_iteration_profile(temp.path());
    enable_v6_legacy_web_tool(temp.path());
    init_git_repo(temp.path());
    let initial = RepoContext::load_from(temp.path()).unwrap();
    crate::state::plans_close(
        &initial,
        crate::state::PlanCloseRequest {
            plan_id: "plan_1".into(),
            resolution: Some("replace fixture plan after Git initialization".into()),
        },
    )
    .unwrap();
    let opened = crate::state::plans_open(
        &initial,
        crate::state::PlanOpenRequest {
            title: "Phase accounting".into(),
            body: Some("Inspect final requirements".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let plan_id = opened["plan_id"].as_str().unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let final_check = work_check_for_plan(&ctx, plan_id, "final", false);
    assert_eq!(final_check["selected_ok"], true, "{final_check:#}");
    assert_eq!(final_check["final_gates_ok"], false, "{final_check:#}");
    assert!(!temp.path().join(".agent/launch.log").exists());

    let preview = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: Default::default(),
            plan_id: plan_id.into(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: Some("iteration".into()),
            explain: true,
        })),
    )
    .unwrap();

    let gate = |id: &str| {
        preview["final_requirements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|gate| gate["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(gate("inactive-web")["status"], "not_applicable");
    assert_eq!(gate("required-review")["status"], "missing");
    assert_eq!(gate("optional-web")["status"], "missing");
    let pending = preview["pending_final_requirements"].as_array().unwrap();
    assert_eq!(pending.len(), 1, "{preview:#}");
    assert_eq!(pending[0]["id"], "required-review");
    assert_eq!(preview["final_gates_ok"], false);
    assert!(!temp.path().join(".agent/launch.log").exists());
}

#[test]
fn iteration_rejects_profile_authority_changed_while_waiting_for_a_lease() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    enable_v6_iteration_profile(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let held = crate::state::acquire_repository_execution_lease(
        &ctx,
        &[jig_contract::ActionEffect::Worktree],
    )
    .unwrap();
    let (wait_tx, wait_rx) = std::sync::mpsc::sync_channel(0);
    let root = temp.path().to_path_buf();
    let mut observer = IterationWaitObserver {
        wait_notice: Some(wait_tx),
    };

    let changer = std::thread::spawn(move || {
        wait_rx.recv().unwrap();
        let config_path = root.join(".jig.toml");
        let config = fs::read_to_string(&config_path).unwrap().replace(
            "id = \"iteration\"\ntargets = [{ component = \"api\", action = \"test\" }]",
            "id = \"iteration\"\ntargets = [{ component = \"web\", action = \"test\" }]",
        );
        fs::write(&config_path, config).unwrap();
        let manifest_path = root.join(".agent/jig-contract.json");
        let mut manifest: Value =
            serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
        let profile = manifest["profiles"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|profile| profile["id"] == "iteration")
            .unwrap();
        profile["targets"] = json!([{"component": "web", "action": "test"}]);
        fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        drop(held);
    });

    let error = crate::runtime::dispatch_with_observer(
        &ctx,
        crate::command::RuntimeCommand::Work(crate::command::WorkCommand::Check(
            crate::command::WorkCheckRequest {
                rust_focus: Default::default(),
                projection: Default::default(),
                plan_id: "plan_1".into(),
                gates: Vec::new(),
                tools: Vec::new(),
                phase: Some(crate::command::WorkCheckPhase::Iteration),
                explain: false,
            },
        )),
        &mut observer,
    )
    .unwrap_err()
    .to_string();
    changer.join().unwrap();

    assert!(error.contains("execution authority changed"), "{error}");
    assert!(!temp.path().join(".agent/launch.log").exists());
}
