// agentic-loc-exception: repository execution evidence and cancellation cases share one durable-run fixture boundary.

use super::*;

#[derive(Default)]
struct LeaseWaitObserver {
    output: Vec<u8>,
    cancelled: bool,
    cancel_on_wait: bool,
    flushes: usize,
    wait_notice: Option<std::sync::mpsc::SyncSender<()>>,
}

impl crate::execution::ExecutionObserver for LeaseWaitObserver {
    fn event(&mut self, event: crate::execution::ExecutionEvent<'_>) {
        if let crate::execution::ExecutionEvent::Output { bytes, .. } = event {
            self.output.extend_from_slice(bytes);
            if self.cancel_on_wait {
                self.cancelled = true;
            }
        }
    }

    fn flush(&mut self) -> Result<()> {
        self.flushes += 1;
        if let Some(wait_notice) = self.wait_notice.take() {
            wait_notice.send(()).unwrap();
        }
        Ok(())
    }
}

impl crate::execution::ExecutionCancellation for LeaseWaitObserver {
    fn cancelled(&self) -> bool {
        self.cancelled
    }
}

#[derive(Default)]
struct PhaseRecordingObserver {
    started: Vec<String>,
    finished: Vec<(String, bool)>,
}

impl crate::execution::ExecutionObserver for PhaseRecordingObserver {
    fn event(&mut self, event: crate::execution::ExecutionEvent<'_>) {
        match event {
            crate::execution::ExecutionEvent::PhaseStarted { label, .. } => {
                self.started.push(label.to_owned());
            }
            crate::execution::ExecutionEvent::PhaseFinished { label, success, .. } => {
                self.finished.push((label.to_owned(), success));
            }
            crate::execution::ExecutionEvent::Output { .. }
            | crate::execution::ExecutionEvent::Heartbeat { .. } => {}
        }
    }
}

impl crate::execution::ExecutionCancellation for PhaseRecordingObserver {}

struct MarkerCancellationObserver {
    marker: std::path::PathBuf,
}

impl crate::execution::ExecutionObserver for MarkerCancellationObserver {}

impl crate::execution::ExecutionCancellation for MarkerCancellationObserver {
    fn cancelled(&self) -> bool {
        self.marker.exists()
    }
}

#[test]
fn empty_freshly_planned_check_rejects_source_drift_before_creating_a_run() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "[repository]\ndefault_check_profile = \"verify\"",
        "[repository]\ndefault_check_profile = \"verify\"\naffected_ignore = [\"README.md\"]",
    );
    fs::write(&config_path, config).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["affected_ignore"] = json!(["README.md"]);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    init_git_repo(temp.path());
    fs::write(temp.path().join("README.md"), "documentation only\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = crate::repository::plan_run(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            affected_base: Some("HEAD".into()),
            ..crate::repository::PlanRunRequest::default()
        },
    )
    .unwrap();
    assert!(plan.targets.is_empty());
    fs::write(temp.path().join("api/example.go"), "package changed\n").unwrap();

    let mut observer = crate::execution::NoopExecutionObserver;
    let error = super::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap_err();

    assert!(error.to_string().contains("source changed after planning"));
    assert!(!temp.path().join(".agent/state/runs.jsonl").exists());
}

#[test]
fn freshly_planned_check_rejects_authority_that_changed_before_planning() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let config_path = temp.path().join(".jig.toml");
    let changed = fs::read_to_string(&config_path).unwrap().replace(
        "api_test_command = \"printf 'api tests passed\\n'\"",
        "api_test_command = \"printf 'changed command\\n'\"",
    );
    fs::write(&config_path, changed).unwrap();
    let plan = crate::repository::plan_run(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..crate::repository::PlanRunRequest::default()
        },
    )
    .unwrap();

    let mut observer = crate::execution::NoopExecutionObserver;
    let error = super::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap_err();

    assert!(error.to_string().contains("execution authority changed"));
    assert!(!temp.path().join(".agent/state/runs.jsonl").exists());
}

