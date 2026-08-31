#[cfg(test)]
mod push_error_tests {
    use tempfile::tempdir;

    use super::*;

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
    fn commit_and_push_refuses_an_unresolved_merge_index() {
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
            &mut NoopExecutionObserver,
        )
        .unwrap_err();

        let PrPushError::Step(PrRepairStepError::Failed(error)) = error else {
            panic!("unresolved index must be an ordinary pre-push failure");
        };
        assert!(
            error.to_string().contains("unresolved merge entries"),
            "{error:#}"
        );
        assert_eq!(git(&["rev-parse", "HEAD"]), head_before);
        assert!(!git(&["ls-files", "--unmerged"]).is_empty());
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
            &mut NoopExecutionObserver,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            PrPushError::Step(PrRepairStepError::Failed(_))
        ));
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
