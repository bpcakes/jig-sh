mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn summary_surfaces_pr_list_truncation() {
        let pull_requests = (0..PR_LIST_LIMIT)
            .map(|number| {
                json!({
                    "number": number,
                    "mergeability": { "mergeable": "MERGEABLE" },
                    "checks": { "summary": { "fail": 0, "pending": 0 } },
                    "review_threads": { "summary": { "unresolved": 0 } },
                    "stack": { "is_stacked": false },
                })
            })
            .collect::<Vec<_>>();

        let summary = summary_for_pull_requests(&pull_requests, PR_LIST_LIMIT, true);

        assert_eq!(summary["open_pr_count"], PR_LIST_LIMIT);
        assert_eq!(summary["pr_list_limit"], PR_LIST_LIMIT);
        assert_eq!(summary["pr_list_truncated"], true);
    }

    #[test]
    fn only_effective_write_permissions_are_trusted_for_worker_input() {
        assert!(permission_is_trusted("admin"));
        assert!(permission_is_trusted("write"));
        assert!(!permission_is_trusted("maintain"));
        assert!(!permission_is_trusted("triage"));
        assert!(!permission_is_trusted("read"));
        assert!(!permission_is_trusted("none"));
        assert_eq!(encode_path_segment("example[bot]"), "example%5Bbot%5D");
    }

    #[test]
    fn head_repository_identity_uses_the_fields_emitted_by_gh() {
        let raw = json!({
            "headRepository": {"id": "repo-1", "name": "ExampleVault"},
            "headRepositoryOwner": {"login": "ExampleProject"},
        });

        assert_eq!(
            head_repository_name_with_owner(&raw).as_deref(),
            Some("ExampleProject/ExampleVault")
        );
    }

    #[test]
    fn head_repository_identity_rejects_a_composite_only_fallback() {
        let raw = json!({
            "headRepository": {
                "nameWithOwner": "ExampleProject/ExampleVault",
            },
        });

        assert_eq!(head_repository_name_with_owner(&raw), None);
    }

    #[cfg(unix)]
    #[test]
    fn gh_commands_scrub_repository_redirects_but_keep_authentication() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt as _;

        use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

        let _env = lock_env();
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .config("")
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let gh = temp.path().join("fixture-gh-environment");
        fs::write(
            &gh,
            r#"#!/bin/sh
set -eu
[ -z "${GIT_DIR+x}" ]
[ -z "${GIT_WORK_TREE+x}" ]
[ -z "${GH_REPO+x}" ]
[ "$GH_TOKEN" = "fixture-token" ]
printf '%s\n' '{}'
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&gh).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&gh, permissions).unwrap();
        let _git_dir = EnvVarGuard::set("GIT_DIR", OsStr::new("/redirected/git"));
        let _git_work_tree = EnvVarGuard::set("GIT_WORK_TREE", OsStr::new("/redirected/tree"));
        let _gh_repo = EnvVarGuard::set("GH_REPO", OsStr::new("OtherProject/OtherVault"));
        let _gh_token = EnvVarGuard::set("GH_TOKEN", OsStr::new("fixture-token"));

        let output = run_gh_with_program(
            &ctx,
            Vec::new(),
            gh.as_os_str(),
            &mut crate::execution::NoopExecutionObserver,
        )
        .unwrap();

        assert_eq!(output.status_code, Some(0));
    }

    #[test]
    fn review_thread_graphql_uses_raw_string_variables() {
        let repository = RepositorySnapshot {
            owner: "8451".into(),
            name: "2048".into(),
            default_branch: "main".into(),
            value: json!({}),
        };
        let args = review_thread_page_args(&repository, 7, Some("123456"))
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        for value in ["owner=8451", "name=2048", "threadsAfter=123456"] {
            assert!(
                args.windows(2)
                    .any(|pair| pair == ["-f", value]),
                "{value} must remain a GraphQL string: {args:?}"
            );
            assert!(!args.windows(2).any(|pair| pair == ["-F", value]));
        }
        assert!(args.windows(2).any(|pair| pair == ["-F", "number=7"]));
    }

    #[cfg(unix)]
    #[test]
    fn review_comment_history_pages_back_to_older_trusted_feedback() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt as _;

        use crate::test_env::{EnvVarGuard, lock_env};

        let _env = lock_env();
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .config("")
            .required_commands(Vec::<String>::new())
            .write();
        let gh = temp.path().join("fixture-gh");
        fs::write(
            &gh,
            r#"#!/bin/sh
case "$*" in
  *"threadId=thread-1"*)
    printf '%s\n' '{"data":{"node":{"id":"thread-1","comments":{"totalCount":2,"pageInfo":{"hasPreviousPage":false,"startCursor":null},"nodes":[{"id":"comment-1","body":"trusted original","author":{"login":"maintainer"}}]}}}}'
    ;;
  *"api graphql "*)
    printf '%s\n' '{"data":{"repository":{"pullRequest":{"reviewThreads":{"totalCount":1,"nodes":[{"id":"thread-1","isResolved":false,"comments":{"totalCount":2,"pageInfo":{"hasPreviousPage":true,"startCursor":"older"},"nodes":[{"id":"comment-2","body":"untrusted reply","author":{"login":"visitor"}}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}'
    ;;
  *"collaborators/maintainer/permission"*) printf '%s\n' '{"permission":"write"}' ;;
  *"collaborators/visitor/permission"*) printf '%s\n' '{"permission":"read"}' ;;
  *) exit 2 ;;
