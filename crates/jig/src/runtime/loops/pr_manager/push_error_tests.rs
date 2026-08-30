#[cfg(test)]
mod push_error_tests {
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
}
