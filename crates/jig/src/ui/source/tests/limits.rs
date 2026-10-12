use jig_context::RepoContext;
use jig_contract::TargetOutputTailV1;
use jig_dashboard::{CollectionDomain, DashboardSource, LimitId, RecorderMode, TimelineLimit};
use serde_json::json;
use tempfile::tempdir;

use crate::test_env::TestRepoBuilder;

use super::super::{MAX_AGGREGATION_KEYS, RepoDashboardSource};
use super::{append_target_results, recorder_request, source_fixture, target_result};

#[test]
fn recorder_reports_exact_root_and_nested_omissions() {
    let root = tempdir().unwrap();
    TestRepoBuilder::new(root.path()).write();
    let context = RepoContext::load_from(root.path()).unwrap();
    let stderr = format!(
        "{}{}",
        "head-".repeat(2),
        "e".repeat(LimitId::FailureOutputChars.ceiling() - 3)
    );
    append_target_results(
        &context,
        (0..257).map(|index| {
            let mut result = target_result(
                &format!("repo:example-{index}"),
                "failure",
                1_000 + index,
                2_000 + index,
            );
            result["output_tail"] = json!({"stdout": "output", "stderr": stderr});
            result
        }),
    );

    let source = RepoDashboardSource::new(context);
    let refresh = source
        .recorder(
            jig_dashboard::RecorderRequest {
                mode: RecorderMode::Refresh,
                timeline_limit: TimelineLimit::new(5).unwrap(),
            },
            &|| false,
        )
        .unwrap();
    let snapshot = refresh.recorder;
    assert_eq!(snapshot.failures.len(), LimitId::Failures.ceiling());
    assert_eq!(snapshot.limits.failures.omitted, Some(247));
    assert_eq!(snapshot.target_stats.len(), LimitId::TargetStats.ceiling());
    assert_eq!(snapshot.limits.target_stats.omitted, Some(1));
    assert_eq!(snapshot.timeline.len(), 5);
    assert_eq!(snapshot.limits.timeline.omitted, Some(252));
    // Failure output keeps its tail, where the error usually is.
    let tail = &snapshot.failures[0].output_tail;
    assert_eq!(
        tail.text().chars().count(),
        LimitId::FailureOutputChars.ceiling()
    );
    assert_eq!(tail.omitted_chars(), Some(7));
    assert!(tail.text().starts_with("ad-e"), "{}", tail.text());
    assert_eq!(snapshot.timeline[0].output_tail, Some(tail.clone()));
}

#[test]
fn recorder_reports_unknown_omission_after_run_log_truncation() {
    let limit = LimitId::FailureOutputChars.ceiling();
    for (stdout, stderr, expected) in [
        ("unused".to_owned(), "e".repeat(10_000), "e".repeat(limit)),
        ("o".repeat(10_000), String::new(), "o".repeat(limit)),
        ("unused".to_owned(), "界".repeat(1_500), "界".repeat(limit)),
        ("🦀".repeat(1_250), String::new(), "🦀".repeat(limit)),
    ] {
        let (_root, source) = source_fixture();
        let persisted = TargetOutputTailV1::from_streams(&stdout, &stderr).unwrap();
        assert!(persisted.stdout_omitted_bytes > 0 || persisted.stderr_omitted_bytes > 0);
        let mut result = target_result("repo:truncated-output", "failure", 30, 40);
        result["output_tail"] = serde_json::to_value(persisted).unwrap();
        append_target_results(&source.context, [result]);

        let snapshot = source
            .recorder(recorder_request(RecorderMode::Refresh), &|| false)
            .unwrap()
            .recorder;
        assert!(
            snapshot
                .errors
                .iter()
                .all(|error| error.scope() != CollectionDomain::Runs.as_str()),
            "{:?}",
            snapshot.errors
        );
        let tail = &snapshot.failures[0].output_tail;
        assert_eq!(tail.text(), expected);
        assert_eq!(tail.applied_chars(), limit);
        assert_eq!(tail.omitted_chars(), None);
        let timeline = snapshot
            .timeline
            .iter()
            .find(|row| row.target == "repo:truncated-output")
            .unwrap();
        assert_eq!(timeline.output_tail.as_ref(), Some(tail));
    }
}

#[test]
fn recorder_counts_complete_stderr_when_only_stdout_was_truncated() {
    let (_root, source) = source_fixture();
    let limit = LimitId::FailureOutputChars.ceiling();
    let persisted =
        TargetOutputTailV1::from_streams(&"o".repeat(10_000), &"é".repeat(limit + 7)).unwrap();
    assert!(persisted.stdout_omitted_bytes > 0);
    assert_eq!(persisted.stderr_omitted_bytes, 0);
    let mut result = target_result("repo:complete-stderr", "failure", 30, 40);
    result["output_tail"] = serde_json::to_value(persisted).unwrap();
    append_target_results(&source.context, [result]);

    let snapshot = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap()
        .recorder;
    assert!(
        snapshot
            .errors
            .iter()
            .all(|error| error.scope() != CollectionDomain::Runs.as_str()),
        "{:?}",
        snapshot.errors
    );
    let tail = &snapshot.failures[0].output_tail;
    assert_eq!(tail.text(), "é".repeat(limit));
    assert_eq!(tail.omitted_chars(), Some(7));
    let timeline = snapshot
        .timeline
        .iter()
        .find(|row| row.target == "repo:complete-stderr")
        .unwrap();
    assert_eq!(timeline.output_tail.as_ref(), Some(tail));
}

#[test]
fn aggregation_key_caps_return_scoped_partial_observations() {
    let (_root, source) = source_fixture();
    append_target_results(
        &source.context,
        (0..MAX_AGGREGATION_KEYS as u64).map(|index| {
            target_result(&format!("repo:target-{index}"), "success", index, index + 1)
        }),
    );

    let refresh = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    assert!(refresh.recorder.errors.iter().any(|error| {
        error.scope() == CollectionDomain::Runs.as_str()
            && error.message().contains("working-set limit")
    }));
    assert!(
        refresh
            .status_local
            .errors
            .iter()
            .all(|error| error.scope == "repository")
    );
}
