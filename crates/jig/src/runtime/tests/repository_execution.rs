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

include!("repository_execution_parts/part_02.rs");
