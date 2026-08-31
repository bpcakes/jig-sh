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
}
