use super::*;

#[test]
fn transferring_admission_budget_to_execution_does_not_restart_it() {
    let budget = TargetBudget {
        started: Instant::now(),
        timeout: Duration::from_secs(120),
    };
    let mut observer = CancellationOnlyRunControl {
        cancelled: &|| Ok(false),
    };
    let remaining_before = {
        let admission = TargetExecutionControl::with_budget(budget, &mut observer, None);
        assert_eq!(admission.budget.started, budget.started);
        admission.remaining().unwrap()
    };
    let execution = TargetExecutionControl::with_budget(budget, &mut observer, None);
    assert_eq!(execution.budget.started, budget.started);
    let remaining_after = execution.remaining().unwrap();
    assert!(remaining_after <= remaining_before);
    assert_eq!(execution.budget.timeout, budget.timeout);
}

#[test]
fn expired_admission_budget_cannot_become_a_new_execution_budget() {
    let budget = TargetBudget {
        started: Instant::now(),
        timeout: Duration::ZERO,
    };
    let mut observer = CancellationOnlyRunControl {
        cancelled: &|| Ok(false),
    };
    for _ in 0..2 {
        let control = TargetExecutionControl::with_budget(budget, &mut observer, None);
        assert!(matches!(control.remaining(), Err(TargetStop::TimedOut)));
    }
}
