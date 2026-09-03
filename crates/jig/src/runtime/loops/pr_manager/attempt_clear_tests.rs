#[cfg(test)]
mod attempt_clear_tests {
    use tempfile::tempdir;

    use super::*;
    use crate::test_env::TestRepoBuilder;

    #[test]
    fn lagging_healthy_head_does_not_clear_the_resulting_heads_attempt() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
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
        let mut attempts = AttemptStore::new(&ctx);
        attempts
            .record_attempt_for_transition(
                &workflow,
                "pr-8",
                Some("observed-head"),
                Some("resulting-head"),
                "failed",
            )
            .unwrap();
        let lagging = PrIdleItem {
            pr_number: 8,
            item_key: "pr-8".into(),
            head_ref: "repair/healthy".into(),
            head_sha: "observed-head".into(),
        };

        let action = clear_observed_healthy_attempt(
            &workflow,
            &mut attempts,
            &lagging,
            &|| false,
        )
        .unwrap();

        assert!(action.is_none());
        assert_eq!(
            attempts
                .get(&workflow.id, &lagging.item_key)
                .unwrap()
                .unwrap()
                .item_version
                .as_deref(),
            Some("resulting-head")
        );

        let current = PrIdleItem {
            head_sha: "resulting-head".into(),
            ..lagging
        };
        let action = clear_observed_healthy_attempt(
            &workflow,
            &mut attempts,
            &current,
            &|| false,
        )
        .unwrap();

        assert_eq!(action.unwrap()["reason"], "observed_healthy");
        assert!(attempts.snapshot().unwrap().is_empty());
    }
}