esac
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&gh).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&gh, permissions).unwrap();
        let _gh = EnvVarGuard::set("JIG_GH_BIN", gh.as_os_str());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let repository = RepositorySnapshot {
            owner: "ExampleProject".into(),
            name: "ExampleVault".into(),
            default_branch: "main".into(),
            value: json!({}),
        };
        let mut observer = crate::execution::NoopExecutionObserver;
        let mut client = GithubSnapshotClient::new(&ctx, &mut observer);

        let snapshot = review_threads_snapshot(
            &mut client,
            &repository,
            7,
            &mut RepositoryPermissionCache::default(),
        )
        .unwrap();

        let comments = &snapshot["nodes"][0]["comments"];
        assert_eq!(comments["truncated"], false, "{snapshot:#}");
        assert_eq!(comments["page_count"], 2);
        assert_eq!(comments["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(comments["nodes"][0]["id"], "comment-1");
        assert_eq!(snapshot["nodes"][0]["has_trusted_comment"], true);
        assert_eq!(snapshot["summary"]["trusted_unresolved"], 1);
    }

    #[cfg(unix)]
    #[test]
    fn incomplete_review_comment_history_marks_the_snapshot_truncated() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt as _;

        use crate::test_env::{EnvVarGuard, lock_env};

        let _env = lock_env();
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .config("")
            .required_commands(Vec::<String>::new())
            .write();
        let gh = temp.path().join("fixture-gh");
        fs::write(
            &gh,
            r#"#!/bin/sh
case "$*" in
  *"api graphql "*) printf '%s\n' '{"data":{"repository":{"pullRequest":{"reviewThreads":{"totalCount":1,"nodes":[{"id":"thread-1","isResolved":false,"comments":{"totalCount":11,"nodes":[{"id":"comment-11","body":"untrusted reply","author":{"login":"visitor"}}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}' ;;
  *) exit 2 ;;
