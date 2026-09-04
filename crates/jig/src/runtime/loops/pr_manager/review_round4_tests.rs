#[cfg(test)]
mod review_round4_tests {
    use tempfile::tempdir;

    use super::*;
    use crate::runtime::loops::authority::resolve_protected_loop_authority;
    use crate::runtime::loops::workflow::{WorkflowCompletion, WorkflowOutcome};

    fn workflow() -> ResolvedWorkflow {
        ResolvedWorkflow {
            id: "../../ExampleProject".into(),
            kind: super::super::workflow::PR_MANAGER_KIND.into(),
            enabled: true,
            configured: true,
            lease_ttl_seconds: 60,
            max_attempts: 2,
            backoff_seconds: 1,
            codex_home_configured: None,
            schedule: None,
            codex_task: None,
        }
    }

    fn item() -> PrWorkItem {
        PrWorkItem {
            pr_number: 7,
            item_key: "pr-7".into(),
            title: "Example repair".into(),
            base_ref: "main".into(),
            head_ref: "repair/example".into(),
            head_sha: "a".repeat(40),
            reasons: vec!["failing_checks".into()],
        }
    }

    fn repair_worktree_fixture() -> (tempfile::TempDir, RepoContext, PathBuf, String) {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "fixture@example.com"],
            vec!["config", "user.name", "Fixture"],
            vec!["add", "."],
            vec!["commit", "-m", "fixture"],
        ] {
            let output = Command::new("git")
                .current_dir(temp.path())
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
        }
        let head = String::from_utf8(
            Command::new("git")
                .current_dir(temp.path())
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();
        let worktree = temp.path().join("repair-worktree");
        let output = Command::new("git")
            .current_dir(temp.path())
            .args(["worktree", "add", "--detach", worktree.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        (temp, ctx, worktree, head)
    }

    #[test]
    fn workflow_ids_cannot_escape_the_durable_pr_worktree_root() {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();

        let root = pr_worktree_root(&ctx, &workflow().id);
        let expected_parent = temp
            .path()
            .join(LOOP_RUNTIME_DIR)
            .join("worktrees/prs");

        assert_eq!(root.parent(), Some(expected_parent.as_path()));
        assert_eq!(
            root.file_name().unwrap().to_string_lossy().len(),
            64,
            "workflow ids should be represented by one SHA-256 path component"
        );
    }

    #[test]
    fn post_start_worker_cancellation_preserves_attention_evidence() {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let workflow = workflow();
        let item = item();
        let worktree = temp.path().join("retained-pr-worktree");
        let mut attempts = AttemptStore::new(&ctx);
        let lease = json!({"owner": "test"});
        let repair = PrRepairContext {
            repo: &ctx,
            workflow: &workflow,
            item: &item,
            lease: &lease,
            codex_home: None,
            worktree_reservation: None,
        };

        let action = record_pr_repair_outcome_under_branch_lease(
            &repair,
            &mut attempts,
            PrRepairOutcome::WorkerCancelled {
                before_start: false,
                worker_receipt_id: "receipt-worker".into(),
                worktree: PreparedPrWorktree::Created(worktree.clone()),
            },
        )
        .unwrap();
        let completion = WorkflowCompletion::from_actions(std::slice::from_ref(&action));

        assert_eq!(action["status"], "needs_attention");
        assert_eq!(action["attention_kind"], "cancelled_after_start");
        assert_eq!(action["worker_receipt_id"], "receipt-worker");
        assert_eq!(action["worktree"], worktree.display().to_string());
        assert_eq!(completion.outcome, WorkflowOutcome::NeedsAttention);
        assert_eq!(
            completion.worker_receipt_id.as_deref(),
            Some("receipt-worker")
        );
        assert_eq!(
            completion.worktree.as_deref(),
            Some(worktree.to_string_lossy().as_ref())
        );
        assert!(attempts.snapshot().unwrap().is_empty());
    }

    #[test]
    fn release_failure_does_not_overwrite_prior_cleanup_authority_loss() {
        let authority_error = anyhow!("cleanup authority was lost");
        let release_error = anyhow!("lease release also failed");
        let action = branch_lease_cleanup_attention(
            json!({"status": "failed", "error": "worker did not start"}),
            &authority_error,
        );

        let action = with_branch_lease_result(action, Some(&release_error));

        assert_eq!(action["attention_kind"], "branch_lease_lost_before_cleanup");
        assert_eq!(action["lease_error"], authority_error.to_string());
        assert_eq!(action["lease_release_error"], release_error.to_string());
    }

    #[cfg(unix)]
    #[test]
    fn pr_worktree_cleanup_runs_before_branch_lease_release() {
        use std::os::unix::fs::PermissionsExt as _;

        let _env_lock = crate::test_env::lock_env();
        let (temp, ctx, worktree, head) = repair_worktree_fixture();
        let lease_path = resolve_protected_loop_authority(temp.path())
            .unwrap()
            .expect("Git fixture needs protected loop authority")
            .dir
            .join("leases.json");
        let observed = temp.path().join("lease-observed-during-cleanup");
        let git = temp.path().join("lease-checking-git");
        fs::write(
            &git,
            r#"#!/bin/sh
case "$*" in
  *"worktree remove"*)
    grep -q 'branch:repair/example' "$JIG_TEST_LEASE_PATH" || exit 42
    printf observed > "$JIG_TEST_LEASE_OBSERVED"
    ;;
esac
exec git "$@"
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&git).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&git, permissions).unwrap();
        let _git = crate::test_env::EnvVarGuard::set(crate::bootstrap::GIT_BIN_ENV, git.as_os_str());
        let _lease_path = crate::test_env::EnvVarGuard::set("JIG_TEST_LEASE_PATH", lease_path.as_os_str());
        let _observed = crate::test_env::EnvVarGuard::set("JIG_TEST_LEASE_OBSERVED", observed.as_os_str());
        let workflow = workflow();
        let mut item = item();
        item.head_sha = head;
        let mut leases = LeaseStore::new(&ctx);
        let LeaseAcquire::Acquired(lease) = leases.acquire("branch:repair/example", 60).unwrap()
        else {
            panic!("expected branch lease acquisition");
        };
        let guard = LeaseGuard::start(leases.clone(), "branch:repair/example", &lease, 60).unwrap();
        let repair = PrRepairContext {
            repo: &ctx,
            workflow: &workflow,
            item: &item,
            lease: &lease,
            codex_home: None,
            worktree_reservation: None,
        };

        let action = finalize_pr_repair_outcome(
            &repair,
            &mut AttemptStore::new(&ctx),
            PrRepairOutcome::PreExecutionFailed {
                error: anyhow!("worker did not start"),
                worktree: Some(PreparedPrWorktree::Created(worktree.clone())),
                worker_receipt_id: None,
            },
            guard,
        )
        .unwrap();

        assert_eq!(action["worktree_retained"], false, "{action:#}");
        assert!(!worktree.exists());
        assert!(observed.is_file(), "Git wrapper never observed cleanup");
        assert!(matches!(
            leases.acquire("branch:repair/example", 60).unwrap(),
            LeaseAcquire::Acquired(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn pr_worktree_cleanup_stops_when_the_branch_lease_is_reassigned_mid_removal() {
        use std::os::unix::fs::PermissionsExt as _;

        let _env_lock = crate::test_env::lock_env();
        let (temp, ctx, worktree, head) = repair_worktree_fixture();
        let started = temp.path().join("cleanup-started");
        let reassigned = temp.path().join("lease-reassigned");
        let real_git = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap();
        assert!(real_git.status.success(), "{real_git:?}");
        let real_git = String::from_utf8(real_git.stdout).unwrap();
        let git = temp.path().join("lease-loss-during-cleanup-git");
        fs::write(
            &git,
            r#"#!/bin/sh
case "$*" in
  *"worktree remove"*)
    touch "$JIG_TEST_CLEANUP_STARTED"
    while [ ! -f "$JIG_TEST_LEASE_REASSIGNED" ]; do sleep 0.05; done
    sleep 5
    ;;
esac
exec "$JIG_TEST_REAL_GIT" "$@"
"#,
        )
        .unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
        let _real_git = crate::test_env::EnvVarGuard::set(
            "JIG_TEST_REAL_GIT",
            std::ffi::OsStr::new(real_git.trim()),
        );
        let _started = crate::test_env::EnvVarGuard::set(
            "JIG_TEST_CLEANUP_STARTED",
            started.as_os_str(),
        );
        let _reassigned = crate::test_env::EnvVarGuard::set(
            "JIG_TEST_LEASE_REASSIGNED",
            reassigned.as_os_str(),
        );
        let _git = crate::test_env::EnvVarGuard::set(crate::bootstrap::GIT_BIN_ENV, git.as_os_str());

        let workflow = workflow();
        let mut item = item();
        item.head_sha = head;
        let mut leases = LeaseStore::new(&ctx);
        let mut replacement_store = leases.clone();
        let LeaseAcquire::Acquired(lease) = leases.acquire("branch:repair/example", 1).unwrap()
        else {
            panic!("expected branch lease acquisition");
        };
        let guard = LeaseGuard::start(leases.clone(), "branch:repair/example", &lease, 1).unwrap();
        let repair = PrRepairContext {
            repo: &ctx,
            workflow: &workflow,
            item: &item,
            lease: &lease,
            codex_home: None,
            worktree_reservation: None,
        };
        let replacement = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !started.exists() {
                assert!(Instant::now() < deadline, "worktree removal never started");
                std::thread::sleep(Duration::from_millis(10));
            }
            replacement_store
                .revoke_for_test("branch:repair/example")
                .unwrap();
            assert!(matches!(
                replacement_store
                    .acquire("branch:repair/example", 60)
                    .unwrap(),
                LeaseAcquire::Acquired(_)
            ));
            fs::write(reassigned, "reassigned\n").unwrap();
        });

        let action = finalize_pr_repair_outcome(
            &repair,
            &mut AttemptStore::new(&ctx),
            PrRepairOutcome::PreExecutionFailed {
                error: anyhow!("worker did not start"),
                worktree: Some(PreparedPrWorktree::Created(worktree.clone())),
                worker_receipt_id: None,
            },
            guard,
        )
        .unwrap();
        replacement.join().unwrap();

        assert_eq!(action["status"], "needs_attention", "{action:#}");
        assert_eq!(action["worktree_retained"], true, "{action:#}");
        assert!(worktree.exists(), "former lease owner removed the worktree");
    }

    #[test]
    fn pre_start_worker_cancellation_is_reported_as_unexecuted() {
        let _env_lock = crate::test_env::lock_env();
        let _git = crate::test_env::EnvVarGuard::set(
            crate::bootstrap::GIT_BIN_ENV,
            std::ffi::OsStr::new("git"),
        );
        let (_temp, ctx, worktree, head) = repair_worktree_fixture();
        let workflow = workflow();
        let mut item = item();
        item.head_sha = head;
        let mut attempts = AttemptStore::new(&ctx);
        let lease = json!({"owner": "test"});
        let repair = PrRepairContext {
            repo: &ctx,
            workflow: &workflow,
            item: &item,
            lease: &lease,
            codex_home: None,
            worktree_reservation: None,
        };

        let action = record_pr_repair_outcome_under_branch_lease(
            &repair,
            &mut attempts,
            PrRepairOutcome::WorkerCancelled {
                before_start: true,
                worker_receipt_id: "receipt-worker".into(),
                worktree: PreparedPrWorktree::Created(worktree.clone()),
            },
        )
        .unwrap();
        let completion = pr_manager_completion(std::slice::from_ref(&action));

        assert_eq!(
            completion.execution,
            WorkflowExecution::Unexecuted(UnexecutedReason::CancelledBeforeStart)
        );
        assert_eq!(completion.worker_receipt_id.as_deref(), Some("receipt-worker"));
        assert_eq!(action["worktree_retained"], false);
        assert!(!worktree.exists());
        assert!(attempts.snapshot().unwrap().is_empty());
    }

    #[test]
    fn pre_start_cancellation_retains_worktree_without_cleanup_authority() {
        let _env_lock = crate::test_env::lock_env();
        let _git = crate::test_env::EnvVarGuard::set(
            crate::bootstrap::GIT_BIN_ENV,
            std::ffi::OsStr::new("git"),
        );
        let (_temp, ctx, worktree, head) = repair_worktree_fixture();
        let workflow = workflow();
        let mut item = item();
        item.head_sha = head;
        let mut attempts = AttemptStore::new(&ctx);
        let lease = json!({"owner": "test"});
        let repair = PrRepairContext {
            repo: &ctx,
            workflow: &workflow,
            item: &item,
            lease: &lease,
            codex_home: None,
            worktree_reservation: None,
        };
        let release_error = anyhow!("branch lease belongs to another dispatcher");
        let mut cleanup = PrWorktreeCleanup::assuming_lease(&ctx);

        let action = record_pr_repair_outcome(
            &repair,
            &mut attempts,
            PrRepairOutcome::WorkerCancelled {
                before_start: true,
                worker_receipt_id: "receipt-worker".into(),
                worktree: PreparedPrWorktree::Created(worktree.clone()),
            },
            Some(&release_error),
            &mut cleanup,
        )
        .unwrap();
        let completion = pr_manager_completion(std::slice::from_ref(&action));

        assert_eq!(action["status"], "needs_attention");
        assert_eq!(
            action["attention_kind"],
            "branch_lease_lost_before_cleanup"
        );
        assert_eq!(action["unexecuted_reason"], "cancelled_before_start");
        assert_eq!(action["worktree_retained"], true);
        assert_eq!(action["worker_receipt_id"], "receipt-worker");
        assert!(worktree.exists());
        assert_eq!(completion.outcome, WorkflowOutcome::NeedsAttention);
        assert_eq!(
            completion.execution,
            WorkflowExecution::Unexecuted(UnexecutedReason::CancelledBeforeStart)
        );
        assert_eq!(
            completion.worktree.as_deref(),
            Some(worktree.to_string_lossy().as_ref())
        );
        assert!(attempts.snapshot().unwrap().is_empty());
    }

    #[test]
    fn pre_execution_failure_cleans_the_worktree_without_consuming_an_attempt() {
        let _env_lock = crate::test_env::lock_env();
        let _git = crate::test_env::EnvVarGuard::set(
            crate::bootstrap::GIT_BIN_ENV,
            std::ffi::OsStr::new("git"),
        );
        let (_temp, ctx, worktree, head) = repair_worktree_fixture();
        let workflow = workflow();
        let mut item = item();
        item.head_sha = head;
        let mut attempts = AttemptStore::new(&ctx);
        let lease = json!({"owner": "test"});
        let repair = PrRepairContext {
            repo: &ctx,
            workflow: &workflow,
            item: &item,
            lease: &lease,
            codex_home: None,
            worktree_reservation: None,
        };

        let action = record_pr_repair_outcome_under_branch_lease(
            &repair,
            &mut attempts,
            PrRepairOutcome::PreExecutionFailed {
                error: anyhow!("worker process could not start"),
                worktree: Some(PreparedPrWorktree::Created(worktree.clone())),
                worker_receipt_id: Some("receipt-worker".into()),
            },
        )
        .unwrap();
        let completion = pr_manager_completion(std::slice::from_ref(&action));

        assert_eq!(
            completion.execution,
            WorkflowExecution::Unexecuted(UnexecutedReason::PreExecutionError)
        );
        assert_eq!(completion.worker_receipt_id.as_deref(), Some("receipt-worker"));
        assert_eq!(action["worktree_retained"], false);
        assert!(!worktree.exists());
        assert!(attempts.snapshot().unwrap().is_empty());
    }

    #[test]
    fn failed_worker_retains_dirty_worktree_and_receipt() {
        let _env_lock = crate::test_env::lock_env();
        let _git = crate::test_env::EnvVarGuard::set(
            crate::bootstrap::GIT_BIN_ENV,
            std::ffi::OsStr::new("git"),
        );
        let (_temp, ctx, worktree, head) = repair_worktree_fixture();
        fs::write(worktree.join("partial.txt"), "preserve me\n").unwrap();
        let workflow = workflow();
        let mut item = item();
        item.head_sha = head;
        let mut attempts = AttemptStore::new(&ctx);
        let lease = json!({"owner": "test"});
        let repair = PrRepairContext {
            repo: &ctx,
            workflow: &workflow,
            item: &item,
            lease: &lease,
            codex_home: None,
            worktree_reservation: None,
        };

        let action = record_pr_repair_outcome_under_branch_lease(
            &repair,
            &mut attempts,
            PrRepairOutcome::WorkerFailed {
                error: anyhow!("worker output was malformed"),
                worker_receipt_id: Some("receipt-worker".into()),
                worktree: worktree.clone(),
            },
        )
        .unwrap();

        assert_eq!(action["status"], "needs_attention");
        assert_eq!(
            action["attention_kind"],
            "failed_repair_worktree_retained"
        );
        assert_eq!(action["worker_receipt_id"], "receipt-worker");
        assert_eq!(action["worktree_retained"], true);
        assert!(worktree.join("partial.txt").exists());
    }

    #[test]
    fn failed_worker_retains_a_clean_local_commit() {
        let _env_lock = crate::test_env::lock_env();
        let _git = crate::test_env::EnvVarGuard::set(
            crate::bootstrap::GIT_BIN_ENV,
            std::ffi::OsStr::new("git"),
        );
        let (_temp, ctx, worktree, head) = repair_worktree_fixture();
        fs::write(worktree.join("committed.txt"), "preserve commit\n").unwrap();
        for args in [
            vec!["add", "committed.txt"],
            vec!["commit", "-m", "worker repair"],
        ] {
            let output = Command::new("git")
                .current_dir(&worktree)
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
        }
        let workflow = workflow();
        let mut item = item();
        item.head_sha = head;
        let mut attempts = AttemptStore::new(&ctx);
        let lease = json!({"owner": "test"});
        let repair = PrRepairContext {
            repo: &ctx,
            workflow: &workflow,
            item: &item,
            lease: &lease,
            codex_home: None,
            worktree_reservation: None,
        };

        let action = record_pr_repair_outcome_under_branch_lease(
            &repair,
            &mut attempts,
            PrRepairOutcome::WorkerFailed {
                error: anyhow!("git push failed before starting"),
                worker_receipt_id: Some("receipt-worker".into()),
                worktree: worktree.clone(),
            },
        )
        .unwrap();

        assert_eq!(
            action["attention_kind"],
            "failed_repair_worktree_retained"
        );
        assert_eq!(action["worktree_retained"], true);
        assert!(worktree.exists());
    }

    #[test]
    fn failed_worker_removes_a_clean_unchanged_worktree() {
        let _env_lock = crate::test_env::lock_env();
        let _git = crate::test_env::EnvVarGuard::set(
            crate::bootstrap::GIT_BIN_ENV,
            std::ffi::OsStr::new("git"),
        );
        let (_temp, ctx, worktree, head) = repair_worktree_fixture();
        let workflow = workflow();
        let mut item = item();
        item.head_sha = head;
        let mut attempts = AttemptStore::new(&ctx);
        let lease = json!({"owner": "test"});
        let repair = PrRepairContext {
            repo: &ctx,
            workflow: &workflow,
            item: &item,
            lease: &lease,
            codex_home: None,
            worktree_reservation: None,
        };

        let action = record_pr_repair_outcome_under_branch_lease(
            &repair,
            &mut attempts,
            PrRepairOutcome::WorkerFailed {
                error: anyhow!("worker output was malformed"),
                worker_receipt_id: Some("receipt-worker".into()),
                worktree: worktree.clone(),
            },
        )
        .unwrap();

        assert_eq!(action["status"], "failed");
        assert_eq!(action["worktree_retained"], false);
        assert!(!worktree.exists());
        assert_eq!(attempts.snapshot().unwrap().len(), 1);
    }

    #[test]
    fn failed_worker_retains_unchanged_worktree_without_cleanup_authority() {
        let _env_lock = crate::test_env::lock_env();
        let _git = crate::test_env::EnvVarGuard::set(
            crate::bootstrap::GIT_BIN_ENV,
            std::ffi::OsStr::new("git"),
        );
        let (_temp, ctx, worktree, head) = repair_worktree_fixture();
        let workflow = workflow();
        let mut item = item();
        item.head_sha = head;
        let mut attempts = AttemptStore::new(&ctx);
        let lease = json!({"owner": "test"});
        let repair = PrRepairContext {
            repo: &ctx,
            workflow: &workflow,
            item: &item,
            lease: &lease,
            codex_home: None,
            worktree_reservation: None,
        };
        let release_error = anyhow!("branch lease belongs to another dispatcher");
        let mut cleanup = PrWorktreeCleanup::assuming_lease(&ctx);

        let action = record_pr_repair_outcome(
            &repair,
            &mut attempts,
            PrRepairOutcome::WorkerFailed {
                error: anyhow!("worker output was malformed"),
                worker_receipt_id: Some("receipt-worker".into()),
                worktree: worktree.clone(),
            },
            Some(&release_error),
            &mut cleanup,
        )
        .unwrap();

        assert_eq!(action["status"], "needs_attention");
        assert_eq!(
            action["attention_kind"],
            "branch_lease_lost_after_start"
        );
        assert_eq!(action["completed_status"], "failed");
        assert_eq!(action["completed_error"], "worker output was malformed");
        assert_eq!(action["lease_error"], release_error.to_string());
        assert_eq!(action["worktree_retained"], true);
        assert_eq!(action["worker_receipt_id"], "receipt-worker");
        assert!(worktree.exists());
        assert_eq!(attempts.snapshot().unwrap().len(), 1);
    }

    #[test]
    fn attempt_persistence_failure_preserves_completed_repair_evidence() {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let workflow = workflow();
        let item = item();
        let worktree = temp.path().join("retained-pr-worktree");
        std::fs::create_dir(&worktree).unwrap();
        std::fs::create_dir_all(temp.path().join(LOOP_CACHE_DIR).join("attempts.json")).unwrap();
        let mut attempts = AttemptStore::new(&ctx);
        let lease = json!({"owner": "test"});
        let repair = PrRepairContext {
            repo: &ctx,
            workflow: &workflow,
            item: &item,
            lease: &lease,
            codex_home: None,
            worktree_reservation: None,
        };

        let action = record_pr_repair_outcome_under_branch_lease(
            &repair,
            &mut attempts,
            PrRepairOutcome::Completed {
                action: json!({
                    "kind": "pr_manager_worker",
                    "status": "attempted",
                    "worktree": worktree,
                    "worker_receipt_id": "receipt-worker",
                    "push": {"final_head": "pushed-head"},
                }),
                worktree: worktree.clone(),
            },
        )
        .unwrap();

        assert_eq!(action["status"], "needs_attention");
        assert_eq!(
            action["attention_kind"],
            "attempt_state_persistence_failed"
        );
        assert_eq!(action["completed_status"], "attempted");
        assert_eq!(action["worker_receipt_id"], "receipt-worker");
        assert_eq!(action["push"]["final_head"], "pushed-head");
        assert_eq!(action["worktree_retained"], true);
        assert!(worktree.exists());
        assert!(action.get("attempt").is_none());
        assert!(
            action["attempt_error"]
                .as_str()
                .unwrap()
                .contains("attempts.json")
        );

        let failed_worktree = temp.path().join("failed-pr-worktree");
        std::fs::create_dir(&failed_worktree).unwrap();
        let failed = record_pr_repair_outcome_under_branch_lease(
            &repair,
            &mut attempts,
            PrRepairOutcome::WorkerFailed {
                error: anyhow!("worker output was invalid"),
                worker_receipt_id: None,
                worktree: failed_worktree.clone(),
            },
        )
        .unwrap();

        assert_eq!(failed["status"], "needs_attention");
        assert_eq!(
            failed["attention_kind"],
            "attempt_state_persistence_failed"
        );
        assert_eq!(failed["completed_status"], "failed");
        assert_eq!(failed["completed_error"], "worker output was invalid");
        assert_eq!(failed["worktree_retained"], true);
        assert!(failed_worktree.exists());
    }
}