#[test]
fn freshly_planned_check_reports_repository_lease_waiting() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = crate::repository::plan_run(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..crate::repository::PlanRunRequest::default()
        },
    )
    .unwrap();
    let held = crate::state::acquire_repository_execution_lease(
        &ctx,
        &[jig_contract::ActionEffect::Worktree],
    )
    .unwrap();
    let (wait_notice_tx, wait_notice_rx) = std::sync::mpsc::sync_channel(0);
    let release = std::thread::spawn(move || {
        wait_notice_rx.recv().unwrap();
        drop(held);
    });
    let mut observer = LeaseWaitObserver {
        wait_notice: Some(wait_notice_tx),
        ..LeaseWaitObserver::default()
    };

    let execution = super::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap();
    release.join().unwrap();

    assert_eq!(
        execution.run.result.conclusion,
        Some(jig_contract::RunConclusion::Success)
    );
    assert!(
        String::from_utf8(observer.output)
            .unwrap()
            .contains("Waiting for another repository execution"),
        "foreground execution must explain repository lease contention"
    );
    assert_eq!(
        observer.flushes, 1,
        "the wait notice must be delivered promptly"
    );
}

#[test]
fn freshly_planned_check_can_cancel_while_waiting_for_repository_lease() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = crate::repository::plan_run(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..crate::repository::PlanRunRequest::default()
        },
    )
    .unwrap();
    let held = crate::state::acquire_repository_execution_lease(
        &ctx,
        &[jig_contract::ActionEffect::Worktree],
    )
    .unwrap();
    let mut observer = LeaseWaitObserver {
        cancel_on_wait: true,
        ..LeaseWaitObserver::default()
    };

    let result = super::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &mut observer,
    );
    drop(held);

    assert_eq!(observer.flushes, 1);
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("cancelled while waiting for another repository execution")
    );
}

#[test]
fn accepted_empty_check_cannot_complete_under_changed_manifest_authority() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "[repository]\ndefault_check_profile = \"verify\"",
        "[repository]\ndefault_check_profile = \"verify\"\naffected_ignore = [\"README.md\"]",
    );
    fs::write(&config_path, config).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["affected_ignore"] = json!(["README.md"]);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    init_git_repo(temp.path());
    fs::write(temp.path().join("README.md"), "documentation only\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = crate::repository::plan_run(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            affected_base: Some("HEAD".into()),
            ..crate::repository::PlanRunRequest::default()
        },
    )
    .unwrap();
    assert!(plan.targets.is_empty());
    let (run, _lease) = super::run_execution::start_check_run(&ctx, &catalog, plan, None).unwrap();
    manifest["jig_version"] = json!("changed-after-acceptance");
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let run_id = run.result.run_id.clone();
    let error = super::run_execution::execute_started_check_run(
        &ctx,
        &catalog,
        run,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &|| Ok(false),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("execution authority changed"), "{error}");
    assert_eq!(
        crate::state::run_by_id(&ctx, &run_id)
            .unwrap()
            .result
            .conclusion,
        Some(jig_contract::RunConclusion::Blocked)
    );
}

#[test]
fn target_that_changes_manifest_authority_cannot_report_success() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let changed = fs::read_to_string(&config_path).unwrap().replace(
        "api_test_command = \"printf 'api tests passed\\n'\"",
        "api_test_command = \"printf 'not-json\\n' > .agent/jig-contract.json\"",
    );
    fs::write(&config_path, changed).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = crate::repository::plan_run(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..crate::repository::PlanRunRequest::default()
        },
    )
    .unwrap();

    let mut observer = PhaseRecordingObserver::default();
    let execution = super::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap();

    let target = &execution.run.result.targets[0];
    assert_eq!(
        target.conclusion,
        Some(jig_contract::RunConclusion::Blocked)
    );
    assert!(
        target
            .findings
            .iter()
            .any(|finding| finding.source.as_deref() == Some("execution_authority"))
    );
    assert_eq!(observer.finished.len(), 1);
    assert!(!observer.finished[0].1);
}

#[test]
fn repository_command_target_fails_on_the_configured_output_limit() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "api_test_command = \"printf 'api tests passed\\n'\"",
        "api_test_command = \"printf 'output larger than the configured bound'\"",
    ) + "\n[execution]\ncommand_output_limit_bytes = 16\n";
    fs::write(config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = crate::repository::plan_run(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..crate::repository::PlanRunRequest::default()
        },
    )
    .unwrap();

    let execution = super::run_execution::execute_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: true,
            fail_fast: false,
        },
        &|| false,
    )
    .unwrap();

    assert_eq!(
        execution.run.result.conclusion,
        Some(jig_contract::RunConclusion::Failure)
    );
    let target = &execution.run.result.targets[0];
    assert_eq!(
        target.conclusion,
        Some(jig_contract::RunConclusion::Failure)
    );
    assert_eq!(
        target.findings[0].source.as_deref(),
        Some("execution_policy")
    );
    assert!(target.findings[0].message.contains("16 byte stdout"));
    assert_eq!(
        execution.results[0]["response"]["result"]["stdout"],
        "output larger th"
    );
}

