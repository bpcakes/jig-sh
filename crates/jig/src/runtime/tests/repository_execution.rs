use super::*;

mod command_targets;
mod parallel;
mod results;

#[derive(Default)]
struct LeaseWaitObserver {
    output: Vec<u8>,
    cancelled: bool,
    cancel_on_wait: bool,
    flushes: usize,
    wait_notice: Option<std::sync::mpsc::SyncSender<()>>,
}

impl jig_execution::ExecutionObserver for LeaseWaitObserver {
    fn event(&mut self, event: jig_execution::ExecutionEvent<'_>) {
        if let jig_execution::ExecutionEvent::Output { bytes, .. } = event {
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

impl jig_execution::ExecutionCancellation for LeaseWaitObserver {
    fn cancelled(&self) -> bool {
        self.cancelled
    }
}

#[derive(Default)]
struct PhaseRecordingObserver {
    started: Vec<String>,
    finished: Vec<(String, bool)>,
}

impl jig_execution::ExecutionObserver for PhaseRecordingObserver {
    fn event(&mut self, event: jig_execution::ExecutionEvent<'_>) {
        match event {
            jig_execution::ExecutionEvent::PhaseStarted { label, .. } => {
                self.started.push(label.to_owned());
            }
            jig_execution::ExecutionEvent::PhaseFinished { label, success, .. } => {
                self.finished.push((label.to_owned(), success));
            }
            jig_execution::ExecutionEvent::Output { .. }
            | jig_execution::ExecutionEvent::Heartbeat { .. } => {}
        }
    }
}

impl jig_execution::ExecutionCancellation for PhaseRecordingObserver {}

struct MarkerCancellationObserver {
    marker: std::path::PathBuf,
}

impl jig_execution::ExecutionObserver for MarkerCancellationObserver {}

impl jig_execution::ExecutionCancellation for MarkerCancellationObserver {
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
    let catalog = jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = jig_repository::plan_run(
        &ctx,
        &catalog,
        jig_repository::PlanRunRequest {
            affected_base: Some("HEAD".into()),
            ..jig_repository::PlanRunRequest::default()
        },
    )
    .unwrap();
    assert!(plan.targets.is_empty());
    fs::write(temp.path().join("api/example.go"), "package changed\n").unwrap();

    let mut observer = jig_execution::NoopExecutionObserver;
    let error = super::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            alias_override: None,
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
    let catalog = jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let config_path = temp.path().join(".jig.toml");
    let changed = fs::read_to_string(&config_path).unwrap().replace(
        "api_test_command = \"printf 'api tests passed\\n'\"",
        "api_test_command = \"printf 'changed command\\n'\"",
    );
    fs::write(&config_path, changed).unwrap();
    let plan = jig_repository::plan_run(
        &ctx,
        &catalog,
        jig_repository::PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..jig_repository::PlanRunRequest::default()
        },
    )
    .unwrap();

    let mut observer = jig_execution::NoopExecutionObserver;
    let error = super::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            alias_override: None,
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
    let catalog = jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = jig_repository::plan_run(
        &ctx,
        &catalog,
        jig_repository::PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..jig_repository::PlanRunRequest::default()
        },
    )
    .unwrap();
    let held = jig_state::acquire_repository_execution_lease(
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
            alias_override: None,
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
    let catalog = jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = jig_repository::plan_run(
        &ctx,
        &catalog,
        jig_repository::PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..jig_repository::PlanRunRequest::default()
        },
    )
    .unwrap();
    let held = jig_state::acquire_repository_execution_lease(
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
            alias_override: None,
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
    let catalog = jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = jig_repository::plan_run(
        &ctx,
        &catalog,
        jig_repository::PlanRunRequest {
            affected_base: Some("HEAD".into()),
            ..jig_repository::PlanRunRequest::default()
        },
    )
    .unwrap();
    assert!(plan.targets.is_empty());
    let (run, _lease) = super::run_execution::start_check_run(&ctx, &catalog, plan).unwrap();
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
            alias_override: None,
            fail_fast: false,
        },
        &|| Ok(false),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("execution authority changed"), "{error}");
    assert_eq!(
        jig_state::run_by_id(&ctx, &run_id)
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
    let catalog = jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = jig_repository::plan_run(
        &ctx,
        &catalog,
        jig_repository::PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..jig_repository::PlanRunRequest::default()
        },
    )
    .unwrap();

    let mut observer = PhaseRecordingObserver::default();
    let execution = super::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            alias_override: None,
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
                comparison: None,
                explain: true,
                fail_fast: false,
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("contract version 6 or later"));
}
