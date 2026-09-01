#[cfg(all(test, unix))]
mod review_thread_boundary_tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use tempfile::tempdir;

    use super::*;
    use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

    #[test]
    fn missing_thread_ids_are_rejected_before_deduplication() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let worker_output = json!({
            "review_thread_replies": [
                {"thread_id": "", "body": "reply"},
                {"thread_id": "  ", "body": "reply"},
            ],
        });

        let result = post_review_thread_updates(
            &ctx,
            &json!({}),
            &worker_output,
            "example-head",
            &mut NoopExecutionObserver,
        );

        assert!(!result.failed);
        assert_eq!(result.posts.as_array().unwrap().len(), 2);
        assert!(
            result.posts.as_array().unwrap().iter().all(|post| {
                post["status"] == "skipped" && post["reason"] == "missing_review_thread"
            }),
            "{}",
            result.posts
        );
    }

    #[test]
    fn duplicate_intents_are_skipped_with_one_observed_thread() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let pull_request = json!({
            "review_threads": { "nodes": [{
                "id": "PRRT_1",
                "is_resolved": false,
                "has_trusted_comment": true,
                "comments": {"total_count": 0, "nodes": []},
            }]},
        });
        let worker_output = json!({
            "review_thread_replies": [
                {"thread_id": "PRRT_1", "body": ""},
                {"thread_id": "PRRT_1", "body": ""},
            ],
        });

        let result = post_review_thread_updates(
            &ctx,
            &pull_request,
            &worker_output,
            "example-head",
            &mut NoopExecutionObserver,
        );

        assert!(!result.failed);
        assert_eq!(result.posts.as_array().unwrap().len(), 2);
        assert_eq!(result.posts[1]["reason"], "duplicate_review_thread");
        assert_eq!(result.posts[1]["reply_comment_id"], Value::Null);
        assert_eq!(result.posts[1]["reply_reconciled"], false);
        assert_eq!(result.posts[1]["reply_skipped"], false);
        assert_eq!(result.posts[1]["reply_skip_reason"], Value::Null);
        assert_eq!(result.posts[1]["is_resolved"], Value::Null);
        assert_eq!(result.posts[1]["resolve_reconciled"], false);
        assert_eq!(result.posts[1]["resolve_skipped"], false);
    }

    #[test]
    fn duplicate_reply_intents_are_collapsed_before_network_calls() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let calls = temp.path().join("gh-calls");
        let gh = temp.path().join("gh-duplicate-stub.sh");
        fs::write(
            &gh,
            r#"#!/bin/sh
set -eu
printf 'call\n' >> "$JIG_TEST_GH_CALLS"
case "$*" in
  *ReviewThreadWitnessState*)
    printf '%s\n' '{"data":{"node":{"id":"PRRT_1","isResolved":false,"comments":{"totalCount":0,"pageInfo":{"hasPreviousPage":false,"startCursor":null},"nodes":[]}}}}'
    ;;
  *ReviewThreadState*)
    printf '%s\n' '{"data":{"node":{"id":"PRRT_1","comments":{"pageInfo":{"hasPreviousPage":false,"startCursor":null},"nodes":[]}}}}'
    ;;
  *addPullRequestReviewThreadReply*)
    printf '%s\n' '{"data":{"addPullRequestReviewThreadReply":{"comment":{"id":"PRRC_1","url":"https://example.invalid/reply"}}}}'
    ;;
  *) exit 2 ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
        let _gh = EnvVarGuard::set("JIG_GH_BIN", gh.as_os_str());
        let _calls = EnvVarGuard::set("JIG_TEST_GH_CALLS", calls.as_os_str());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let pull_request = json!({
            "review_threads": { "nodes": [
                {"id": "PRRT_1", "is_resolved": false, "has_trusted_comment": true, "comments": {"total_count": 0, "nodes": []}},
                {"id": "PRRT_2", "is_resolved": false, "has_trusted_comment": true, "comments": {"total_count": 0, "nodes": []}},
            ]},
        });
        let worker_output = json!({
            "review_thread_replies": [
                {"thread_id": "PRRT_1", "body": "fixed", "resolve": false},
                {"thread_id": "PRRT_1", "body": "duplicate", "resolve": false},
            ],
        });

        let result = post_review_thread_updates(
            &ctx,
            &pull_request,
            &worker_output,
            "example-head",
            &mut NoopExecutionObserver,
        );

        assert!(!result.failed);
        assert_eq!(result.posts.as_array().unwrap().len(), 2);
        assert_eq!(result.posts[1]["reason"], "duplicate_review_thread");
        assert_eq!(fs::read_to_string(calls).unwrap().lines().count(), 3);
    }

    #[test]
    fn large_review_reply_body_is_read_from_a_file() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let gh = temp.path().join("gh-stub.sh");
        fs::write(
            &gh,
            r#"#!/bin/sh
set -eu
case "$*" in
  *ReviewThreadWitnessState*)
    printf '%s\n' '{"data":{"node":{"id":"PRRT_1","isResolved":false,"comments":{"totalCount":0,"pageInfo":{"hasPreviousPage":false,"startCursor":null},"nodes":[]}}}}'
    ;;
  *ReviewThreadState*)
    cat <<'JSON'
{"data":{"node":{"id":"PRRT_1","comments":{"pageInfo":{"hasPreviousPage":false,"startCursor":null},"nodes":[]}}}}
JSON
    ;;
  *addPullRequestReviewThreadReply*)
    body_file=''
    for arg in "$@"; do
      case "$arg" in
        body=@*) body_file=${arg#body=@} ;;
      esac
    done
    test -n "$body_file"
    cp "$body_file" captured-reply
    cat <<'JSON'
{"data":{"addPullRequestReviewThreadReply":{"comment":{"id":"PRRC_1","url":"https://example.invalid/reply"}}}}
JSON
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
        let body = format!("review reply begins\n{}\nreview reply ends", "x".repeat(256 * 1_024));
        let witness = ReviewThreadWitness::default();
        let marker = review_thread_reply_marker("PRRT_1", "example-head", &witness, &body);
        let mut budget = ReviewThreadUpdateBudget::new(ctx.command_timeout());

        let response = post_review_thread_reply(
            &ctx,
            "PRRT_1",
            &body,
            "example-head",
            &witness,
            &mut NoopExecutionObserver,
            &mut budget,
        )
        .unwrap();
        let ReviewThreadReply::Posted(response) = response else {
            panic!("unchanged review thread should receive a reply");
        };

        assert_eq!(
            response["data"]["addPullRequestReviewThreadReply"]["comment"]["id"],
            "PRRC_1"
        );
        let captured = fs::read_to_string(temp.path().join("captured-reply")).unwrap();
        assert!(captured.starts_with("review reply begins\n"));
        assert!(captured.contains("review reply ends"));
        assert!(captured.ends_with(&marker));
        assert!(captured.len() > 256 * 1_024);
    }

    #[test]
    fn reply_marker_binds_feedback_generation_and_response_intent() {
        let first = json!({
            "comments": {"nodes": [{
                "id": "COMMENT_1",
                "updatedAt": "2026-09-01T10:00:00Z",
                "body": "Please add a regression test",
                "author": {"trusted": true},
            }]},
        });
        let later = json!({
            "comments": {"nodes": [
                {
                    "id": "COMMENT_1",
                    "updatedAt": "2026-09-01T10:00:00Z",
                    "body": "Please add a regression test",
                    "author": {"trusted": true},
                },
                {
                    "id": "COMMENT_2",
                    "updatedAt": "2026-09-01T11:00:00Z",
                    "body": "Please cover the cancellation path too",
                    "author": {"trusted": true},
                },
            ]},
        });
        let first = ReviewThreadWitness {
            reply_generation: review_reply_generation(&first),
            ..ReviewThreadWitness::default()
        };
        let later = ReviewThreadWitness {
            reply_generation: review_reply_generation(&later),
            ..ReviewThreadWitness::default()
        };

        let original = review_thread_reply_marker("PRRT_1", "same-head", &first, "Done.");
        assert_eq!(
            original,
            review_thread_reply_marker("PRRT_1", "same-head", &first, "Done.")
        );
        assert_ne!(
            original,
            review_thread_reply_marker("PRRT_1", "same-head", &later, "Done.")
        );
        assert_ne!(
            original,
            review_thread_reply_marker("PRRT_1", "same-head", &first, "Added more coverage.")
        );
    }

    #[test]
    fn reply_reconciliation_requires_githubs_viewer_authorship_fact() {
        let marker = review_thread_reply_marker(
            "PRRT_1",
            "pushed-head",
            &ReviewThreadWitness::default(),
            "Addressed.",
        );
        let spoofed = json!({
            "data": {"node": {"comments": {"nodes": [{
                "id": "PRRC_SPOOFED",
                "url": "https://example.invalid/spoofed",
                "body": marker,
                "viewerDidAuthor": false,
            }]}}}
        });
        let owned = json!({
            "data": {"node": {"comments": {"nodes": [{
                "id": "PRRC_OWNED",
                "url": "https://example.invalid/owned",
                "body": marker,
                "viewerDidAuthor": true,
            }]}}}
        });

        assert!(review_thread_comment_with_marker(&spoofed, &marker).is_none());
        assert_eq!(
            review_thread_comment_with_marker(&owned, &marker)
                .and_then(|comment| comment["id"].as_str()),
            Some("PRRC_OWNED")
        );
    }

    #[test]
    fn resolution_witness_rejects_added_or_edited_feedback() {
        let observed = json!({"review_threads": {"nodes": [{
            "id": "PRRT_1",
            "is_resolved": false,
            "has_trusted_comment": true,
            "comments": {"total_count": 1, "nodes": [{
                "id": "PRRC_ORIGINAL",
                "updatedAt": "2026-09-01T10:00:00Z",
                "body": "Please add a regression test",
                "author": {"trusted": true},
            }]},
        }]}});
        let witness = observed_review_thread_witnesses(&observed)
            .remove("PRRT_1")
            .unwrap();
        let changed = LiveReviewThreadState {
            is_resolved: false,
            total_count: 2,
            comments: vec![
                observed.pointer("/review_threads/nodes/0/comments/nodes/0").unwrap().clone(),
                json!({"id": "PRRC_NEW_FEEDBACK", "updatedAt": "2026-09-01T11:00:00Z", "body": "Also cover cancellation"}),
            ],
        };

        assert!(!review_thread_matches_witness(&changed, &witness, None));
        let edited = LiveReviewThreadState {
            is_resolved: false,
            total_count: 1,
            comments: vec![json!({
                "id": "PRRC_ORIGINAL",
                "updatedAt": "2026-09-01T11:00:00Z",
                "body": "Please cover cancellation instead",
            })],
        };
        assert!(!review_thread_matches_witness(&edited, &witness, None));
        assert!(review_thread_matches_witness(
            &LiveReviewThreadState {
                is_resolved: false,
                total_count: 2,
                comments: vec![
                    observed.pointer("/review_threads/nodes/0/comments/nodes/0").unwrap().clone(),
                    json!({"id": "PRRC_JIG_REPLY", "updatedAt": "2026-09-01T11:00:00Z", "body": "Addressed"}),
                ],
            },
            &witness,
            Some("PRRC_JIG_REPLY"),
        ));
    }

    #[test]
    fn changed_review_thread_is_skipped_before_resolution_mutation() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let calls = temp.path().join("gh-calls");
        let gh = temp.path().join("gh-changed-thread.sh");
        fs::write(
            &gh,
            r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$JIG_TEST_GH_CALLS"
case "$*" in
  *"query=mutation"*) exit 9 ;;
  *"ReviewThreadWitnessState"*)
    printf '%s\n' '{"data":{"node":{"id":"PRRT_1","isResolved":false,"comments":{"totalCount":1,"pageInfo":{"hasPreviousPage":false,"startCursor":null},"nodes":[{"id":"PRRC_ORIGINAL","updatedAt":"2026-09-01T11:00:00Z","body":"edited feedback"}]}}}}'
    ;;
  *) exit 2 ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
        let _gh = EnvVarGuard::set("JIG_GH_BIN", gh.as_os_str());
        let _calls = EnvVarGuard::set("JIG_TEST_GH_CALLS", calls.as_os_str());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let pull_request = json!({
            "review_threads": {"nodes": [{
                "id": "PRRT_1",
                "is_resolved": false,
                "has_trusted_comment": true,
                "comments": {
                    "total_count": 1,
                    "nodes": [{
                        "id": "PRRC_ORIGINAL",
                        "updatedAt": "2026-09-01T10:00:00Z",
                        "body": "original feedback",
                    }],
                },
            }]},
        });
        let worker_output = json!({
            "review_thread_replies": [{
                "thread_id": "PRRT_1",
                "body": "",
                "resolve": true,
            }],
        });

        let result = post_review_thread_updates(
            &ctx,
            &pull_request,
            &worker_output,
            "example-head",
            &mut NoopExecutionObserver,
        );

        assert!(!result.failed, "{}", result.posts);
        assert_eq!(result.posts[0]["status"], "skipped");
        assert_eq!(result.posts[0]["reason"], "review_thread_changed");
        assert_eq!(result.posts[0]["resolved"], false);
        assert_eq!(result.posts[0]["resolve_skipped"], true);
        assert_eq!(
            result.posts[0]["resolve_skip_reason"],
            "review_thread_changed"
        );
        let calls = fs::read_to_string(calls).unwrap();
        assert!(calls.contains("ReviewThreadWitnessState"), "{calls}");
        assert!(!calls.contains("resolveReviewThread(input"), "{calls}");
    }

    #[test]
    fn changed_review_thread_is_skipped_before_reply_mutation() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let calls = temp.path().join("gh-calls");
        let gh = temp.path().join("gh-stale-reply.sh");
        fs::write(
            &gh,
            r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$JIG_TEST_GH_CALLS"
case "$*" in
  *"query=mutation"*) exit 9 ;;
  *"ReviewThreadWitnessState"*)
    printf '%s\n' '{"data":{"node":{"id":"PRRT_1","isResolved":false,"comments":{"totalCount":1,"pageInfo":{"hasPreviousPage":false,"startCursor":null},"nodes":[{"id":"PRRC_ORIGINAL","updatedAt":"2026-09-01T11:00:00Z","body":"edited feedback"}]}}}}'
    ;;
  *"ReviewThreadState"*)
    printf '%s\n' '{"data":{"node":{"id":"PRRT_1","comments":{"pageInfo":{"hasPreviousPage":false,"startCursor":null},"nodes":[]}}}}'
    ;;
  *) exit 2 ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
        let _gh = EnvVarGuard::set("JIG_GH_BIN", gh.as_os_str());
        let _calls = EnvVarGuard::set("JIG_TEST_GH_CALLS", calls.as_os_str());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let pull_request = json!({
            "review_threads": {"nodes": [{
                "id": "PRRT_1",
                "is_resolved": false,
                "has_trusted_comment": true,
                "comments": {
                    "total_count": 1,
                    "nodes": [{
                        "id": "PRRC_ORIGINAL",
                        "updatedAt": "2026-09-01T10:00:00Z",
                        "body": "original feedback",
                    }],
                },
            }]},
        });
        let worker_output = json!({
            "review_thread_replies": [{
                "thread_id": "PRRT_1",
                "body": "Addressed.",
                "resolve": true,
            }],
        });

        let result = post_review_thread_updates(
            &ctx,
            &pull_request,
            &worker_output,
            "example-head",
            &mut NoopExecutionObserver,
        );

        assert!(!result.failed, "{}", result.posts);
        assert_eq!(result.posts[0]["status"], "skipped");
        assert_eq!(result.posts[0]["reply_skipped"], true);
        assert_eq!(
            result.posts[0]["reply_skip_reason"],
            "review_thread_changed"
        );
        assert_eq!(result.posts[0]["resolve_skipped"], true);
        let calls = fs::read_to_string(calls).unwrap();
        assert!(calls.contains("ReviewThreadWitnessState"), "{calls}");
        assert!(!calls.contains("addPullRequestReviewThreadReply"), "{calls}");
        assert!(!calls.contains("resolveReviewThread(input"), "{calls}");
    }
}
