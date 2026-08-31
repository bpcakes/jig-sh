#[cfg(all(test, unix))]
mod preparation_tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    use tempfile::tempdir;

    use super::*;
    use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

    struct CancelWhenPresent(PathBuf);

    impl crate::execution::ExecutionObserver for CancelWhenPresent {}

    impl crate::execution::ExecutionCancellation for CancelWhenPresent {
        fn cancelled(&self) -> bool {
            self.0.exists()
        }
    }

    #[test]
    fn cancellation_after_worktree_add_cleans_registration_before_returning() {
        let _guard = lock_env();
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
        git(&["commit", "-m", "fixture"]);
        git(&["branch", "repair/example"]);
        let head_sha = git(&["rev-parse", "repair/example"]);
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

        let real_git = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap();
        assert!(real_git.status.success(), "{real_git:?}");
        let real_git = String::from_utf8(real_git.stdout).unwrap();
        let wrapper = repo.path().join("git-wrapper.sh");
        fs::write(
            &wrapper,
            r#"#!/bin/sh
"$JIG_TEST_REAL_GIT" "$@"
status=$?
if [ "$status" -eq 0 ]; then
  case "$*" in
    *"worktree add"*) touch "$JIG_TEST_CANCEL_MARKER" ;;
  esac
fi
exit "$status"
"#,
        )
        .unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
        let marker = repo.path().join("cancel-after-worktree-add");
        let _real_git = EnvVarGuard::set("JIG_TEST_REAL_GIT", OsStr::new(real_git.trim()));
        let _marker = EnvVarGuard::set("JIG_TEST_CANCEL_MARKER", marker.as_os_str());
        let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, wrapper.as_os_str());
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
            title: "Example repair".into(),
            base_ref: "main".into(),
            head_ref: "repair/example".into(),
            head_sha,
            reasons: vec!["failing_checks".into()],
        };
        let lease = json!({"owner": "fixture-owner"});
        let repair = PrRepairContext {
            repo: &ctx,
            workflow: &workflow,
            item: &item,
            lease: &lease,
            codex_home: None,
        };
        let expected_worktree = pr_worktree_path(&ctx, &workflow, &item);
        let mut observer = CancelWhenPresent(marker);

        let outcome = run_pr_repair(&repair, &json!({}), &mut observer);

        let PrRepairOutcome::Cancelled { worktree, .. } = outcome else {
            panic!("post-add cancellation should remain an unexecuted cancellation");
        };
        assert!(worktree.is_none());
        assert!(!expected_worktree.exists());
        let listing = Command::new(real_git.trim())
            .current_dir(repo.path())
            .args(["worktree", "list", "--porcelain"])
            .output()
            .unwrap();
        assert!(listing.status.success(), "{listing:?}");
        assert!(
            !String::from_utf8_lossy(&listing.stdout)
                .lines()
                .any(|line| line == format!("worktree {}", expected_worktree.display()))
        );
    }

    #[test]
    fn failed_preparation_cleanup_is_retained_without_a_post_release_retry() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let output = Command::new("git")
            .current_dir(temp.path())
            .args(["init"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
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
        let item = PrWorkItem {
            pr_number: 7,
            item_key: "pr-7".into(),
            title: "Example repair".into(),
            base_ref: "main".into(),
            head_ref: "repair/example".into(),
            head_sha: "a".repeat(40),
            reasons: vec!["failing_checks".into()],
        };
        let worktree = pr_worktree_path(&ctx, &workflow, &item);
        fs::create_dir_all(&worktree).unwrap();
        fs::write(worktree.join("partial-evidence"), "retain me\n").unwrap();
        let failure = cleanup_failed_worktree_preparation(
            &ctx,
            &worktree,
            PrRepairStepError::Cancelled("injected preparation cancellation".into()),
            false,
        );
        let lease = json!({"owner": "fixture-owner"});
        let repair = PrRepairContext {
            repo: &ctx,
            workflow: &workflow,
            item: &item,
            lease: &lease,
            codex_home: None,
        };
        let action = preparation_cleanup_attention(
            &repair,
            pr_step_error(failure.source),
            failure.cleanup_error.unwrap(),
            failure.retained_worktree.unwrap(),
            UnexecutedReason::CancelledBeforeStart,
            None,
        );

        assert_eq!(action["status"], "needs_attention");
        assert_eq!(action["attention_kind"], "worktree_cleanup_failed");
        assert_eq!(action["unexecuted_reason"], "cancelled_before_start");
        assert_eq!(action["worktree_retained"], true);
        assert!(worktree.join("partial-evidence").is_file());
    }
}
