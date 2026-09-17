use super::*;
use crate::state::work_links::{
    WorkLinkEstablishedBy, WorkLinkIssueV1, WorkLinkRequest, WorkLinkSnapshotV1,
};

fn record(plan_id: &str, event_id: &str, issue_id: &str) -> Vec<u8> {
    let request = WorkLinkRequest::new(
        plan_id,
        WorkLinkIssueV1::beads("01EXAMPLEWORKSPACE", issue_id).unwrap(),
        WorkLinkSnapshotV1::new(1, "Example", "Description", "Acceptance").unwrap(),
        WorkLinkEstablishedBy::Attach,
    )
    .unwrap();
    serde_json::to_vec(&WorkLinkRecordV1::from_request(event_id.into(), &request)).unwrap()
}

fn observe(journal: &mut JournalProjection, line: u64, bytes: &[u8]) -> Result<()> {
    journal.observe(RawJsonlRecord {
        line_number: line,
        start_offset: 0,
        bytes,
        terminated: true,
    })
}

#[test]
fn unsupported_lock_preserves_torn_work_link_authority() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("work-links.jsonl");
    let mut completed = record("plan_example", "work-link_example", "example-123");
    completed.push(b'\n');

    let mut conflicting = record("plan_example", "work-link_conflicting", "example-456");
    conflicting.pop();
    completed.extend(conflicting);

    for (bytes, final_line) in [(vec![b'{'], 1), (completed, 2)] {
        std::fs::write(&path, &bytes).unwrap();
        let journal = crate::state::jsonl::with_unsupported_scan_lock(|| {
            scan_journal(&path, Some("plan_example")).unwrap()
        });

        let diagnostics = journal.diagnostics();
        assert_eq!(diagnostics.authority, WorkLinkJournalAuthority::Torn);
        assert!(diagnostics.torn_tail);
        assert_eq!(diagnostics.errors[0].line_number, Some(final_line));
        assert!(matches!(
            journal.for_plan("plan_example"),
            WorkLinkProjection::Corrupt(_)
        ));
        assert!(journal.ensure_authoritative_write_safe().is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn projection_retains_full_record_only_for_the_selected_plan() {
    let mut journal = JournalProjection::new(
        Some("plan_selected"),
        ProjectionLimits {
            unique_events: 10,
            known_plans: 10,
        },
    );
    observe(
        &mut journal,
        1,
        &record("plan_other", "work-link_other", "example-other"),
    )
    .unwrap();
    observe(
        &mut journal,
        2,
        &record("plan_selected", "work-link_selected", "example-selected"),
    )
    .unwrap();

    assert!(journal.plans["plan_other"].selected.is_none());
    assert!(journal.plans["plan_selected"].selected.is_some());
    assert_eq!(
        journal.plans["plan_selected"]
            .selected
            .as_ref()
            .unwrap()
            .event_ids
            .len(),
        1
    );
    assert_eq!(journal.seen_events.len(), 2);
}

#[test]
fn projection_rejects_unique_events_before_crossing_aggregate_limit() {
    let mut journal = JournalProjection::new(
        None,
        ProjectionLimits {
            unique_events: 2,
            known_plans: 2,
        },
    );
    observe(
        &mut journal,
        1,
        br#"{"id":"work-link_a","schema_version":2,"plan_id":"plan_a"}"#,
    )
    .unwrap();
    observe(
        &mut journal,
        2,
        br#"{"id":"work-link_b","schema_version":2,"plan_id":"plan_a"}"#,
    )
    .unwrap();
    let error = observe(
        &mut journal,
        3,
        br#"{"id":"work-link_c","schema_version":2,"plan_id":"plan_a"}"#,
    )
    .unwrap_err();

    assert!(error.downcast_ref::<WorkLinkProjectionLimit>().is_some());
    assert_eq!(journal.seen_events.len(), 2);
}

#[test]
fn event_fingerprint_preserves_complete_json_semantics_not_layout() {
    let mut journal = JournalProjection::new(
        None,
        ProjectionLimits {
            unique_events: 1,
            known_plans: 1,
        },
    );
    observe(
        &mut journal,
        1,
        br#"{"id":"work-link_a","schema_version":2,"plan_id":"plan_a","extension":{"enabled":true}}"#,
    )
    .unwrap();
    observe(
        &mut journal,
        2,
        br#"{ "extension": { "enabled": true }, "plan_id": "plan_a", "schema_version": 2, "id": "work-link_a" }"#,
    )
    .unwrap();

    assert_eq!(journal.seen_events.len(), 1);
    assert_eq!(journal.replayed_records, 1);
    assert!(journal.global_conflicts.is_empty());
}

#[test]
fn projection_rejects_known_plans_before_crossing_aggregate_limit() {
    let mut journal = JournalProjection::new(
        None,
        ProjectionLimits {
            unique_events: 3,
            known_plans: 2,
        },
    );
    observe(
        &mut journal,
        1,
        br#"{"id":"work-link_a","schema_version":2,"plan_id":"plan_a"}"#,
    )
    .unwrap();
    observe(
        &mut journal,
        2,
        br#"{"id":"work-link_b","schema_version":2,"plan_id":"plan_b"}"#,
    )
    .unwrap();
    let error = observe(
        &mut journal,
        3,
        br#"{"id":"work-link_c","schema_version":2,"plan_id":"plan_c"}"#,
    )
    .unwrap_err();

    assert!(error.downcast_ref::<WorkLinkProjectionLimit>().is_some());
    assert_eq!(journal.plans.len(), 2);
    assert_eq!(journal.seen_events.len(), 2);
}

#[test]
fn projection_truncates_untrusted_diagnostics_when_they_are_recorded() {
    let mut journal = JournalProjection::new(
        None,
        ProjectionLimits {
            unique_events: 1,
            known_plans: 1,
        },
    );
    let private_tail = "private-tail";
    let provider = format!("{}{}", "x".repeat(256 * 1024), private_tail);
    let bytes = serde_json::to_vec(&serde_json::json!({
        "id": "work-link_a",
        "schema_version": 1,
        "plan_id": "plan_a",
        "issue": {"provider": provider},
    }))
    .unwrap();

    observe(&mut journal, 1, &bytes).unwrap();

    let sample = journal.plans["plan_a"]
        .unsupported
        .sample
        .as_deref()
        .unwrap();
    assert!(sample.message.len() < 10_000);
    assert!(!sample.message.contains(private_tail));
}
