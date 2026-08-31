#[cfg(all(test, unix))]
mod review_thread_boundary_tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use tempfile::tempdir;

    use super::*;
    use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

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

        let response = post_review_thread_reply(
            &ctx,
            "PRRT_1",
            &body,
            "example-head",
            &mut NoopExecutionObserver,
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
