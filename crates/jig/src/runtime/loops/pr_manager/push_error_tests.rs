#[cfg(test)]
mod push_error_tests {
    #[cfg(unix)]
    use std::ffi::{OsStr, OsString};
    #[cfg(unix)]
    use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;

    use tempfile::tempdir;

    use super::*;
    #[cfg(unix)]
    use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

    #[test]
    fn push_evidence_reports_the_expected_head_force_lease() {
        let push = push_result_value(" observed-head\n", "repair-head\n", None);

        assert_eq!(push["force"], true);
        assert_eq!(push["force_with_lease"], true);
        assert_eq!(push["expected_remote_head"], "observed-head");
    }

    #[cfg(unix)]
    #[test]
    fn pr_worker_actions_encode_non_utf8_worktree_paths() {
        let path = PathBuf::from(OsString::from_vec(b"/tmp/pr-worktree-\xff".to_vec()));
        let encoded = pr_worktree_value(&path);
        let item = PrWorkItem {
            pr_number: 7,
            item_key: "pr-7".into(),
            title: "Example repair".into(),
            base_ref: "main".into(),
            head_ref: "repair/example".into(),
            head_sha: "a".repeat(40),
            reasons: vec!["failing_checks".into()],
        };

        let action = pr_worker_action(
            &item,
            &json!({"owner": "example"}),
            None,
            "failed",
            "worker failed",
            Some(&path),
            None,
        );

        assert_eq!(action["worktree"], encoded);
        assert!(encoded.as_str().unwrap().starts_with("jig-path-v1:unix-hex:"));
    }

