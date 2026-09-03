#[cfg(test)]
mod cancellation_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tempfile::tempdir;

    use super::*;

    struct CancelledControl;

    impl crate::execution::ExecutionObserver for CancelledControl {}

    impl crate::execution::ExecutionCancellation for CancelledControl {
        fn cancelled(&self) -> bool {
            true
        }
    }

    struct CancelAfterStart(AtomicUsize);

    impl crate::execution::ExecutionObserver for CancelAfterStart {}

    impl crate::execution::ExecutionCancellation for CancelAfterStart {
        fn cancelled(&self) -> bool {
            self.0.fetch_add(1, Ordering::SeqCst) > 0
        }
    }

    struct CancelWhenPresent(PathBuf);

    impl crate::execution::ExecutionObserver for CancelWhenPresent {}

    impl crate::execution::ExecutionCancellation for CancelWhenPresent {
        fn cancelled(&self) -> bool {
            self.0.exists()
        }
    }

    #[test]
    fn cancelled_repair_does_not_consume_attempt_budget() {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let workflow = ResolvedWorkflow {
            id: "pr-manager".into(),
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
            title: "Example repair".into(),
            base_ref: "main".into(),
            head_ref: "repair/example".into(),
            head_sha: "a".repeat(40),
            reasons: vec!["failing_checks".into()],
        };
        let mut attempt_store = AttemptStore::new(&ctx);
        let lease = json!({"owner": "test"});
        let repair = PrRepairContext {
            repo: &ctx,
            workflow: &workflow,
            item: &item,
            lease: &lease,
            codex_home: None,
            worktree_reservation: None,
        };

        let mut observer = CancelledControl;
        let action_result = run_pr_repair(&repair, &json!({}), &mut observer);
        let PrRepairOutcome::Cancelled { detail, worktree } = &action_result else {
            panic!("pre-start Git cancellation must cancel the repair");
        };
        assert!(detail.contains("git fetch was cancelled before it started"));
        assert!(worktree.is_none());
        let action = record_pr_repair_outcome_under_branch_lease(
            &repair,
            &mut attempt_store,
            action_result,
        )
        .unwrap();
        let completion = pr_manager_completion(std::slice::from_ref(&action));
        assert_eq!(action["status"], "failed");
        assert_eq!(action["unexecuted_reason"], "cancelled_before_start");
        assert!(
            action["error"].as_str().unwrap().contains("git fetch was cancelled")
        );
        assert!(matches!(
            completion.execution,
            WorkflowExecution::Unexecuted(UnexecutedReason::CancelledBeforeStart)
        ));
        assert!(attempt_store.snapshot().unwrap().is_empty());
    }

    #[test]
    fn post_commit_cancellation_records_the_pushed_head() {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
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
        let mut item = PrWorkItem {
            pr_number: 7,
            item_key: "pr-7".into(),
            title: "Example repair".into(),
            base_ref: "main".into(),
            head_ref: "repair/example".into(),
            head_sha: "observed-head".into(),
            reasons: vec!["failing_checks".into()],
        };
        let mut attempt_store = AttemptStore::new(&ctx);
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
            &mut attempt_store,
            PrRepairOutcome::Completed {
                action: json!({
                    "kind": "pr_manager_worker",
                    "status": "cancelled_after_commit",
                    "push": {"final_head": "pushed-head"},
                }),
                worktree: temp.path().join("repair-worktree"),
            },
        )
        .unwrap();

        assert_eq!(action["status"], "cancelled_after_commit");
        assert_eq!(action["attempt"]["item_version"], "pushed-head");
        assert_eq!(
            action["attempt"]["observed_item_version"],
            "observed-head"
        );
        assert_eq!(action["attempt"]["last_status"], "attempted");
        let attempts = attempt_store.snapshot().unwrap();
        assert_eq!(attempts[0].item_version.as_deref(), Some("pushed-head"));
        assert_eq!(
            attempts[0].observed_item_version.as_deref(),
            Some("observed-head")
        );
        assert!(
            !attempt_version_is_stale(&attempts[0], &item),
            "a lagging snapshot must not reset the attempt budget"
        );
        item.head_sha = "pushed-head".into();
        assert!(!attempt_version_is_stale(&attempts[0], &item));
        item.head_sha = "new-contributor-head".into();
        assert!(attempt_version_is_stale(&attempts[0], &item));
    }

    #[test]
    fn post_commit_cancellation_message_formats_the_head_as_text() {
        assert_eq!(
            post_commit_cancellation_error("pushed-head"),
            "PR manager repair was cancelled after pushing pushed-head; follow-up review thread updates are incomplete"
        );
    }

    #[test]
    fn completed_pr_action_keeps_attempt_evidence_when_lease_cleanup_fails() {
        let action = json!({
            "status": "attempted",
            "push": { "final_head": "abc123" },
            "attempt": { "attempts": 1, "last_status": "attempted" },
        });
        let error = anyhow!("injected lease renewal failure");

        let action = with_branch_lease_result(action, Some(&error));

        assert_eq!(action["status"], "needs_attention");
        assert_eq!(
            action["attention_kind"],
            "branch_lease_lost_after_start"
        );
        assert_eq!(action["completed_status"], "attempted");
        assert_eq!(action["push"]["final_head"], "abc123");
        assert_eq!(action["attempt"]["attempts"], 1);
        assert_eq!(action["attempt"]["last_status"], "attempted");
        assert_eq!(action["lease_error"], "injected lease renewal failure");
        let completion = super::super::workflow::WorkflowCompletion::from_actions(&[action]);
        assert_eq!(
            completion.outcome,
            super::super::workflow::WorkflowOutcome::NeedsAttention
        );
    }

    #[test]
    fn lease_cleanup_failure_preserves_post_commit_attention_diagnostic() {
        let action = json!({
            "status": "cancelled_after_commit",
            "worker_receipt_id": "receipt-worker",
            "error": "follow-up review thread updates are incomplete",
        });
        let error = anyhow!("injected lease renewal failure");

        let action = with_branch_lease_result(action, Some(&error));

        assert_eq!(action["status"], "needs_attention");
        assert_eq!(action["completed_status"], "cancelled_after_commit");
        assert_eq!(
            action["completed_error"],
            "follow-up review thread updates are incomplete"
        );
        assert!(
            action["error"]
                .as_str()
                .is_some_and(|error| error.contains("follow-up review thread updates are incomplete")),
            "{action:#}"
        );
        let completion =
            super::super::workflow::WorkflowCompletion::from_actions(std::slice::from_ref(&action));
        assert_eq!(
            completion.outcome,
            super::super::workflow::WorkflowOutcome::NeedsAttention
        );
        assert_eq!(
            completion.worker_receipt_id.as_deref(),
            Some("receipt-worker")
        );
    }

    #[test]
    fn review_mutations_require_the_expected_success_payload() {
        let reply_error = validate_reply_mutation_response(json!({"data": {}})).unwrap_err();
        assert!(reply_error.to_string().contains("invalid payload"));

        let resolve_error = validate_resolve_mutation_response(json!({
            "data": {"resolveReviewThread": {"thread": {"isResolved": false}}}
        }))
        .unwrap_err();
        assert!(resolve_error.to_string().contains("did not report"));

        let state_error = validate_review_thread_reply_state(
            json!({
                "data": {
                    "node": {
                        "id": "PRRT_1",
                        "comments": {"nodes": []}
                    }
                }
            }),
            "PRRT_1",
        )
        .unwrap_err();
        assert!(state_error.to_string().contains("invalid payload"));

        let resolution = validate_review_thread_resolution_state(
            json!({
                "data": {
                    "node": {
                        "id": "PRRT_1",
                        "isResolved": false,
                        "pullRequest": {"headRefOid": "pushed-head"},
                        "comments": {"totalCount": 0, "nodes": []}
                    }
                }
            }),
            "PRRT_1",
        )
        .unwrap();
        assert_eq!(resolution["data"]["node"]["isResolved"], false);
    }

    #[cfg(unix)]
    #[test]
    fn cancelled_review_reply_reconciles_the_remote_comment() {
        use std::os::unix::fs::PermissionsExt;

        use crate::test_env::{EnvVarGuard, lock_env};

        let _env_lock = lock_env();
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let gh = temp.path().join("gh-reply-reconciliation");
        fs::write(
            &gh,
            r#"#!/bin/sh
set -eu
case "$*" in
  *ReviewThreadWitnessState*)
    printf '%s\n' '{"data":{"node":{"id":"PRRT_1","isResolved":false,"pullRequest":{"headRefOid":"pushed-head"},"comments":{"totalCount":0,"pageInfo":{"hasPreviousPage":false,"startCursor":null},"nodes":[]}}}}'
    ;;
  *ReviewThreadState*)
    if [ -f remote-reply ]; then
      marker=$(tail -n 1 remote-marker)
      printf '%s\n' "{\"data\":{\"node\":{\"id\":\"PRRT_1\",\"comments\":{\"pageInfo\":{\"hasPreviousPage\":false,\"startCursor\":\"cursor-1\"},\"nodes\":[{\"id\":\"PRRC_REMOTE\",\"url\":\"https://example.invalid/reply\",\"body\":\"$marker\",\"viewerDidAuthor\":true}]}}}}"
    else
      cat <<'JSON'
{"data":{"node":{"id":"PRRT_1","comments":{"pageInfo":{"hasPreviousPage":false,"startCursor":null},"nodes":[]}}}}
JSON
    fi
    ;;
  *addPullRequestReviewThreadReply*)
    body_file=''
    for arg in "$@"; do
      case "$arg" in
        body=@*) body_file=${arg#body=@} ;;
      esac
    done
    test -n "$body_file"
    tail -n 1 "$body_file" > remote-marker
    printf 'mutation\n' >> mutation.log
    : > remote-reply
    : > mutation-started
    sleep 60
    ;;
  *)
    echo "unexpected gh arguments: $*" >&2
    exit 2
    ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
        let _gh = EnvVarGuard::set("JIG_GH_BIN", gh.as_os_str());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut observer = CancelWhenPresent(temp.path().join("mutation-started"));
        let witness = ReviewThreadWitness::default();
        let mut budget = ReviewThreadUpdateBudget::new(ctx.command_timeout(), 1);

        let response = post_review_thread_reply(
            &ctx,
            "PRRT_1",
            "Addressed in the pushed repair.",
            "pushed-head",
            &witness,
            &mut observer,
            &mut budget,
        )
        .unwrap();
        let ReviewThreadReply::Posted(response) = response else {
            panic!("unchanged review thread should receive the reconciled reply");
        };

        assert_eq!(response["_jig"]["reconciled"], true);
        assert_eq!(
            response["data"]["addPullRequestReviewThreadReply"]["comment"]["id"],
            "PRRC_REMOTE"
        );
        assert_eq!(
            fs::read_to_string(temp.path().join("mutation.log")).unwrap(),
            "mutation\n"
        );
    }

    #[test]
    fn review_thread_reply_search_stops_on_the_newest_matching_page() {
        let marker = "<!-- jig-pr-manager:review-reply:PRRT_1:pushed-head -->";
        let mut calls = Vec::new();

        let comment = fetch_review_thread_reply_comment_with_markers("PRRT_1", &[marker], |cursor| {
            calls.push(cursor.map(str::to_string));
            Ok(json!({
                "data": {"node": {
                    "id": "PRRT_1",
                    "comments": {
                        "pageInfo": {
                            "hasPreviousPage": true,
                            "startCursor": "cursor-newest",
                        },
                        "nodes": [{
                            "id": "PRRC_NEWEST",
                            "url": "https://example.invalid/newest",
                            "body": marker,
                            "viewerDidAuthor": true,
                        }],
                    }
                }}
            }))
        })
        .unwrap()
        .unwrap();

        assert_eq!(calls, [None]);
        assert_eq!(comment["id"], "PRRC_NEWEST");
    }

    #[test]
    fn review_thread_reply_search_pages_backward_until_it_finds_the_marker() {
        let marker = "<!-- jig-pr-manager:review-reply:PRRT_1:pushed-head -->";
        let mut calls = Vec::new();

        let comment = fetch_review_thread_reply_comment_with_markers("PRRT_1", &[marker], |cursor| {
            calls.push(cursor.map(str::to_string));
            Ok(if cursor.is_none() {
                json!({
                    "data": {"node": {
                        "id": "PRRT_1",
                        "comments": {
                            "pageInfo": {
                                "hasPreviousPage": true,
                                "startCursor": "cursor-100",
                            },
                            "nodes": [{
                                "id": "PRRC_NEWER",
                                "url": "https://example.invalid/newer",
                                "body": "newer comment",
                                "viewerDidAuthor": false,
                            }],
                        }
                    }}
                })
            } else {
                json!({
                    "data": {"node": {
                        "id": "PRRT_1",
                        "comments": {
                            "pageInfo": {
                                "hasPreviousPage": false,
                                "startCursor": "cursor-oldest",
                            },
                            "nodes": [{
                                "id": "PRRC_MATCH",
                                "url": "https://example.invalid/match",
                                "body": marker,
                                "viewerDidAuthor": true,
                            }],
                        }
                    }}
                })
            })
        })
        .unwrap()
        .unwrap();

        assert_eq!(calls, [None, Some("cursor-100".into())]);
        assert_eq!(comment["id"], "PRRC_MATCH");
    }

    #[test]
    fn review_thread_reply_search_rejects_missing_backward_cursor() {
        let error = fetch_review_thread_reply_comment_with_markers("PRRT_1", &["marker"], |_| {
            Ok(json!({
                "data": {"node": {
                    "id": "PRRT_1",
                    "comments": {
                        "pageInfo": {"hasPreviousPage": true, "startCursor": null},
                        "nodes": [],
                    }
                }}
            }))
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("without a start cursor"), "{error}");
    }

    #[test]
    fn review_thread_reply_search_enforces_the_page_safety_limit() {
        let mut calls = 0;
        let error = fetch_review_thread_reply_comment_with_markers("PRRT_1", &["marker"], |_| {
            calls += 1;
            Ok(json!({
                "data": {"node": {
                    "id": "PRRT_1",
                    "comments": {
                        "pageInfo": {
                            "hasPreviousPage": true,
                            "startCursor": format!("cursor-{calls}"),
                        },
                        "nodes": [],
                    }
                }}
            }))
        })
        .unwrap_err()
        .to_string();

        assert_eq!(calls, REVIEW_THREAD_COMMENT_PAGE_LIMIT);
        assert!(error.contains("page safety limit"), "{error}");
    }

    #[test]
    fn review_thread_reply_search_stops_when_total_deadline_expires() {
        let total_timeout = Duration::from_secs(60);
        let live_deadline = Instant::now() + total_timeout;
        let mut requests = 0;

        let error = fetch_review_thread_reply_comment_with_markers("PRRT_1", &["marker"], |_| {
            let deadline = if requests == 0 {
                live_deadline
            } else {
                Instant::now()
            };
            let _timeout = remaining_operation_timeout(
                deadline,
                total_timeout,
                "GitHub review thread reply lookup",
            )?;
            requests += 1;
            Ok(json!({
                "data": {"node": {
                    "id": "PRRT_1",
                    "comments": {
                        "pageInfo": {
                            "hasPreviousPage": true,
                            "startCursor": format!("cursor-{requests}"),
                        },
                        "nodes": [],
                    }
                }}
            }))
        })
        .unwrap_err()
        .to_string();

        assert_eq!(requests, 1, "no second GitHub request should start");
        assert!(error.contains("reply lookup exceeded its 60 second total timeout"));
    }

    #[test]
    fn review_thread_cursor_is_always_passed_as_a_raw_string() {
        let args = review_thread_reply_state_args("PRRT_1", Some("123"));
        let args = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(
            args.windows(2)
                .any(|pair| pair == ["-f", "commentsBefore=123"]),
            "{args:?}"
        );
        assert!(!args.iter().any(|arg| arg == "-F"), "{args:?}");
    }

    #[test]
    fn mutation_reconciliation_uses_one_total_deadline() {
        let timeout = remaining_reconciliation_timeout(
            Instant::now() + Duration::from_millis(1_500),
        )
        .unwrap();
        assert!(timeout.as_secs() <= 2);

        let expired = remaining_reconciliation_timeout(Instant::now())
            .unwrap_err()
            .to_string();
        assert!(expired.contains("total timeout"), "{expired}");
    }

    #[cfg(unix)]
    #[test]
    fn cancelled_review_resolution_reconciles_the_remote_thread() {
        use std::os::unix::fs::PermissionsExt;

        use crate::test_env::{EnvVarGuard, lock_env};

        let _env_lock = lock_env();
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let gh = temp.path().join("gh-resolve-reconciliation");
        fs::write(
            &gh,
            r#"#!/bin/sh
set -eu
case "$*" in
  *ReviewThreadWitnessState*)
    printf '%s\n' '{"data":{"node":{"id":"PRRT_1","isResolved":false,"pullRequest":{"headRefOid":"pushed-head"},"comments":{"totalCount":0,"pageInfo":{"hasPreviousPage":false,"startCursor":null},"nodes":[]}}}}'
    ;;
  *ReviewThreadState*)
    if [ -f remote-resolved ]; then resolved=true; else resolved=false; fi
    printf '{"data":{"viewer":{"login":"jig-bot"},"node":{"id":"PRRT_1","isResolved":%s,"pullRequest":{"headRefOid":"pushed-head"},"comments":{"totalCount":0,"nodes":[]}}}}\n' "$resolved"
    ;;
  *resolveReviewThread*)
    printf 'mutation\n' >> mutation.log
    : > remote-resolved
    : > mutation-started
    sleep 60
    ;;
  *)
    echo "unexpected gh arguments: $*" >&2
    exit 2
    ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
        let _gh = EnvVarGuard::set("JIG_GH_BIN", gh.as_os_str());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut observer = CancelWhenPresent(temp.path().join("mutation-started"));
        let mut budget = ReviewThreadUpdateBudget::new(ctx.command_timeout(), 1);

        let response = resolve_review_thread(
            &ctx,
            "PRRT_1",
            &ReviewThreadWitness::default(),
            None,
            "pushed-head",
            &mut observer,
            &mut budget,
        )
        .unwrap();
        let ReviewThreadResolution::Resolved(response) = response else {
            panic!("unchanged review thread should resolve");
        };

        assert_eq!(response["_jig"]["reconciled"], true);
        assert_eq!(
            response["data"]["resolveReviewThread"]["thread"]["isResolved"],
            true
        );
        assert_eq!(
            fs::read_to_string(temp.path().join("mutation.log")).unwrap(),
            "mutation\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn in_flight_git_cancellation_remains_typed() {
        use std::os::unix::fs::PermissionsExt;

        use crate::test_env::{EnvVarGuard, lock_env};

        let _env_lock = lock_env();
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let git = temp.path().join("slow-git");
        fs::write(&git, "#!/bin/sh\nsleep 60\n").unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
        let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, git.as_os_str());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut observer = CancelAfterStart(AtomicUsize::new(0));

        let error = git_output(&ctx, temp.path(), ["fetch"], &mut observer).unwrap_err();

        let PrRepairStepError::Cancelled(detail) = error else {
            panic!("in-flight Git cancellation must remain typed");
        };
        assert!(detail.contains("git fetch was cancelled while it was running"));
    }

    #[cfg(unix)]
    #[test]
    fn cancelled_push_is_reconciled_when_the_remote_received_the_commit() {
        use std::os::unix::fs::PermissionsExt;

        use crate::test_env::{EnvVarGuard, lock_env};

        fn checked_git(cwd: &Path, args: &[&str]) -> String {
            let output = Command::new("git")
                .current_dir(cwd)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }

        struct MarkerCancellation(PathBuf);

        impl crate::execution::ExecutionObserver for MarkerCancellation {}

        impl crate::execution::ExecutionCancellation for MarkerCancellation {
            fn cancelled(&self) -> bool {
                self.0.exists()
            }
        }

        let _env_lock = lock_env();
        let temp = tempdir().unwrap();
        let remote = temp.path().join("remote.git");
        let worktree = temp.path().join("worktree");
        fs::create_dir(&remote).unwrap();
        fs::create_dir(&worktree).unwrap();
        checked_git(&remote, &["init", "--bare"]);
        checked_git(&worktree, &["init"]);
        checked_git(&worktree, &["config", "user.name", "Example User"]);
        checked_git(
            &worktree,
            &["config", "user.email", "example@example.invalid"],
        );
        fs::write(worktree.join("example.txt"), "before\n").unwrap();
        checked_git(&worktree, &["add", "example.txt"]);
        checked_git(&worktree, &["commit", "-m", "initial"]);
        checked_git(&worktree, &["branch", "-M", "repair/example"]);
        checked_git(
            &worktree,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        checked_git(&worktree, &["push", "-u", "origin", "repair/example"]);
        let base_head = checked_git(&worktree, &["rev-parse", "HEAD"]);
        fs::write(worktree.join("example.txt"), "after\n").unwrap();

        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let marker = temp.path().join("push-completed");
        let wrapper = temp.path().join("git-wrapper");
        fs::write(
            &wrapper,
            "#!/bin/sh\n\
             if [ \"$2\" = push ]; then\n\
               git \"$@\"\n\
               status=$?\n\
               if [ \"$status\" -eq 0 ]; then\n\
                 : > \"$JIG_TEST_CANCEL_MARKER\"\n\
                 sleep 60\n\
               fi\n\
               exit \"$status\"\n\
             fi\n\
             exec git \"$@\"\n",
        )
        .unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
        let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, wrapper.as_os_str());
        let _marker = EnvVarGuard::set("JIG_TEST_CANCEL_MARKER", marker.as_os_str());
        let mut observer = MarkerCancellation(marker);

        let push = commit_and_push(
            &ctx,
            &worktree,
            "repair/example",
            &base_head,
            None,
            &base_head,
            &mut observer,
        )
        .unwrap();

        let final_head = checked_git(&worktree, &["rev-parse", "HEAD"]);
        let remote_head = checked_git(
            temp.path(),
            &[
                "--git-dir",
                remote.to_str().unwrap(),
                "rev-parse",
                "refs/heads/repair/example",
            ],
        );
        assert_eq!(remote_head, final_head);
        assert_eq!(push["pushed"], true);
        assert!(
            push["reconciliation"]
                .as_str()
                .unwrap()
                .contains("confirmed at")
        );
    }
}