esac
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&gh).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&gh, permissions).unwrap();
        let _gh = EnvVarGuard::set("JIG_GH_BIN", gh.as_os_str());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let repository = RepositorySnapshot {
            owner: "ExampleProject".into(),
            name: "ExampleVault".into(),
            default_branch: "main".into(),
            value: json!({}),
        };
        let mut observer = crate::execution::NoopExecutionObserver;
        let mut client = GithubSnapshotClient::new(&ctx, &mut observer);

        let snapshot = review_threads_snapshot(
            &mut client,
            &repository,
            7,
            &mut RepositoryPermissionCache::default(),
        )
        .unwrap();

        assert_eq!(snapshot["page_info"]["truncated"], true);
        assert_eq!(snapshot["nodes"][0]["comments"]["truncated"], true);
        assert_eq!(snapshot["summary"]["trusted_unresolved"], 0);
        assert_eq!(
            client.budget_snapshot()["request_count"],
            1,
            "truncated comment history must not trigger permission lookups"
        );
    }

    #[test]
    fn snapshot_budget_bounds_composed_github_work() {
        let timeout = CommandTimeout::from_seconds(60).unwrap();
        let mut request_budget = GithubSnapshotBudget::new(timeout);
        for _ in 0..GITHUB_SNAPSHOT_REQUEST_LIMIT {
            request_budget.reserve_request().unwrap();
        }
        assert!(
            request_budget
                .reserve_request()
                .unwrap_err()
                .to_string()
                .contains("request budget")
        );

        let mut byte_budget = GithubSnapshotBudget::new(timeout);
        byte_budget.response_bytes = GITHUB_SNAPSHOT_RESPONSE_BYTE_LIMIT - 1;
        let one_byte = GhOutput {
            status_code: Some(0),
            stdout: "a".into(),
            stderr: String::new(),
        };
        byte_budget.record_response(&one_byte).unwrap();
        assert_eq!(
            byte_budget.response_bytes,
            GITHUB_SNAPSHOT_RESPONSE_BYTE_LIMIT
        );
        assert!(
            byte_budget
                .record_response(&one_byte)
                .unwrap_err()
                .to_string()
                .contains("response budget")
        );

        let mut review_budget = GithubSnapshotBudget::new(timeout);
        review_budget.review_item_count = GITHUB_SNAPSHOT_REVIEW_ITEM_LIMIT - 1;
        review_budget.reserve_review_items(1).unwrap();
        assert_eq!(
            review_budget.review_item_count,
            GITHUB_SNAPSHOT_REVIEW_ITEM_LIMIT
        );
        assert!(
            review_budget
                .reserve_review_items(1)
                .unwrap_err()
                .to_string()
                .contains("review budget")
        );

        let mut expired_budget = GithubSnapshotBudget::new(timeout);
        expired_budget.started_at = Instant::now() - expired_budget.timeout;
        assert!(
            expired_budget
                .reserve_request()
                .unwrap_err()
                .to_string()
                .contains("deadline")
        );
    }

    #[cfg(unix)]
    #[test]
    fn full_pr_page_without_review_fanout_fits_the_snapshot_budget() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt as _;

        use crate::test_env::{EnvVarGuard, lock_env};

        let _env = lock_env();
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .config("")
            .required_commands(Vec::<String>::new())
            .write();
        let pull_requests = (1..=PR_LIST_LIMIT)
            .map(|number| {
                json!({
                    "number": number,
                    "title": format!("Example PR {number}"),
                    "state": "OPEN",
                    "isDraft": false,
                    "baseRefName": "main",
                    "headRefName": format!("repair/example-{number}"),
                    "headRefOid": format!("{number:040x}"),
                    "isCrossRepository": false,
                    "mergeable": "MERGEABLE",
                    "mergeStateStatus": "CLEAN",
                })
            })
            .collect::<Vec<_>>();
        let pull_requests = serde_json::to_string(&pull_requests).unwrap();
        let log = temp.path().join("gh-calls.log");
        let gh = temp.path().join("fixture-gh");
        fs::write(
            &gh,
            r#"#!/bin/sh
printf 'CALL\n' >> "$JIG_TEST_GH_LOG"
case "$*" in
  *"repo view"*)
    printf '%s\n' '{"nameWithOwner":"ExampleProject/ExampleVault","name":"ExampleVault","owner":{"login":"ExampleProject"},"url":"https://example.invalid/ExampleProject/ExampleVault","defaultBranchRef":{"name":"main"}}'
    ;;
  *"pr list"*) printf '%s\n' "$JIG_TEST_PR_LIST" ;;
  *"pr checks"*) printf '%s\n' '[]' ;;
  *"api graphql"*)
    printf '%s\n' '{"data":{"repository":{"pullRequest":{"reviewThreads":{"totalCount":0,"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}'
    ;;
  *) exit 2 ;;