#[test]
fn repository_command_target_uses_the_configured_default_timeout() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "api_test_command = \"printf 'api tests passed\\n'\"",
        "api_test_command = \"sleep 30\"",
    ) + "\n[execution]\ncommand_timeout_seconds = 1\n";
    fs::write(config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = crate::repository::plan_run(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..crate::repository::PlanRunRequest::default()
        },
    )
    .unwrap();

    let execution = super::run_execution::execute_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &|| false,
    )
    .unwrap();

    assert_eq!(
        execution.run.result.conclusion,
        Some(jig_contract::RunConclusion::TimedOut)
    );
    assert_eq!(
        execution.run.result.targets[0].conclusion,
        Some(jig_contract::RunConclusion::TimedOut)
    );
}

#[test]
fn repository_affected_check_rejects_legacy_contracts_before_git_resolution() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = super::super::dispatch(
        &ctx,
        RuntimeCommand::Check(crate::command::CheckCommand::Repository(
            crate::command::RepositoryCheckRequest {
                selectors: Vec::new(),
                profile: None,
                affected_base: Some("missing-ref".into()),
                explain: true,
                fail_fast: false,
                tool: crate::command::ToolRequest::new(None, true),
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("contract version 6 or later"));
}

#[test]
fn independent_read_only_layer_targets_execute_concurrently() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace(
            "api_test_command = \"printf 'api tests passed\\n'\"",
            "api_test_command = \"touch .agent/.cache/api-started; for attempt in $(seq 1 200); do [ -f .agent/.cache/web-started ] && { touch .agent/.cache/api-finished; exit 0; }; sleep 0.01; done; exit 9\"",
        )
        .replace(
            "web_test_command = \"printf 'web tests passed\\n'\"",
            "web_test_command = \"touch .agent/.cache/web-started; for attempt in $(seq 1 200); do [ -f .agent/.cache/api-finished ] && { sleep 1; exit 0; }; sleep 0.01; done; exit 9\"",
        );
    fs::write(config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan =
        crate::repository::plan_run(&ctx, &catalog, crate::repository::PlanRunRequest::default())
            .unwrap();
    assert_eq!(plan.execution_layers.len(), 1);
    assert_eq!(plan.execution_layers[0].len(), 2);
    let mut observer = PhaseRecordingObserver::default();

    let execution = super::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap();

    assert_eq!(
        execution.run.result.conclusion,
        Some(jig_contract::RunConclusion::Success)
    );
    assert_eq!(
        execution
            .run
            .result
            .targets
            .iter()
            .map(|target| target.target.to_string())
            .collect::<Vec<_>>(),
        ["api:test", "web:test"]
    );
    assert_eq!(observer.started.len(), 2);
    assert_eq!(observer.finished.len(), 2);
    let api = &execution.run.result.targets[0];
    let web = &execution.run.result.targets[1];
    assert!(
        api.ended_at_ms.unwrap().saturating_add(500) <= web.ended_at_ms.unwrap(),
        "each parallel target must retain its own completion time: api={api:?}, web={web:?}"
    );
}

#[test]
fn wide_parallel_layer_keeps_the_bounded_worker_pool_busy() {
    let temp = tempdir().unwrap();
    let mut commands = vec!["sleep 0.05".to_owned(); 9];
    commands[0] = "for attempt in $(seq 1 300); do [ -f .agent/.cache/ninth-started ] && exit 0; sleep 0.01; done; exit 9".into();
    commands[8] = "touch .agent/.cache/ninth-started".into();
    write_wide_v6_evidence_fixture_repo(temp.path(), &commands);
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = super::super::dispatch(
        &ctx,
        RuntimeCommand::Check(crate::command::CheckCommand::Repository(
            crate::command::RepositoryCheckRequest {
                selectors: Vec::new(),
                profile: None,
                affected_base: None,
                explain: false,
                fail_fast: false,
                tool: crate::command::ToolRequest::new(None, true),
            },
        )),
    )
    .unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(
        output["source_observations"]["count"], 3,
        "the queued target requires a fresh source precondition"
    );
}

#[test]
fn queued_parallel_target_revalidates_source_before_starting() {
    let temp = tempdir().unwrap();
    let mut commands = (0..9)
        .map(|_| {
            "for attempt in $(seq 1 200); do [ -f .agent/.cache/source-mutated ] && { sleep 0.5; exit 0; }; sleep 0.01; done; exit 9"
                .to_owned()
        })
        .collect::<Vec<_>>();
    commands[0] =
        "printf 'mutated\n' >> example0/example.txt; touch .agent/.cache/source-mutated".into();
    commands[8] = "touch .agent/.cache/queued-target-ran".into();
    write_wide_v6_evidence_fixture_repo(temp.path(), &commands);
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan =
        crate::repository::plan_run(&ctx, &catalog, crate::repository::PlanRunRequest::default())
            .unwrap();
    let mut observer = PhaseRecordingObserver::default();

    let execution = super::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap();

    let queued = &execution.run.result.targets[8];
    assert_eq!(queued.started_at_ms, None, "{queued:?}");
    assert!(
        queued.findings.iter().any(|finding| finding
            .message
            .contains("worktree changed after plan validation")),
        "queued work must preserve its failed source precondition: {queued:?}"
    );
    assert!(
        !temp.path().join(".agent/.cache/queued-target-ran").exists(),
        "a target claimed after stable source drift must remain unstarted"
    );
}

#[test]
fn parallel_read_only_layer_fails_closed_and_reports_failure_on_a_source_mutation() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace(
            "api_test_command = \"printf 'api tests passed\\n'\"",
            "api_test_command = \"touch .agent/.cache/api-started; for attempt in $(seq 1 200); do [ -f .agent/.cache/web-started ] && { printf 'mutated\\n' >> api/example.go; exit 0; }; sleep 0.01; done; exit 9\"",
        )
        .replace(
            "web_test_command = \"printf 'web tests passed\\n'\"",
            "web_test_command = \"touch .agent/.cache/web-started; for attempt in $(seq 1 200); do [ -f .agent/.cache/api-started ] && { sleep 0.1; exit 0; }; sleep 0.01; done; exit 9\"",
        );
    fs::write(config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan =
        crate::repository::plan_run(&ctx, &catalog, crate::repository::PlanRunRequest::default())
            .unwrap();
    let mut observer = PhaseRecordingObserver::default();

    let execution = super::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap();

    assert_eq!(
        execution.run.result.conclusion,
        Some(jig_contract::RunConclusion::Failure)
    );
    assert_eq!(
        serde_json::to_value(execution.source_observations).unwrap()["count"],
        2
    );
    assert!(
        execution
            .run
            .result
            .targets
            .iter()
            .all(|target| target.conclusion == Some(jig_contract::RunConclusion::Failure))
    );
    assert!(
        observer.finished.iter().all(|(_, success)| !success),
        "phase completion must reflect the postcondition-adjusted target result: {:?}",
        observer.finished
    );
    for target in &execution.run.result.targets {
        let effect_policy = target
            .findings
            .iter()
            .find(|finding| finding.source.as_deref() == Some("effect_policy"))
            .expect("each started parallel target must record the shared layer violation");
        assert!(
            effect_policy.message.contains("parallel read-only layer"),
            "shared observations must describe the layer rather than blame one target: {effect_policy:?}"
        );
        assert!(
            !effect_policy.message.contains("while target"),
            "shared observations cannot identify which concurrent target changed the source: {effect_policy:?}"
        );
    }
}

#[test]
fn cancelled_parallel_target_keeps_not_started_evidence_after_a_sibling_mutation() {
    let temp = tempdir().unwrap();
    let mut commands = vec!["sleep 2".to_owned(); 9];
    commands[0] =
        "printf 'mutated\\n' >> example0/example.txt; touch .agent/.cache/cancel; sleep 2".into();
    commands[8] = "exit 9".into();
    write_wide_v6_evidence_fixture_repo(temp.path(), &commands);
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan =
        crate::repository::plan_run(&ctx, &catalog, crate::repository::PlanRunRequest::default())
            .unwrap();
    let ninth_planned_digest = plan.targets[8].input_digest.clone();
    let mut observer = MarkerCancellationObserver {
        marker: temp.path().join(".agent/.cache/cancel"),
    };

    let execution = super::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap();

    let ninth = &execution.run.result.targets[8];
    assert_eq!(ninth.started_at_ms, None, "{ninth:?}");
    assert_eq!(ninth.input_digest, ninth_planned_digest);
    assert!(
        ninth
            .findings
            .iter()
            .all(|finding| finding.source.as_deref() != Some("effect_policy")),
        "a target that never started must not be blamed for a sibling mutation: {ninth:?}"
    );
}

#[test]
fn parallel_target_that_fails_authority_before_start_keeps_specific_receipt_evidence() {
    let temp = tempdir().unwrap();
    let mut commands = (0..8)
        .map(|index| {
            format!(
                "touch .agent/.cache/parallel-started-{index}; for attempt in $(seq 1 200); do [ \"$(find .agent/.cache -name 'parallel-started-*' | wc -l)\" -eq 8 ] && {{ sleep 0.5; exit 0; }}; sleep 0.01; done; exit 9"
            )
        })
        .chain(std::iter::once("exit 0".to_owned()))
        .collect::<Vec<_>>();
    commands[0] = "touch .agent/.cache/parallel-started-0; for attempt in $(seq 1 200); do [ \"$(find .agent/.cache -name 'parallel-started-*' | wc -l)\" -eq 8 ] && { printf 'invalid contract\n' > .agent/jig-contract.json; exit 0; }; sleep 0.01; done; exit 9".into();
    write_wide_v6_evidence_fixture_repo(temp.path(), &commands);
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan =
        crate::repository::plan_run(&ctx, &catalog, crate::repository::PlanRunRequest::default())
            .unwrap();
    let mut observer = PhaseRecordingObserver::default();

    let execution = super::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: true,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap();

    let ninth = &execution.run.result.targets[8];
    assert_eq!(ninth.started_at_ms, None, "{ninth:?}");
    let receipt_id = ninth.receipt_id.as_deref().unwrap();
    let receipt = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|receipt| receipt["id"] == receipt_id)
        .unwrap();
    assert!(
        receipt["worktree_fingerprint_error"]
            .as_str()
            .unwrap()
            .contains("authority could not be verified"),
        "a pre-start authority failure must remain specific in durable evidence: {receipt:#}"
    );
}

#[test]
fn parallel_layer_uses_the_baseline_adopted_by_a_mutating_predecessor() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    add_v6_effectful_evidence_actions(temp.path());
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace(
            "[commands]",
            "[commands]\napi_generate_command = \"printf 'generated\\n' > api/generated.go\"",
        )
        .replace(
            "target = { component = \"api\", action = \"generate\" }\nintent = \"generate\"\neffects = [\"worktree\", \"process\"]\nrunner = { kind = \"command\", command = \"api_test_command\" }",
            "target = { component = \"api\", action = \"generate\" }\nintent = \"generate\"\neffects = [\"worktree\", \"process\"]\nrunner = { kind = \"command\", command = \"api_generate_command\" }",
        )
        .replace(
            "[[repository.profiles]]",
            r#"[[repository.actions]]
target = { component = "web", action = "verify-generated" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "web_test_command" }
inputs = ["web/**"]
depends_on = [{ component = "api", action = "generate" }]

[[repository.profiles]]"#,
        );
    fs::write(&config_path, config).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["required_commands"]
        .as_array_mut()
        .unwrap()
        .push(json!("api_generate_command"));
    let actions = manifest["actions"].as_array_mut().unwrap();
    actions
        .iter_mut()
        .find(|action| action["target"]["action"] == "generate")
        .unwrap()["runner"]["command"] = json!("api_generate_command");
    actions.push(json!({
        "target": {"component": "web", "action": "verify-generated"},
        "intent": "check",
        "effects": ["read_only", "process"],
        "runner": {"kind": "command", "command": "web_test_command"},
        "inputs": ["web/**"],
        "depends_on": [{"component": "api", "action": "generate"}]
    }));
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = crate::repository::plan_action_run(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            selectors: vec!["api:verify-generated".into(), "web:verify-generated".into()],
            profile: None,
            affected_base: None,
        },
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        plan.execution_layers
            .iter()
            .map(Vec::len)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    let mut observer = PhaseRecordingObserver::default();

    let execution = super::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap();

    assert_eq!(
        execution.run.result.conclusion,
        Some(jig_contract::RunConclusion::Success),
        "{:?}",
        execution.run.result.targets
    );
    assert_eq!(
        serde_json::to_value(execution.source_observations).unwrap()["count"],
        4
    );
    assert!(temp.path().join("api/generated.go").exists());
}

