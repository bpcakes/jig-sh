#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_env::{EnvVarGuard, lock_env};

    #[test]
    fn git_command_uses_configured_program_and_scrubs_repository_redirects() {
        let _env_lock = lock_env();
        let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, "custom-git");
        let _git_dir = EnvVarGuard::set("GIT_DIR", "/tmp/redirected.git");
        let _git_trace = EnvVarGuard::set("GIT_TRACE", "1");
        let _git_ssh = EnvVarGuard::set("GIT_SSH_COMMAND", "ssh -i test-key");

        let command = git_command(Path::new("/tmp/repository"), ["status", "--short"]);

        assert_eq!(command.get_program(), OsStr::new("custom-git"));
        assert_eq!(
            command.get_current_dir(),
            Some(Path::new("/tmp/repository"))
        );
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["--no-replace-objects", "status", "--short"]
                .map(OsStr::new)
                .as_slice()
        );
        assert!(
            command
                .get_envs()
                .any(|(name, value)| { name == OsStr::new("GIT_DIR") && value.is_none() })
        );
        assert!(
            command
                .get_envs()
                .any(|(name, value)| { name == OsStr::new("GIT_TRACE") && value.is_none() })
        );
        assert!(
            !command
                .get_envs()
                .any(|(name, _)| { name == OsStr::new("GIT_SSH_COMMAND") })
        );
    }

    #[test]
    fn classify_uses_observed_default_branch_when_base_ref_is_missing() {
        let pull_request = json!({
            "number": 7,
            "title": "Fix widgets",
            "is_draft": false,
            "head": {
                "ref": "codex/widgets",
                "sha": "abc123",
                "is_cross_repository": false,
            },
            "stack": {
                "is_stacked": false,
            },
            "mergeability": {
                "mergeable": "MERGEABLE",
                "merge_state_status": "CLEAN",
            },
            "review_decision": "REVIEW_REQUIRED",
            "checks": {
                "summary": {
                    "fail": 1,
                },
            },
            "review_threads": {
                "summary": {
                    "unresolved": 0,
                },
            },
        });

        match classify_pull_request(&pull_request, "trunk") {
            PrCandidate::Actionable(item) => assert_eq!(item.base_ref, "trunk"),
            PrCandidate::Skip(_) | PrCandidate::Idle(_) | PrCandidate::Pending(_) => {
                panic!("expected actionable PR candidate")
            }
        }
    }

    #[test]
    fn classify_pending_checks_are_not_observed_healthy() {
        let pull_request = json!({
            "number": 7,
            "title": "Fix widgets",
            "is_draft": false,
            "head": {
                "ref": "codex/widgets",
                "sha": "abc123",
                "is_cross_repository": false,
            },
            "stack": {
                "is_stacked": false,
            },
            "mergeability": {
                "mergeable": "MERGEABLE",
                "merge_state_status": "CLEAN",
            },
            "review_decision": "REVIEW_REQUIRED",
            "checks": {
                "summary": {
                    "fail": 0,
                    "pending": 1,
                },
            },
            "review_threads": {
                "summary": {
                    "unresolved": 0,
                },
            },
        });

        match classify_pull_request(&pull_request, "main") {
            PrCandidate::Pending(item) => {
                assert_eq!(item.item_key, "pr-7");
                assert_eq!(item.pending_checks, 1);
            }
            PrCandidate::Actionable(_) | PrCandidate::Skip(_) | PrCandidate::Idle(_) => {
                panic!("expected pending PR candidate")
            }
        }
    }
}