esac
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&gh).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&gh, permissions).unwrap();
        let _gh = EnvVarGuard::set("JIG_GH_BIN", gh.as_os_str());
        let _pr_list = EnvVarGuard::set("JIG_TEST_PR_LIST", OsStr::new(&pull_requests));
        let _log = EnvVarGuard::set("JIG_TEST_GH_LOG", log.as_os_str());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut observer = crate::execution::NoopExecutionObserver;

        let snapshot = github_pr_status_snapshot(&ctx, &mut observer).unwrap();

        assert_eq!(snapshot["pull_requests"].as_array().unwrap().len(), PR_LIST_LIMIT);
        assert_eq!(snapshot["summary"]["pr_list_truncated"], false);
        assert_eq!(snapshot["budget"]["request_count"], 2 + 2 * PR_LIST_LIMIT);
        assert_eq!(snapshot["budget"]["review_item_count"], 0);
        assert_eq!(
            fs::read_to_string(log).unwrap().lines().count(),
            2 + 2 * PR_LIST_LIMIT
        );
    }

    #[cfg(unix)]
    #[test]
    fn missing_review_thread_cursor_stops_pagination_without_duplicates() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt as _;

        use crate::test_env::{EnvVarGuard, lock_env};

        let _env = lock_env();
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .config("")
            .required_commands(Vec::<String>::new())
            .write();
        let gh = temp.path().join("fixture-gh");
        fs::write(
            &gh,
            r#"#!/bin/sh
printf '%s\n' '{"data":{"repository":{"pullRequest":{"reviewThreads":{"totalCount":1,"nodes":[{"id":"thread-1","isResolved":false,"comments":{"totalCount":0,"nodes":[]}}],"pageInfo":{"hasNextPage":true,"endCursor":null}}}}}}'
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&gh).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&gh, permissions).unwrap();
        let _gh = EnvVarGuard::set("JIG_GH_BIN", gh.as_os_str());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let repository = RepositorySnapshot {
            owner: "ExampleProject".into(),
            name: "ExampleVault".into(),
            default_branch: "main".into(),
            value: json!({}),
        };
        let mut observer = crate::execution::NoopExecutionObserver;
        let mut client = GithubSnapshotClient::new(&ctx, &mut observer);

        let snapshot = review_threads_snapshot(
            &mut client,
            &repository,
            7,
            &mut RepositoryPermissionCache::default(),
        )
        .unwrap();

        assert_eq!(snapshot["page_info"]["page_count"], 1);
        assert_eq!(snapshot["page_info"]["truncated"], true);
        assert_eq!(snapshot["page_info"]["has_next_page"], true);
        assert_eq!(snapshot["nodes"].as_array().unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn changing_review_thread_total_marks_the_snapshot_incomplete() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt as _;

        use crate::test_env::{EnvVarGuard, lock_env};

        let _env = lock_env();
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .config("")
            .required_commands(Vec::<String>::new())
            .write();
        let gh = temp.path().join("fixture-gh");
        fs::write(
            &gh,
            r#"#!/bin/sh
case "$*" in
  *"threadsAfter=cursor-1"*)
    printf '%s\n' '{"data":{"repository":{"pullRequest":{"reviewThreads":{"totalCount":2,"nodes":[{"id":"thread-2","isResolved":false,"comments":{"totalCount":0,"nodes":[]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}'
    ;;
  *)
    printf '%s\n' '{"data":{"repository":{"pullRequest":{"reviewThreads":{"totalCount":1,"nodes":[{"id":"thread-1","isResolved":false,"comments":{"totalCount":0,"nodes":[]}}],"pageInfo":{"hasNextPage":true,"endCursor":"cursor-1"}}}}}}'
    ;;
