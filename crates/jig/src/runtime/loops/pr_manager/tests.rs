#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

    #[test]
    fn remote_branch_names_are_never_passed_as_git_options() {
        assert_eq!(
            remote_branch_ref("-upload-pack=fixture"),
            "refs/heads/-upload-pack=fixture"
        );
        assert_eq!(remote_branch_ref("repair/example"), "refs/heads/repair/example");
    }

    #[test]
    fn remote_head_parser_requires_the_exact_requested_ref() {
        let stdout = b"abc123\trefs/heads/example\ndef456\trefs/heads/example-old\n";

        assert_eq!(
            remote_head_from_ls_remote(stdout, "refs/heads/example"),
            Some("abc123")
        );
        assert_eq!(
            remote_head_from_ls_remote(stdout, "refs/heads/missing"),
            None
        );
    }

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
            head_sha: "a".repeat(40),
            reasons: vec!["failing_checks".into()],
        };
        let mut attempts = AttemptStore::new(&ctx);
        let attempt = attempts
            .record_attempt_for_transition(
                &workflow,
                &item.item_key,
                Some(&item.head_sha),
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
        assert!(!pr_manager_action_consumed_tick(&action));
        assert_eq!(
            completion.outcome,
            super::super::workflow::WorkflowOutcome::Succeeded
        );
        assert!(!pr_manager_action_consumed_tick(&action));
    }

    #[test]
    fn side_effectful_attention_consumes_the_pr_manager_tick() {
        for attention_kind in [
            "cancelled_after_start",
            "ambiguous_push",
            "branch_lease_lost_after_start",
            "worktree_cleanup_failed",
        ] {
            assert!(pr_manager_action_consumed_tick(&json!({
                "status": "needs_attention",
                "attention_kind": attention_kind,
            })));
        }
    }

    #[test]
    fn failed_pr_worktree_cleanup_preserves_attention_evidence() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(crate::bootstrap::GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
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
        let worktree = temp.path().join("repair-worktree");
        let output = Command::new("git")
            .current_dir(temp.path())
            .args(["worktree", "add", "--detach", worktree.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        fs::write(worktree.join("partial.txt"), "preserve me\n").unwrap();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut cleanup = PrWorktreeCleanup::assuming_lease(&ctx);

        let action = finalize_pr_worktree(
            &mut cleanup,
            json!({
                "kind": "pr_manager_worker",
                "status": "attempted",
                "worktree": worktree,
                "worker_receipt_id": "receipt-worker",
                "error": null,
            }),
            &worktree,
            false,
        );

        assert_eq!(action["status"], "needs_attention");
        assert_eq!(action["attention_kind"], "worktree_cleanup_failed");
        assert_eq!(action["completed_status"], "attempted");
        assert_eq!(action["worktree_retained"], true);
        assert!(action["cleanup_error"].is_string());
        assert!(worktree.exists());
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
                "repository": {"name": "ExampleVault"},
                "repository_name_with_owner": "ExampleProject/ExampleVault",
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

        match classify_pull_request(
            &pull_request,
            "trunk",
            Some("ExampleProject/ExampleVault"),
        ) {
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
                "repository": {"name": "ExampleVault"},
                "repository_name_with_owner": "ExampleProject/ExampleVault",
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

        match classify_pull_request(
            &pull_request,
            "main",
            Some("ExampleProject/ExampleVault"),
        ) {
            PrCandidate::Pending(item) => {
                assert_eq!(item.item_key, "pr-7");
                assert_eq!(item.pending_checks, 1);
            }
            PrCandidate::Actionable(_) | PrCandidate::Skip(_) | PrCandidate::Idle(_) => {
                panic!("expected pending PR candidate")
            }
        }
    }

    #[test]
    fn classify_requires_an_exact_same_repository_head_identity() {
        let mut pull_request = json!({
            "number": 7,
            "is_draft": false,
            "head": {
                "ref": "repair/example",
                "sha": "abc123",
                "repository": {"name": "ExampleVault"},
                "repository_name_with_owner": "ExampleProject/ExampleVault",
                "is_cross_repository": false,
            },
            "stack": {"is_stacked": false},
            "mergeability": {"mergeable": "MERGEABLE", "merge_state_status": "CLEAN"},
            "checks": {"summary": {"fail": 0, "pending": 0}},
            "review_threads": {"summary": {"trusted_unresolved": 0}},
        });

        assert!(matches!(
            classify_pull_request(
                &pull_request,
                "main",
                Some("ExampleProject/ExampleVault"),
            ),
            PrCandidate::Idle(_)
        ));

        for malformed in [Value::Null, json!("false"), json!(0)] {
            pull_request["head"]["is_cross_repository"] = malformed;
            let PrCandidate::Skip(action) = classify_pull_request(
                &pull_request,
                "main",
                Some("ExampleProject/ExampleVault"),
            ) else {
                panic!("malformed cross-repository metadata must fail closed");
            };
            assert_eq!(action["reason"], "unverified_head_repository");
        }

        pull_request["head"]["is_cross_repository"] = json!(false);
        pull_request["head"]["repository_name_with_owner"] =
            json!("AnotherProject/ExampleVault");
        let PrCandidate::Skip(action) = classify_pull_request(
            &pull_request,
            "main",
            Some("ExampleProject/ExampleVault"),
        ) else {
            panic!("a mismatched head repository must be skipped");
        };
        assert_eq!(action["reason"], "cross_repository_pr");
    }

    #[test]
    fn pr_manager_refuses_a_truncated_pr_list_but_scopes_truncated_threads() {
        let pr_list = json!({
            "summary": {"pr_list_truncated": true},
            "pull_requests": [],
        });
        let pr_list_action = incomplete_pr_list_action(&pr_list).unwrap();
        assert_eq!(pr_list_action["status"], "failed");
        assert_eq!(pr_list_action["pr_list_truncated"], true);

        let review_threads = json!({
            "summary": {"pr_list_truncated": false},
            "pull_requests": [{
                "number": 7,
                "review_threads": {"page_info": {"truncated": true}},
            }],
        });
        let review_action =
            incomplete_pull_request_action(&review_threads["pull_requests"][0]).unwrap();
        assert_eq!(review_action["status"], "waiting");
        assert_eq!(review_action["pr_number"], 7);
        assert_eq!(review_action["review_threads_truncated"], true);
    }

    #[test]
    fn pr_manager_accepts_a_complete_snapshot() {
        let observed = json!({
            "summary": {"pr_list_truncated": false},
            "pull_requests": [{
                "number": 7,
                "review_threads": {"page_info": {"truncated": false}},
            }],
        });

        assert!(
            incomplete_pr_list_action(&observed).is_none()
        );
        assert!(incomplete_pull_request_action(&observed["pull_requests"][0]).is_none());
    }

    #[test]
    fn truncated_review_threads_do_not_clear_a_healthy_attempt() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let workflow = ResolvedWorkflow {
            id: "pr-manager".into(),
            kind: super::super::workflow::PR_MANAGER_KIND.into(),
            enabled: true,
            configured: true,
            lease_ttl_seconds: 60,
            max_attempts: 2,
            backoff_seconds: 1,
            codex_home_configured: None,
            schedule: None,
            codex_task: None,
        };
        let mut leases = LeaseStore::new(&ctx);
        let mut attempts = AttemptStore::new(&ctx);
        attempts
            .record_attempt_for_transition(
                &workflow,
                "pr-7",
                Some("a-head"),
                Some("a-head"),
                "failed",
            )
            .unwrap();
        let observed = json!({
            "summary": {"pr_list_truncated": false},
            "repository": {
                "default_branch": "main",
                "name_with_owner": "ExampleProject/ExampleVault",
            },
            "pull_requests": [{
                "number": 7,
                "review_threads": {"page_info": {"truncated": true}},
            }],
        });
        let mut observer = crate::execution::NoopExecutionObserver;

        let tick = pr_manager_tick_from_snapshot(
            &ctx,
            &workflow,
            &mut leases,
            &mut attempts,
            observed,
            PrManagerExecution {
                codex_home: None,
                worktree_reservation: None,
                observer: &mut observer,
            },
        )
        .unwrap();

        assert_eq!(tick.actions[0]["reason"], "incomplete_github_snapshot");
        assert_eq!(
            tick.completion.execution,
            WorkflowExecution::Executed
        );
        assert_eq!(attempts.snapshot().unwrap().len(), 1);
    }

    #[test]
    fn truncated_review_threads_do_not_block_healthy_pr_processing() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let workflow = ResolvedWorkflow {
            id: "pr-manager".into(),
            kind: super::super::workflow::PR_MANAGER_KIND.into(),
            enabled: true,
            configured: true,
            lease_ttl_seconds: 60,
            max_attempts: 2,
            backoff_seconds: 1,
            codex_home_configured: None,
            schedule: None,
            codex_task: None,
        };
        let mut leases = LeaseStore::new(&ctx);
        let mut attempts = AttemptStore::new(&ctx);
        attempts
            .record_attempt_for_transition(
                &workflow,
                "pr-8",
                Some("healthy-head"),
                Some("healthy-head"),
                "failed",
            )
            .unwrap();
        let observed = json!({
            "summary": {"pr_list_truncated": false},
            "repository": {
                "default_branch": "main",
                "name_with_owner": "ExampleProject/ExampleVault",
            },
            "pull_requests": [
                {
                    "number": 7,
                    "review_threads": {"page_info": {"truncated": true}}
                },
                {
                    "number": 8,
                    "head": {
                        "ref": "repair/healthy",
                        "sha": "healthy-head",
                        "repository": {"name": "ExampleVault"},
                        "repository_name_with_owner": "ExampleProject/ExampleVault",
                        "is_cross_repository": false,
                    },
                    "checks": {"summary": {"fail": 0, "pending": 0}},
                    "review_threads": {
                        "page_info": {"truncated": false},
                        "summary": {"trusted_unresolved": 0}
                    }
                }
            ],
        });
        let mut observer = crate::execution::NoopExecutionObserver;

        let tick = pr_manager_tick_from_snapshot(
            &ctx,
            &workflow,
            &mut leases,
            &mut attempts,
            observed,
            PrManagerExecution {
                codex_home: None,
                worktree_reservation: None,
                observer: &mut observer,
            },
        )
        .unwrap();

        assert_eq!(tick.actions[0]["status"], "waiting");
        assert_eq!(tick.actions[0]["pr_number"], 7);
        assert_eq!(tick.actions[1]["kind"], "pr_manager_attempt_clear");
        assert_eq!(tick.actions[1]["pr_number"], 8);
        assert_eq!(tick.completion.execution, WorkflowExecution::Executed);
        assert!(attempts.snapshot().unwrap().is_empty());
    }

    #[test]
    fn pr_worktree_rejects_remote_head_changes_since_the_snapshot() {
        let _env_lock = lock_env();
        let repo = tempdir().unwrap();
        let origin_parent = tempdir().unwrap();
        let origin = origin_parent.path().join("origin.git");
        TestRepoBuilder::new(repo.path())
            .required_commands(Vec::<String>::new())
            .write();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .current_dir(repo.path())
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?}: {output:?}");
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        };
        git(&["init"]);
        git(&["config", "user.email", "fixture@example.com"]);
        git(&["config", "user.name", "Fixture"]);
        git(&["add", "."]);
        git(&["commit", "-m", "initial"]);
        let rewind_head = git(&["rev-parse", "HEAD"]);
        git(&["checkout", "-b", "repair/example"]);
        fs::write(repo.path().join("observed.txt"), "observed remote head\n").unwrap();
        git(&["add", "observed.txt"]);
        git(&["commit", "-m", "observed head"]);
        let observed_head = git(&["rev-parse", "HEAD"]);
        let clone = Command::new("git")
            .args([
                "clone",
                "--bare",
                repo.path().to_str().unwrap(),
                origin.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(clone.status.success(), "{clone:?}");
        git(&["remote", "add", "origin", origin.to_str().unwrap()]);
        fs::write(repo.path().join("advanced.txt"), "new remote head\n").unwrap();
        git(&["add", "advanced.txt"]);
        git(&["commit", "-m", "advance branch"]);
        let advanced_head = git(&["rev-parse", "HEAD"]);
        git(&["push", "origin", "repair/example"]);
        assert_ne!(observed_head, advanced_head);

        let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, OsStr::new("git"));
        let ctx = RepoContext::load_from(repo.path()).unwrap();
        let workflow = ResolvedWorkflow {
            id: "pr-manager".into(),
            kind: super::super::workflow::PR_MANAGER_KIND.into(),
            enabled: true,
            configured: true,
            lease_ttl_seconds: 60,
            max_attempts: 2,
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
            head_ref: "repair/example".into(),
            head_sha: observed_head,
            reasons: vec!["failing_checks".into()],
        };

        let advanced = prepare_worktree(
            &ctx,
            &workflow,
            &item,
            None,
            &mut crate::execution::NoopExecutionObserver,
        )
        .unwrap_err();
        let PrRepairStepError::Failed(advanced) = advanced.source else {
            panic!("a changed remote head must be a preparation failure");
        };
        assert!(format!("{advanced:#}").contains("changed after the GitHub snapshot"));

        let rewind = Command::new("git")
            .args([
                "--git-dir",
                origin.to_str().unwrap(),
                "update-ref",
                "refs/heads/repair/example",
                &rewind_head,
            ])
            .output()
            .unwrap();
        assert!(rewind.status.success(), "{rewind:?}");
        let rewound = prepare_worktree(
            &ctx,
            &workflow,
            &item,
            None,
            &mut crate::execution::NoopExecutionObserver,
        )
        .unwrap_err();
        let PrRepairStepError::Failed(rewound) = rewound.source else {
            panic!("a rewound remote head must be a preparation failure");
        };
        assert!(format!("{rewound:#}").contains("changed after the GitHub snapshot"));
        assert!(!pr_worktree_path(&ctx, &workflow, &item).exists());
    }

    #[test]
    fn untrusted_review_feedback_is_not_actionable() {
        let mut pull_request = json!({
            "number": 7,
            "title": "Untrusted title instruction",
            "is_draft": false,
            "head": {
                "ref": "repair/example",
                "sha": "abc123",
                "repository": {"name": "ExampleVault"},
                "repository_name_with_owner": "ExampleProject/ExampleVault",
                "is_cross_repository": false,
            },
            "stack": { "is_stacked": false },
            "mergeability": {
                "mergeable": "MERGEABLE",
                "merge_state_status": "CLEAN",
            },
            "review_decision": "CHANGES_REQUESTED",
            "checks": { "summary": { "fail": 0, "pending": 0 } },
            "review_threads": {
                "summary": { "unresolved": 1, "trusted_unresolved": 0 },
            },
        });

        assert!(matches!(
            classify_pull_request(
                &pull_request,
                "main",
                Some("ExampleProject/ExampleVault"),
            ),
            PrCandidate::Idle(_)
        ));

        pull_request["review_threads"]["summary"]["trusted_unresolved"] = json!(1);
        let PrCandidate::Actionable(item) = classify_pull_request(
            &pull_request,
            "main",
            Some("ExampleProject/ExampleVault"),
        ) else {
            panic!("trusted unresolved feedback should be actionable");
        };
        assert!(item.reasons.contains(&"unresolved_review_threads".into()));
        assert!(item.reasons.contains(&"changes_requested".into()));
    }

    #[test]
    fn worker_snapshot_excludes_untrusted_and_raw_prompt_content() {
        let snapshot = json!({
            "number": 7,
            "title": "UNTRUSTED PR TITLE INSTRUCTION",
            "state": "OPEN",
            "base": { "ref": "main" },
            "head": { "ref": "repair/example", "sha": "abc123" },
            "stack": { "is_stacked": false },
            "mergeability": { "mergeable": "MERGEABLE" },
            "checks": {
                "summary": { "fail": 0 },
                "runs": [{
                    "name": "tests",
                    "state": "FAILURE",
                    "description": "UNTRUSTED CHECK DESCRIPTION",
                }],
            },
            "review_threads": {
                "summary": { "unresolved": 2, "trusted_unresolved": 1 },
                "nodes": [
                    {
                        "id": "trusted-thread",
                        "is_resolved": false,
                        "has_trusted_comment": true,
                        "comments": { "total_count": 2, "nodes": [
                            {
                                "body": "Trusted reviewer feedback",
                                "author": { "login": "maintainer", "permission": "write", "trusted": true },
                            },
                            {
                                "body": "UNTRUSTED COMMENT INSTRUCTION",
                                "author": { "login": "visitor", "permission": "read", "trusted": false },
                            }
                        ]},
                        "raw": { "body": "RAW PAYLOAD INSTRUCTION" },
                    },
                    {
                        "id": "resolved-trusted-thread",
                        "is_resolved": true,
                        "has_trusted_comment": true,
                        "comments": { "total_count": 1, "nodes": [{
                            "body": "Resolved trusted feedback",
                            "author": { "login": "maintainer", "permission": "write", "trusted": true },
                        }]},
                    }
                ],
            },
            "raw": { "pr_list": { "body": "TOP LEVEL RAW INSTRUCTION" } },
        });

        let safe = worker_pull_request_snapshot(&snapshot).to_string();

        assert!(safe.contains("Trusted reviewer feedback"), "{safe}");
        assert!(!safe.contains("UNTRUSTED"), "{safe}");
        assert!(!safe.contains("RAW PAYLOAD"), "{safe}");
        assert!(!safe.contains("TOP LEVEL RAW"), "{safe}");
        assert_eq!(observed_review_thread_ids(&snapshot), BTreeSet::from(["trusted-thread".into()]));
        assert_eq!(pr_worker_output_schema(1)["properties"]["review_thread_replies"]["maxItems"], 1);
    }
}