#[test]
fn repository_execution_records_cancelled_results_for_every_target() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .contract_version(5)
        .config(
            r#"
[commands]
first_command = "printf 'first\n'"
second_command = "printf 'second\n'"

[work]
checks = ["jig.first", "jig.second"]
"#,
        )
        .required_commands(["first_command", "second_command"])
        .tool(json!({
            "name": "jig.first",
            "kind": "command",
            "description": "Run first.",
            "command": "first_command"
        }))
        .tool(json!({
            "name": "jig.second",
            "kind": "command",
            "description": "Run second.",
            "command": "second_command"
        }))
        .write();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan =
        crate::repository::plan_run(&ctx, &catalog, crate::repository::PlanRunRequest::default())
            .unwrap();

    let execution = super::run_execution::execute_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: true,
            fail_fast: false,
        },
        &|| true,
    )
    .unwrap();

    assert_eq!(
        execution.run.result.conclusion,
        Some(jig_contract::RunConclusion::Cancelled)
    );
    assert_eq!(execution.run.result.targets.len(), 2);
    assert!(execution.run.result.targets.iter().all(|target| {
        target.status == jig_contract::RunStatus::Completed
            && target.conclusion == Some(jig_contract::RunConclusion::Cancelled)
            && target.receipt_id.is_some()
    }));
}

