#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

    #[test]
    fn exhausted_attempt_is_not_an_occurrence_attention_action() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let workflow = ResolvedWorkflow {
            id: "pr-status".into(),
            kind: super::super::workflow::PR_MANAGER_KIND.into(),
            enabled: true,
            configured: true,
            lease_ttl_seconds: 60,
            max_attempts: 1,
            backoff_seconds: 1,
            codex_home_configured: None,
            schedule: None,
            codex_task: None,
        };
        let item = PrWorkItem {
            pr_number: 7,
            item_key: "pr-7".into(),
            title: "Repair example".into(),
            base_ref: "main".into(),
            head_ref: "example/repair".into(),
            head_sha: "abc123".into(),
            reasons: vec!["failing_checks".into()],
        };
        let mut attempts = AttemptStore::new(&ctx);
        let attempt = attempts
            .record_attempt_for_version(
                &workflow,
                &item.item_key,
                Some(&item.head_sha),
                "failed",
            )
            .unwrap();
        assert!(attempt.exhausted);

        let action = attempt_blocking_action(&workflow, &mut attempts, &item)
            .unwrap()
            .unwrap();
        let completion = WorkflowTick::from_actions(Value::Null, vec![action.clone()]).completion;

        assert_eq!(action["status"], "needs_attention");
        assert_eq!(action["attention_kind"], "exhausted_attempt");
        assert_eq!(action["attempt"]["exhausted"], true);
        assert_eq!(
            completion.outcome,
            super::super::workflow::WorkflowOutcome::Succeeded
        );
        assert!(!pr_manager_action_consumed_tick(&action));
    }

    #[test]
    fn git_command_uses_configured_program_and_scrubs_repository_redirects() {
        let _env_lock = lock_env();
        let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, "custom-git");
        let _git_dir = EnvVarGuard::set("GIT_DIR", "/tmp/redirected.git");
        let _git_trace = EnvVarGuard::set("GIT_TRACE", "1");
        let _git_ssh = EnvVarGuard::set("GIT_SSH_COMMAND", "ssh -i test-key");

        let command = git_command(Path::new("/tmp/repository"), ["status", "--short"]);

        assert_eq!(command.get_program(), OsStr::new("custom-git"));
        assert_eq!(
            command.get_current_dir(),
            Some(Path::new("/tmp/repository"))
        );
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["--no-replace-objects", "status", "--short"]
                .map(OsStr::new)
                .as_slice()
        );
        assert!(
            command
                .get_envs()
                .any(|(name, value)| { name == OsStr::new("GIT_DIR") && value.is_none() })
        );
        assert!(
            command
                .get_envs()
                .any(|(name, value)| { name == OsStr::new("GIT_TRACE") && value.is_none() })
        );
        assert!(
            !command
                .get_envs()
                .any(|(name, _)| { name == OsStr::new("GIT_SSH_COMMAND") })
        );
    }

    #[test]
    fn classify_uses_observed_default_branch_when_base_ref_is_missing() {
        let pull_request = json!({
            "number": 7,
            "title": "Fix widgets",
            "is_draft": false,
            "head": {
                "ref": "codex/widgets",
                "sha": "abc123",
                "is_cross_repository": false,
            },
            "stack": {
                "is_stacked": false,
            },
            "mergeability": {
                "mergeable": "MERGEABLE",
                "merge_state_status": "CLEAN",
            },
            "review_decision": "REVIEW_REQUIRED",
            "checks": {
                "summary": {
                    "fail": 1,
                },
            },
            "review_threads": {
                "summary": {
                    "unresolved": 0,
                },
            },
        });

        match classify_pull_request(&pull_request, "trunk") {
            PrCandidate::Actionable(item) => assert_eq!(item.base_ref, "trunk"),
            PrCandidate::Skip(_) | PrCandidate::Idle(_) | PrCandidate::Pending(_) => {
                panic!("expected actionable PR candidate")
            }
        }
    }

    #[test]
    fn classify_pending_checks_are_not_observed_healthy() {
        let pull_request = json!({
            "number": 7,
            "title": "Fix widgets",
            "is_draft": false,
            "head": {
                "ref": "codex/widgets",
                "sha": "abc123",
                "is_cross_repository": false,
            },
            "stack": {
                "is_stacked": false,
            },
            "mergeability": {
                "mergeable": "MERGEABLE",
                "merge_state_status": "CLEAN",
            },
            "review_decision": "REVIEW_REQUIRED",
            "checks": {
                "summary": {
                    "fail": 0,
                    "pending": 1,
                },
            },
            "review_threads": {
                "summary": {
                    "unresolved": 0,
                },
            },
        });

        match classify_pull_request(&pull_request, "main") {
            PrCandidate::Pending(item) => {
                assert_eq!(item.item_key, "pr-7");
                assert_eq!(item.pending_checks, 1);
            }
            PrCandidate::Actionable(_) | PrCandidate::Skip(_) | PrCandidate::Idle(_) => {
                panic!("expected pending PR candidate")
            }
        }
    }
}