esac
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&gh).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&gh, permissions).unwrap();
        let _gh = EnvVarGuard::set("JIG_GH_BIN", gh.as_os_str());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let repository = RepositorySnapshot {
            owner: "ExampleProject".into(),
            name: "ExampleVault".into(),
            default_branch: "main".into(),
            value: json!({}),
        };
        let mut observer = crate::execution::NoopExecutionObserver;
        let mut client = GithubSnapshotClient::new(&ctx, &mut observer);

        let snapshot = review_threads_snapshot(
            &mut client,
            &repository,
            7,
            &mut RepositoryPermissionCache::default(),
        )
        .unwrap();

        assert_eq!(snapshot["page_info"]["page_count"], 2);
        assert_eq!(snapshot["page_info"]["truncated"], true);
        assert_eq!(snapshot["nodes"].as_array().unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn collaborator_permission_lookup_fails_closed_for_non_writers_and_missing_actors() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt as _;

        use crate::test_env::{EnvVarGuard, lock_env};

        let _env = lock_env();
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .config("")
            .required_commands(Vec::<String>::new())
            .write();
        let gh = temp.path().join("fixture-gh");
        fs::write(
            &gh,
            r#"#!/bin/sh
case "$*" in
  *maintainer*) printf '%s\n' '{"permission":"write"}' ;;
  *visitor*) printf '%s\n' '{"permission":"read"}' ;;
  *) printf '%s\n' 'gh: Not Found (HTTP 404)' >&2; exit 1 ;;
esac
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&gh).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&gh, permissions).unwrap();
        let _gh = EnvVarGuard::set("JIG_GH_BIN", gh.as_os_str());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let repository = RepositorySnapshot {
            owner: "ExampleProject".into(),
            name: "ExampleVault".into(),
            default_branch: "main".into(),
            value: json!({}),
        };
        let mut cache = RepositoryPermissionCache::default();
        let mut observer = crate::execution::NoopExecutionObserver;
        let mut client = GithubSnapshotClient::new(&ctx, &mut observer);

        let maintainer = cache
            .author_snapshot(&mut client, &repository, Some("maintainer"))
            .unwrap();
        let visitor = cache
            .author_snapshot(&mut client, &repository, Some("visitor"))
            .unwrap();
        let missing = cache
            .author_snapshot(&mut client, &repository, Some("example[bot]"))
            .unwrap();

        assert_eq!(maintainer["trusted"], true);
        assert_eq!(visitor["trusted"], false);
        assert_eq!(missing["trusted"], false);
        assert!(missing["permission"].is_null());
    }

    #[cfg(unix)]
    #[test]
    fn gh_execution_honors_in_flight_cancellation() {
        struct CancelAfterStart(PathBuf);

        impl crate::execution::ExecutionObserver for CancelAfterStart {}

        impl crate::execution::ExecutionCancellation for CancelAfterStart {
            fn cancelled(&self) -> bool {
                self.0.exists()
            }
        }

        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .config("")
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let marker = temp.path().join("gh-started");
        let mut observer = CancelAfterStart(marker.clone());
        let started = Instant::now();

        let error = run_gh_with_program(
            &ctx,
            vec![
                OsString::from("-c"),
                OsString::from("printf started > \"$1\"; sleep 30"),
                OsString::from("fixture-shell"),
                marker.into_os_string(),
            ],
            OsStr::new("sh"),
            &mut observer,
        )
        .err()
        .expect("cancelled gh command should fail")
        .to_string();

        assert!(error.contains("cancelled"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