#[test]
fn repository_check_collects_failures_unless_fail_fast_is_explicit() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .contract_version(5)
        .config(
            r#"
[commands]
failing_command = "printf 'failed\n' >&2; exit 7"
later_command = "printf 'later\n' > later-ran.txt"

[work]
checks = ["jig.a_fail", "jig.z_later"]
"#,
        )
        .required_commands(["failing_command", "later_command"])
        .tool(json!({
            "name": "jig.a_fail",
            "kind": "command",
            "description": "Fail first.",
            "command": "failing_command"
        }))
        .tool(json!({
            "name": "jig.z_later",
            "kind": "command",
            "description": "Run later.",
            "command": "later_command"
        }))
        .write();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let request = |fail_fast| {
        RuntimeCommand::Check(crate::command::CheckCommand::Repository(
            crate::command::RepositoryCheckRequest {
                selectors: Vec::new(),
                profile: None,
                affected_base: None,
                explain: false,
                fail_fast,
                tool: crate::command::ToolRequest::new(None, true),
            },
        ))
    };

    let collected = super::super::dispatch(&ctx, request(false)).unwrap();
    assert_eq!(collected["ok"], false);
    assert_eq!(collected["results"].as_array().unwrap().len(), 2);
    assert!(temp.path().join("later-ran.txt").exists());

    fs::remove_file(temp.path().join("later-ran.txt")).unwrap();
    let stopped = super::super::dispatch(&ctx, request(true)).unwrap();
    assert_eq!(stopped["ok"], false);
    assert_eq!(stopped["results"].as_array().unwrap().len(), 1);
    assert!(!temp.path().join("later-ran.txt").exists());
    let stopped_run_id = stopped["run"]["run_id"].as_str().unwrap();
    let skipped_target = &stopped["run"]["targets"][1]["target"];
    let skipped_receipt = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|receipt| receipt["run_id"] == stopped_run_id && receipt["target"] == *skipped_target)
        .unwrap();
    assert!(skipped_receipt["worktree_fingerprint"].is_null());
    assert!(
        skipped_receipt["worktree_fingerprint_error"]
            .as_str()
            .unwrap()
            .contains("did not start")
    );
}

