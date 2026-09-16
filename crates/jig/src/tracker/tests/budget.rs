use super::*;
use std::cell::Cell;
use std::rc::Rc;

#[test]
fn mutation_budget_covers_readiness_and_the_write_as_one_operation() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "split_budget");
    let policy = TrackerProcessPolicy {
        read_timeout: Duration::from_secs(1),
        mutation_timeout: Duration::from_millis(700),
        ..TrackerProcessPolicy::default()
    };
    let (adapter, _) = fixture.discover(policy);
    let mut never_cancelled = || false;

    assert_eq!(
        adapter
            .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::IndeterminateWrite {
            operation: TrackerOperation::ClaimIssue
        }
    );
    let groups = logged_argument_groups(&fixture.log);
    assert_eq!(groups.len(), 4);
    assert!(groups[2].iter().any(|argument| argument == "sync"));
    assert!(groups[3].iter().any(|argument| argument == "update"));
}

#[test]
fn terminal_validated_results_win_over_late_cancellation() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let completed = Rc::new(Cell::new(false));
    let hook_completed = Rc::clone(&completed);
    let _hook = TestAfterProfiledProcessHook::set(move |operation| {
        if matches!(
            operation,
            TrackerOperation::ShowIssue | TrackerOperation::ClaimIssue
        ) {
            hook_completed.set(true);
        }
    });
    let mut cancelled_after_exit = || completed.get();

    assert_eq!(
        adapter
            .show_issue(ISSUE_ID, &mut cancelled_after_exit)
            .unwrap()
            .id,
        ISSUE_ID
    );

    completed.set(false);
    let claimed = adapter
        .claim_issue(ISSUE_ID, ACTOR, &mut cancelled_after_exit)
        .unwrap();
    assert_eq!(claimed.assignee.as_deref(), Some(ACTOR));
}
