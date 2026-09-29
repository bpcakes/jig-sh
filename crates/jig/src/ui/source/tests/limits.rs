use jig_ui::dashboard::{CollectionDomain, DashboardSource, LimitId, RecorderMode, TimelineLimit};
use serde_json::json;
use tempfile::tempdir;

use crate::context::RepoContext;
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
            jig_ui::dashboard::RecorderRequest {
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