#[test]
fn command_tool_streams_both_outputs_through_execution_observer() {
    #[derive(Default)]
    struct RecordingObserver {
        output: Vec<u8>,
        started: bool,
        finished: bool,
    }

    impl crate::execution::ExecutionObserver for RecordingObserver {
        fn event(&mut self, event: crate::execution::ExecutionEvent<'_>) {
            match event {
                crate::execution::ExecutionEvent::PhaseStarted { .. } => self.started = true,
                crate::execution::ExecutionEvent::Output { bytes, .. } => {
                    self.output.extend_from_slice(bytes)
                }
                crate::execution::ExecutionEvent::PhaseFinished { .. } => self.finished = true,
                crate::execution::ExecutionEvent::Heartbeat { .. } => {}
            }
        }
    }

    impl crate::execution::ExecutionCancellation for RecordingObserver {}

    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
rust_test_command = "printf 'live stdout'; printf 'live stderr' >&2"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": "jig.test",
            "kind": "command",
            "description": "Run configured test command.",
            "command": "rust_test_command"
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = RecordingObserver::default();

    let output = dispatch_with_observer(
        &ctx,
        RuntimeCommand::Check(crate::command::CheckCommand::Test(
            crate::command::ToolRequest::new(None, false),
        )),
        &mut observer,
    )
    .unwrap();

    assert_eq!(output["result"]["stdout"], "live stdout");
    assert_eq!(output["result"]["stderr"], "live stderr");
    let observed = String::from_utf8(observer.output).unwrap();
    assert!(observed.contains("live stdout"));
    assert!(observed.contains("live stderr"));
    assert!(observer.started);
    assert!(observer.finished);
}

