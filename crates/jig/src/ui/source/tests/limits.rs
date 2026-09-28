use std::fs;

use jig_ui::dashboard::{CollectionDomain, DashboardSource, LimitId, RecorderMode, TimelineLimit};
use serde_json::json;
use tempfile::tempdir;

use crate::context::RepoContext;
use crate::state::{ReceiptInput, record_receipt};
use crate::test_env::TestRepoBuilder;

use super::super::{MAX_AGGREGATION_KEYS, RepoDashboardSource};
use super::{recorder_request, source_fixture};

#[test]
fn recorder_reports_exact_root_and_nested_omissions() {
    let root = tempdir().unwrap();
    TestRepoBuilder::new(root.path()).write();
    let context = RepoContext::load_from(root.path()).unwrap();
    fs::create_dir_all(root.path().join(".agent/state")).unwrap();

    for index in 0..257 {
        record_receipt(
            &context,
            ReceiptInput {
                tool_name: &format!("jig.example_{index}"),
                args: json!({}),
                invoked_command_key: Some(format!("example_{index}")),
                started_at_ms: 1_000 + index,
                ended_at_ms: 2_000 + index,
                exit_status: 1,
                stdout: "output",
                stderr: &"e".repeat(LimitId::FailureStderrChars.ceiling() + 7),
                evidence: None,
                collect_git_metadata: false,
                collect_worktree_fingerprint: false,
                worktree_fingerprint_override: None,
            },
        )
        .unwrap();
    }

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
    assert_eq!(snapshot.tool_stats.len(), LimitId::ToolStats.ceiling());
    assert_eq!(snapshot.limits.tool_stats.omitted, Some(1));
    assert_eq!(snapshot.timeline.len(), 5);
    assert_eq!(snapshot.limits.timeline.omitted, Some(252));
    assert_eq!(
        snapshot.failures[0].stderr_preview.text().chars().count(),
        LimitId::FailureStderrChars.ceiling()
    );
    assert_eq!(snapshot.failures[0].stderr_preview.omitted_chars(), Some(7));
}

#[test]
fn aggregation_key_caps_return_scoped_partial_observations() {
    let (root, source) = source_fixture();
    let receipts = (0..=MAX_AGGREGATION_KEYS)
        .map(|index| {
            format!(
                "{}\n",
                json!({
                    "id": format!("receipt-{index}"),
                    "tool_name": format!("jig.tool-{index}"),
                    "args": {},
                    "invoked_command_key": format!("tool-{index}"),
                    "started_at_ms": index,
                    "ended_at_ms": index + 1,
                    "exit_status": 0,
                    "stdout_preview": "",
                    "stderr_preview": "",
                    "changed_paths": [],
                    "diff_stat": {"files": 0, "insertions": 0, "deletions": 0}
                })
            )
        })
        .collect::<String>();
    fs::write(root.path().join(".agent/state/receipts.jsonl"), receipts).unwrap();

    let refresh = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    assert!(refresh.recorder.errors.iter().any(|error| {
        error.scope() == CollectionDomain::Receipts.as_str()
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
