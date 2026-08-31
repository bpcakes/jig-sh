#[cfg(test)]
mod review_thread_budget_tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn review_thread_updates_share_one_aggregate_request_budget() {
        let timeout = CommandTimeout::from_seconds(60).unwrap();
        let mut budget = ReviewThreadUpdateBudget::new(timeout);

        for _ in 0..REVIEW_THREAD_UPDATE_REQUEST_LIMIT {
            budget.reserve_request(timeout).unwrap();
        }
        let error = budget.reserve_request(timeout).unwrap_err().to_string();

        assert!(error.contains("request budget"), "{error}");
    }

    #[test]
    fn cancellation_before_mutation_skips_remote_reconciliation() {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut reply_budget = ReviewThreadUpdateBudget::new(ctx.command_timeout());
        let mut resolve_budget = ReviewThreadUpdateBudget::new(ctx.command_timeout());

        assert!(matches!(
            reconcile_reply_mutation(
                &ctx,
                "PRRT_1",
                "marker",
                Err(ExecutionCommandError::CancelledBeforeStart),
                &mut reply_budget,
            ),
            Err(ExecutionCommandError::CancelledBeforeStart)
        ));
        assert!(matches!(
            reconcile_resolve_mutation(
                &ctx,
                "PRRT_1",
                Err(ExecutionCommandError::CancelledBeforeStart),
                &mut resolve_budget,
            ),
            Err(ExecutionCommandError::CancelledBeforeStart)
        ));
    }
}
