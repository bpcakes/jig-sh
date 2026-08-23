use super::*;

#[test]
fn cleanup_deadline_is_fixed_by_the_first_attempt() {
    let mut deadline = None;
    let (first, first_attempt) =
        fixed_process_cleanup_deadline(&mut deadline, Duration::from_secs(1)).unwrap();
    let (second, second_attempt) =
        fixed_process_cleanup_deadline(&mut deadline, Duration::from_secs(30)).unwrap();

    assert!(first_attempt);
    assert!(!second_attempt);
    assert_eq!(first, second);
}

#[test]
fn secondary_cleanup_error_does_not_replace_primary_error() {
    let error = append_secondary_error(
        anyhow!("capture failed first"),
        "process cleanup also failed",
        Some(anyhow!("cleanup failed second")),
    )
    .to_string();

    assert!(error.starts_with("capture failed first"));
    assert!(error.contains("process cleanup also failed: cleanup failed second"));
}

#[test]
fn completed_poll_results_take_precedence_over_a_newly_expired_deadline() {
    let drain_error =
        preserve_poll_result_before_timeout(Err(anyhow!("capture failed first")), None, || {
            anyhow!("timeout second")
        })
        .unwrap_err()
        .to_string();
    assert_eq!(drain_error, "capture failed first");

    assert_eq!(
        preserve_leader_poll_result_before_timeout(
            Ok(LeaderObservation::Exited),
            None,
            || anyhow!("timeout second"),
        )
        .unwrap(),
        LeaderObservation::Exited,
    );
    let observation_error = preserve_leader_poll_result_before_timeout(
        Err(anyhow!("observation failed first")),
        None,
        || anyhow!("timeout second"),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(observation_error, "observation failed first");
    let timeout =
        preserve_leader_poll_result_before_timeout(Ok(LeaderObservation::Running), None, || {
            anyhow!("timeout after running observation")
        })
        .unwrap_err()
        .to_string();
    assert_eq!(timeout, "timeout after running observation");
}