    #[cfg(unix)]
    #[test]
    fn pr_cleanup_uses_the_native_non_utf8_worktree_path() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        for args in [
            &["init"][..],
            &["config", "user.email", "fixture@example.com"],
            &["config", "user.name", "Fixture"],
            &["add", "."],
            &["commit", "-m", "fixture"],
        ] {
            let output = Command::new("git")
                .current_dir(temp.path())
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?}: {output:?}");
        }
        let worktree = temp
            .path()
            .join(OsString::from_vec(b"repair-worktree-\xff".to_vec()));
        let output = Command::new("git")
            .current_dir(temp.path())
            .args([
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("--detach"),
                worktree.as_os_str(),
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut cleanup = PrWorktreeCleanup::assuming_lease(&ctx);

        let action = finalize_pr_worktree(
            &mut cleanup,
            json!({
                "kind": "pr_manager_worker",
                "status": "attempted",
                "worktree": pr_worktree_value(&worktree),
                "error": null,
            }),
            &worktree,
            false,
        );

        assert_eq!(action["status"], "attempted");
        assert_eq!(action["worktree_retained"], false);
        assert!(!worktree.exists());
    }

    #[cfg(unix)]
    #[test]
    fn git_path_output_preserves_non_utf8_bytes() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let expected = temp
            .path()
            .join(OsString::from_vec(b"common-git-\xff".to_vec()));
        let git = temp.path().join("git-path-stub.sh");
        fs::write(&git, "#!/bin/sh\nprintf '%s\\n' \"$JIG_TEST_COMMON_GIT_DIR\"\n").unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, git.as_os_str());
        let _common = EnvVarGuard::set("JIG_TEST_COMMON_GIT_DIR", expected.as_os_str());
        let ctx = RepoContext::load_from(temp.path()).unwrap();

        let actual = git_stdout_path(
            &ctx,
            temp.path(),
            ["rev-parse", "--git-common-dir"],
            &mut NoopExecutionObserver,
        )
        .unwrap();

        assert_eq!(actual.as_os_str().as_bytes(), expected.as_os_str().as_bytes());
    }

    #[test]
    fn push_execution_error_distinguishes_started_and_unstarted_failures() {
        let started = pr_push_execution_error(
            ExecutionCommandError::Failed {
                error: anyhow!("transport failed"),
                process_started: true,
            },
            "candidate-head",
        );
        let PrPushError::Ambiguous { error, final_head } = started else {
            panic!("a started push must preserve its ambiguous side effect");
        };
        assert_eq!(final_head, "candidate-head");
        assert_eq!(error.to_string(), "transport failed");

        let unstarted = pr_push_execution_error(
            ExecutionCommandError::Failed {
                error: anyhow!("spawn failed"),
                process_started: false,
            },
            "candidate-head",
        );
        let PrPushError::Step(PrRepairStepError::Failed(error)) = unstarted else {
            panic!("an unstarted push must remain an ordinary step failure");
        };
        assert_eq!(error.to_string(), "spawn failed");
    }

    #[test]
    fn commit_and_push_refuses_unresolved_conflict_markers_after_parent_staging() {
        let _env = crate::test_env::lock_env();
        let repo = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(repo.path())
            .required_commands(Vec::<String>::new())
            .write();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
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
        std::fs::write(repo.path().join("conflict.txt"), "base\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "base"]);
        let initial_branch = git(&["symbolic-ref", "--short", "HEAD"]);
        git(&["branch", "other"]);
        std::fs::write(repo.path().join("conflict.txt"), "main\n").unwrap();
        git(&["commit", "-am", "main"]);
        git(&["checkout", "other"]);
        std::fs::write(repo.path().join("conflict.txt"), "other\n").unwrap();
        git(&["commit", "-am", "other"]);
        let head_before = git(&["rev-parse", "HEAD"]);
        let merge = std::process::Command::new("git")
            .current_dir(repo.path())
            .args(["merge", &initial_branch])
            .output()
            .unwrap();
        assert!(!merge.status.success(), "{merge:?}");
        let ctx = RepoContext::load_from(repo.path()).unwrap();

        let error = commit_and_push(
            &ctx,
            repo.path(),
            "other",
            &head_before,
            &head_before,
            &mut NoopExecutionObserver,
        )
        .unwrap_err();

        let PrPushError::Step(PrRepairStepError::Failed(error)) = error else {
            panic!("unresolved conflict markers must be an ordinary pre-push failure");
        };
        assert!(
            format!("{error:#}").contains("conflict marker"),
            "{error:#}"
        );
        assert_eq!(git(&["rev-parse", "HEAD"]), head_before);
        assert!(git(&["ls-files", "--unmerged"]).is_empty());
    }

    #[test]
    fn commit_and_push_refuses_conflict_markers_already_committed_by_the_worker() {
        let _env = crate::test_env::lock_env();
        let repo = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(repo.path())
            .required_commands(Vec::<String>::new())
            .write();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
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
        std::fs::write(repo.path().join("result.txt"), "base\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "base"]);
        let base_head = git(&["rev-parse", "HEAD"]);
        std::fs::write(
            repo.path().join("result.txt"),
            "<<<<<<< ours\nfirst\n=======\nsecond\n>>>>>>> theirs\n",
        )
        .unwrap();
        git(&["commit", "-am", "bad resolution"]);
        let ctx = RepoContext::load_from(repo.path()).unwrap();

        let error = commit_and_push(
            &ctx,
            repo.path(),
            "repair/example",
            &base_head,
            &base_head,
            &mut NoopExecutionObserver,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            PrPushError::Step(PrRepairStepError::Failed(_))
        ));
    }

    #[test]
    fn commit_and_push_checks_only_changes_after_the_pre_worker_merge() {
        let _env = crate::test_env::lock_env();
        let repo = tempdir().unwrap();
        let remote_parent = tempdir().unwrap();
        let remote = remote_parent.path().join("origin.git");
        crate::test_env::TestRepoBuilder::new(repo.path())
            .required_commands(Vec::<String>::new())
            .write();
        let git = |cwd: &Path, args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(cwd)
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?}: {output:?}");
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        };
        git(repo.path(), &["init"]);
        git(
            repo.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        git(repo.path(), &["config", "user.name", "Fixture"]);
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-m", "observed head"]);
        let observed_head = git(repo.path(), &["rev-parse", "HEAD"]);
        git(remote_parent.path(), &["init", "--bare", "origin.git"]);
        git(
            repo.path(),
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(
            repo.path(),
            &["push", "origin", "HEAD:refs/heads/repair/example"],
        );

        std::fs::write(repo.path().join("base.txt"), "pre-existing whitespace \n").unwrap();
        git(repo.path(), &["add", "base.txt"]);
        git(repo.path(), &["commit", "-m", "merge base"]);
        let validation_head = git(repo.path(), &["rev-parse", "HEAD"]);
        std::fs::write(repo.path().join("repair.txt"), "valid repair\n").unwrap();
        git(repo.path(), &["add", "repair.txt"]);
        git(repo.path(), &["commit", "-m", "worker repair"]);
        let ctx = RepoContext::load_from(repo.path()).unwrap();

        let push = commit_and_push(
            &ctx,
            repo.path(),
            "repair/example",
            &observed_head,
            &validation_head,
            &mut NoopExecutionObserver,
        )
        .unwrap();

        assert_eq!(push["pushed"], true);
        assert_eq!(
            git(
                remote_parent.path(),
                &[
                    "--git-dir",
                    "origin.git",
                    "rev-parse",
                    "refs/heads/repair/example",
                ],
            ),
            git(repo.path(), &["rev-parse", "HEAD"])
        );
    }

    #[test]
    fn commit_and_push_rejects_whitespace_introduced_by_the_worker() {
        let _env = crate::test_env::lock_env();
        let repo = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(repo.path())
            .required_commands(Vec::<String>::new())
            .write();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
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
        git(&["commit", "-m", "observed head"]);
        let observed_head = git(&["rev-parse", "HEAD"]);
        std::fs::write(repo.path().join("repair.txt"), "worker whitespace \n").unwrap();
        let ctx = RepoContext::load_from(repo.path()).unwrap();

        let error = commit_and_push(
            &ctx,
            repo.path(),
            "repair/example",
            &observed_head,
            &observed_head,
            &mut NoopExecutionObserver,
        )
        .unwrap_err();

        let PrPushError::Step(PrRepairStepError::Failed(error)) = error else {
            panic!("worker whitespace must be an ordinary pre-push failure");
        };
        let error = format!("{error:#}");
        assert!(error.contains("trailing whitespace"), "{error}");
        assert_eq!(git(&["rev-parse", "HEAD"]), observed_head);
    }

    #[derive(Clone, Copy)]
    enum RemoteMutation {
        Rewind,
        Delete,
    }

    fn assert_expected_head_lease_rejects_remote_mutation(mutation: RemoteMutation) {
        let _env = crate::test_env::lock_env();
        let repo = tempdir().unwrap();
        let remote_parent = tempdir().unwrap();
        let remote = remote_parent.path().join("origin.git");
        crate::test_env::TestRepoBuilder::new(repo.path())
            .required_commands(Vec::<String>::new())
            .write();
        let git = |cwd: &Path, args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(cwd)
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?}: {output:?}");
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        };
        git(repo.path(), &["init"]);
        git(
            repo.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        git(repo.path(), &["config", "user.name", "Fixture"]);
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-m", "base"]);
        let rewind_head = git(repo.path(), &["rev-parse", "HEAD"]);
        std::fs::write(repo.path().join("observed.txt"), "observed\n").unwrap();
        git(repo.path(), &["add", "observed.txt"]);
        git(repo.path(), &["commit", "-m", "observed head"]);
        let observed_head = git(repo.path(), &["rev-parse", "HEAD"]);
        git(remote_parent.path(), &["init", "--bare", "origin.git"]);
        git(
            repo.path(),
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(
            repo.path(),
            &["push", "origin", "HEAD:refs/heads/repair/example"],
        );
        std::fs::write(repo.path().join("repair.txt"), "repair\n").unwrap();
        match mutation {
            RemoteMutation::Rewind => {
                git(
                    remote_parent.path(),
                    &[
                        "--git-dir",
                        "origin.git",
                        "update-ref",
                        "refs/heads/repair/example",
                        &rewind_head,
                    ],
                );
            }
            RemoteMutation::Delete => {
                git(
                    remote_parent.path(),
                    &[
                        "--git-dir",
                        "origin.git",
                        "update-ref",
                        "-d",
                        "refs/heads/repair/example",
                    ],
                );
            }
        }
        let ctx = RepoContext::load_from(repo.path()).unwrap();

        let error = commit_and_push(
            &ctx,
            repo.path(),
            "repair/example",
            &observed_head,
            &observed_head,
            &mut NoopExecutionObserver,
        )
        .unwrap_err();

        assert!(matches!(error, PrPushError::Ambiguous { .. }));
        match mutation {
            RemoteMutation::Rewind => assert_eq!(
                git(
                    remote_parent.path(),
                    &["--git-dir", "origin.git", "rev-parse", "refs/heads/repair/example"]
                ),
                rewind_head
            ),
            RemoteMutation::Delete => {
                let output = std::process::Command::new("git")
                    .current_dir(remote_parent.path())
                    .args([
                        "--git-dir",
                        "origin.git",
                        "show-ref",
                        "--verify",
                        "refs/heads/repair/example",
                    ])
                    .output()
                    .unwrap();
                assert!(!output.status.success(), "deleted branch was recreated: {output:?}");
            }
        }
    }

    #[test]
    fn expected_head_lease_preserves_a_concurrently_rewound_remote() {
        assert_expected_head_lease_rejects_remote_mutation(RemoteMutation::Rewind);
    }

    #[test]
    fn expected_head_lease_preserves_a_concurrently_deleted_remote() {
        assert_expected_head_lease_rejects_remote_mutation(RemoteMutation::Delete);
    }
}