#[test]
fn plain_v6_named_test_routes_through_repository_planning_for_every_component() {
    #[derive(Default)]
    struct RecordingObserver {
        output: Vec<u8>,
        started: bool,
        finished: bool,
    }

    impl crate::execution::ExecutionObserver for RecordingObserver {
        fn event(&mut self, event: crate::execution::ExecutionEvent<'_>) {
            match event {
                crate::execution::ExecutionEvent::PhaseStarted { .. } => self.started = true,
                crate::execution::ExecutionEvent::Output { bytes, .. } => {
                    self.output.extend_from_slice(bytes);
                }
                crate::execution::ExecutionEvent::PhaseFinished { .. } => self.finished = true,
                crate::execution::ExecutionEvent::Heartbeat { .. } => {}
            }
        }
    }

    impl crate::execution::ExecutionCancellation for RecordingObserver {}

    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "api_test_command = \"printf 'api tests passed\\n'\"",
        "api_test_command = \"printf 'live v6 stdout'; printf 'live v6 stderr' >&2\"",
    );
    fs::write(config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = RecordingObserver::default();

    let output = dispatch_with_observer(
        &ctx,
        RuntimeCommand::Check(crate::command::CheckCommand::Test(
            crate::command::ToolRequest::new(None, false),
        )),
        &mut observer,
    )
    .unwrap();

    assert_eq!(
        output["results"][0]["response"]["result"]["stdout"],
        "live v6 stdout"
    );
    assert_eq!(
        output["results"][0]["response"]["result"]["stderr"],
        "live v6 stderr"
    );
    let observed = String::from_utf8(observer.output).unwrap();
    assert!(observed.contains("live v6 stdout"));
    assert!(observed.contains("live v6 stderr"));
    assert!(observer.started);
    assert!(observer.finished);
    assert_eq!(output["run"]["targets"].as_array().unwrap().len(), 2);
    assert_eq!(
        output["run"]["targets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|target| target["target"].clone())
            .collect::<Vec<_>>(),
        [
            json!({"component": "api", "action": "test"}),
            json!({"component": "web", "action": "test"}),
        ]
    );
    assert_eq!(
        output["source_observations"]["count"], 2,
        "one parallel layer requires one shared before/after source observation"
    );
}

#[cfg(unix)]
#[test]
fn configured_command_output_limit_can_exceed_the_internal_protocol_bound() {
    const OUTPUT_BYTES: usize = 4 * 1024 * 1024 + 1;

    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(format!(
            r#"
rust_test_command = "head -c {OUTPUT_BYTES} /dev/zero"

[execution]
command_output_limit_bytes = {OUTPUT_BYTES}
"#,
        ))
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": "jig.test",
            "kind": "command",
            "description": "Run configured test command.",
            "command": "rust_test_command"
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Check(crate::command::CheckCommand::Test(
            crate::command::ToolRequest::new(None, false),
        )),
    )
    .unwrap();

    assert_eq!(
        output["result"]["stdout"].as_str().unwrap().len(),
        OUTPUT_BYTES
    );
}

#[test]
fn command_tool_honors_repository_timeout() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
rust_test_command = "sleep 30"

