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

        let maintainer = cache
            .author_snapshot(
                &ctx,
                &repository,
                Some("maintainer"),
                &mut crate::execution::NoopExecutionObserver,
            )
            .unwrap();
        let visitor = cache
            .author_snapshot(
                &ctx,
                &repository,
                Some("visitor"),
                &mut crate::execution::NoopExecutionObserver,
            )
            .unwrap();
        let missing = cache
            .author_snapshot(
                &ctx,
                &repository,
                Some("example[bot]"),
                &mut crate::execution::NoopExecutionObserver,
            )
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
