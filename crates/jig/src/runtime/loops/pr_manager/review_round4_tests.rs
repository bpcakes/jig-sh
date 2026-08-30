#[cfg(test)]
mod review_round4_tests {
    use tempfile::tempdir;

    use super::*;
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

    #[test]
    fn workflow_ids_cannot_escape_the_pr_worktree_cache() {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();

        let root = pr_worktree_root(&ctx, &workflow().id);
        let expected_parent = temp.path().join(LOOP_CACHE_DIR).join("worktrees");

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
        };

        let action = record_pr_repair_outcome(
            &repair,
            &mut attempts,
            Ok(PrRepairOutcome::WorkerCancelled {
                before_start: false,
                worker_receipt_id: "receipt-worker".into(),
                worktree: worktree.clone(),
            }),
            None,
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
        };

        let action = record_pr_repair_outcome(
            &repair,
            &mut attempts,
            Ok(PrRepairOutcome::Completed(json!({
                "kind": "pr_manager_worker",
                "status": "attempted",
                "worktree": worktree,
                "worker_receipt_id": "receipt-worker",
                "push": {"final_head": "pushed-head"},
            }))),
            None,
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
        let failed = record_pr_repair_outcome(
            &repair,
            &mut attempts,
            Ok(PrRepairOutcome::Failed {
                error: anyhow!("worker output was invalid"),
                worktree: failed_worktree.clone(),
            }),
            None,
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