[execution]
command_timeout_seconds = 1
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": "jig.test",
            "kind": "command",
            "description": "Run configured test command.",
            "command": "rust_test_command"
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let started = Instant::now();

    let error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Test(crate::cli::CheckTargetOpts {
                tool: crate::cli::ToolOpts {
                    plan_id: None,
                    no_receipt: true,
                },
                selectors: Vec::new(),
            }),
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("timed out"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(5));
}
#[test]
fn native_tool_no_receipt_skips_receipt_append() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(temp.path().join(".mcp.json"), "{}").unwrap();
    fs::write(temp.path().join("scripts/jig"), "#!/bin/sh\n").unwrap();
    fs::write(temp.path().join("scripts/install-jig.sh"), "#!/bin/sh\n").unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
bootstrap_command = "printf 'bootstrap\n'"
rust_fmt_check_command = "printf 'fmt\n'"
rust_clippy_command = "printf 'clippy\n'"
rust_test_command = "printf 'test\n'"
rust_test_locked_command = "printf 'test locked\n'"
"#,
        )
        .required_commands([
            "bootstrap_command",
            "rust_fmt_check_command",
            "rust_clippy_command",
            "rust_test_command",
            "rust_test_locked_command",
        ])
        .tool(json!({ "name": "jig.bootstrap", "kind": "command", "description": "Run bootstrap.", "command": "bootstrap_command" }))
        .tool(json!({ "name": "jig.fmt_check", "kind": "command", "description": "Run fmt.", "command": "rust_fmt_check_command" }))
        .tool(json!({ "name": "jig.clippy", "kind": "command", "description": "Run clippy.", "command": "rust_clippy_command" }))
        .tool(json!({ "name": "jig.test", "kind": "command", "description": "Run tests.", "command": "rust_test_command" }))
        .tool(json!({ "name": "jig.test_locked", "kind": "command", "description": "Run locked tests.", "command": "rust_test_locked_command" }))
        .tool(json!({ "name": "jig.contract_check", "kind": "native", "description": "Run native contract check." }))
        .write();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Contract(crate::cli::CheckTargetOpts {
                tool: crate::cli::ToolOpts {
                    plan_id: None,
                    no_receipt: true,
                },
                selectors: Vec::new(),
            }),
        )),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["receipt_id"], serde_json::Value::Null);
    assert!(
        output["result"]["stdout"]
            .as_str()
            .unwrap()
            .contains("jig contract check passed")
    );
    assert!(!temp.path().join(".agent/state/receipts.jsonl").exists());
}

#[test]
fn failed_tool_error_remains_primary_when_receipt_append_fails() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
rust_test_command = "printf 'tool failed stdout\n'; printf 'tool failed stderr\n' >&2; exit 7"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": "jig.test",
            "kind": "command",
            "description": "Run configured test command.",
            "command": "rust_test_command"
        }))
        .write();
    fs::write(temp.path().join(".agent/state"), "not a directory").unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Test(crate::cli::CheckTargetOpts {
                tool: crate::cli::ToolOpts {
                    plan_id: None,
                    no_receipt: false,
                },
                selectors: Vec::new(),
            }),
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("jig.test failed with status 7"), "{error}");
    assert!(error.contains("command key: rust_test_command"), "{error}");
    assert!(error.contains("tool failed stdout"), "{error}");
    assert!(error.contains("tool failed stderr"), "{error}");
    assert!(error.contains("receipt recording also failed"), "{error}");
}

#[test]
fn collect_result_keeps_failed_tool_context_when_receipt_append_fails() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
rust_test_command = "printf 'tool failed stdout\n'; printf 'tool failed stderr\n' >&2; exit 7"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": "jig.test",
            "kind": "command",
            "description": "Run configured test command.",
            "command": "rust_test_command"
        }))
        .write();
    fs::write(temp.path().join(".agent/state"), "not a directory").unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = tool_execution::execute_manifest_tool_result_without_worktree_fingerprint(
        &ctx,
        crate::tool_defs::tool::TEST,
        json!({}),
        None,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("jig.test failed with status 7"), "{error}");
    assert!(error.contains("command key: rust_test_command"), "{error}");
    assert!(error.contains("tool failed stdout"), "{error}");
    assert!(error.contains("tool failed stderr"), "{error}");
    assert!(error.contains("receipt recording also failed"), "{error}");
}
