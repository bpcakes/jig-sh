#[cfg(all(test, unix))]
mod review_thread_boundary_tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use tempfile::tempdir;

    use super::*;
    use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

    #[test]
    fn reply_intents_cannot_exceed_observed_actionable_threads() {
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
            }]},
        });
        let worker_output = json!({
            "review_thread_replies": [
                {"thread_id": "PRRT_1", "body": "first"},
                {"thread_id": "PRRT_1", "body": "duplicate"},
            ],
        });

        let result = post_review_thread_updates(
            &ctx,
            &pull_request,
            &worker_output,
            "example-head",
            &mut NoopExecutionObserver,
        );

        assert!(result.failed);
        assert_eq!(result.posts[0]["reason"], "review_thread_reply_limit_exceeded");
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
                {"id": "PRRT_1", "is_resolved": false, "has_trusted_comment": true},
                {"id": "PRRT_2", "is_resolved": false, "has_trusted_comment": true},
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
        assert_eq!(fs::read_to_string(calls).unwrap().lines().count(), 2);
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
        let mut budget = ReviewThreadUpdateBudget::new(ctx.command_timeout());

        let response = post_review_thread_reply(
            &ctx,
            "PRRT_1",
            &body,
            "example-head",
            &mut NoopExecutionObserver,
            &mut budget,
        )
        .unwrap();

        assert_eq!(
            response["data"]["addPullRequestReviewThreadReply"]["comment"]["id"],
            "PRRC_1"
        );
        let captured = fs::read_to_string(temp.path().join("captured-reply")).unwrap();
        assert!(captured.starts_with("review reply begins\n"));
        assert!(captured.contains("review reply ends"));
        assert!(captured.ends_with(
            "<!-- jig-pr-manager:review-reply:PRRT_1:example-head -->"
        ));
        assert!(captured.len() > 256 * 1_024);
    }
}
