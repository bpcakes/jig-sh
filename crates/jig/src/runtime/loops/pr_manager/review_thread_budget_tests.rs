#[cfg(test)]
mod review_thread_budget_tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn one_review_thread_intent_has_one_bounded_request_budget() {
        let command_timeout = CommandTimeout::from_seconds(60).unwrap();
        let timeout = command_timeout.duration();
        let mut budget = ReviewThreadUpdateBudget::new(command_timeout, 1);

        for _ in 0..REVIEW_THREAD_PRIMARY_REQUESTS_PER_INTENT {
            budget.reserve_request(timeout).unwrap();
        }
        let error = budget.reserve_request(timeout).unwrap_err().to_string();

        assert!(error.contains("primary-operation budget"), "{error}");
    }

    #[test]
    fn aggregate_request_budget_scales_with_unique_actionable_intents() {
        let command_timeout = CommandTimeout::from_seconds(60).unwrap();
        let timeout = command_timeout.duration();
        let mut budget = ReviewThreadUpdateBudget::new(command_timeout, 2);

        for _ in 0..2 {
            budget.begin_intent(command_timeout);
            for _ in 0..REVIEW_THREAD_PRIMARY_REQUESTS_PER_INTENT {
                budget.reserve_request(timeout).unwrap();
            }
        }
        budget.begin_intent(command_timeout);
        let error = budget.reserve_request(timeout).unwrap_err().to_string();

        assert!(error.contains("604-request primary-operation budget"), "{error}");
    }

    #[test]
    fn one_saturated_intent_does_not_consume_the_next_intents_slice() {
        let command_timeout = CommandTimeout::from_seconds(60).unwrap();
        let timeout = command_timeout.duration();
        let mut budget = ReviewThreadUpdateBudget::new(command_timeout, 2);

        for _ in 0..REVIEW_THREAD_PRIMARY_REQUESTS_PER_INTENT {
            budget.reserve_request(timeout).unwrap();
        }
        assert!(
            budget
                .reserve_request(timeout)
                .unwrap_err()
                .to_string()
                .contains("intent")
        );
        budget.begin_intent(command_timeout);

        assert!(!budget.reserve_request(timeout).unwrap().is_zero());
    }

    #[test]
    fn one_expired_intent_does_not_consume_the_next_intents_time_slice() {
        let command_timeout = CommandTimeout::from_seconds(1).unwrap();
        let mut budget = ReviewThreadUpdateBudget::new(command_timeout, 2);
        budget.intent_started_at -= Duration::from_secs(2);

        assert!(
            budget
                .reserve_request(command_timeout.duration())
                .unwrap_err()
                .to_string()
                .contains("intent")
        );
        budget.begin_intent(command_timeout);

        assert!(budget.reserve_request(command_timeout.duration()).is_ok());
    }

    #[test]
    fn aggregate_deadline_scales_with_unique_actionable_intents() {
        let command_timeout = CommandTimeout::from_seconds(2).unwrap();

        assert_eq!(
            ReviewThreadUpdateBudget::new(command_timeout, 3).timeout,
            Duration::from_secs(6)
        );
        assert_eq!(
            ReviewThreadUpdateBudget::new(command_timeout, usize::MAX).timeout,
            REVIEW_THREAD_UPDATE_TIMEOUT
        );
    }

    #[test]
    fn review_thread_budget_preserves_subsecond_remaining_time() {
        let command_timeout = CommandTimeout::from_seconds(1).unwrap();
        let mut budget = ReviewThreadUpdateBudget::new(command_timeout, 1);

        let first = budget.reserve_request(command_timeout.duration()).unwrap();
        let second = budget.reserve_request(command_timeout.duration()).unwrap();

        assert!(!first.is_zero());
        assert!(!second.is_zero());
        assert!(second <= first);
        assert_eq!(budget.request_count, 2);
    }

    #[test]
    fn reconciliation_keeps_a_dedicated_deadline_and_the_full_worst_case_budget() {
        let command_timeout = CommandTimeout::from_seconds(1).unwrap();
        let mut budget = ReviewThreadUpdateBudget::new(command_timeout, 1);
        for _ in 0..REVIEW_THREAD_PRIMARY_REQUESTS_PER_INTENT {
            budget
                .reserve_request(command_timeout.duration())
                .unwrap();
        }
        budget.started_at -= Duration::from_secs(2);
        budget.intent_started_at -= Duration::from_secs(2);
        budget.begin_reconciliation();

        for _ in 0..REVIEW_THREAD_RECONCILIATION_REQUESTS_PER_INTENT {
            budget
                .reserve_reconciliation_request(MUTATION_RECONCILIATION_TIMEOUT)
                .unwrap();
        }

        assert_eq!(budget.request_count, REVIEW_THREAD_UPDATE_REQUESTS_PER_INTENT);
        assert_eq!(REVIEW_THREAD_UPDATE_REQUESTS_PER_INTENT, 403);
        assert!(
            budget
                .reserve_reconciliation_request(MUTATION_RECONCILIATION_TIMEOUT)
                .unwrap_err()
                .to_string()
                .contains("403-request total budget")
        );
    }

    #[test]
    fn cancellation_before_mutation_skips_remote_reconciliation() {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut reply_budget = ReviewThreadUpdateBudget::new(ctx.command_timeout(), 1);
        let mut resolve_budget = ReviewThreadUpdateBudget::new(ctx.command_timeout(), 1);

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
