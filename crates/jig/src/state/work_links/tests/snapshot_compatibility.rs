use super::*;
use crate::tracker::{BeadsExport, PRIMARY_EXPORT};

const WORKSPACE_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

#[test]
fn supported_beads_requirements_survive_snapshot_append_projection_and_retry() {
    for (description, acceptance, fill_record) in [
        ("d".repeat(600_000), "a".repeat(300_000), false),
        ("d".repeat(300_000), "a".repeat(600_000), false),
        ("\\\"\n\u{1}é😀".repeat(50_000), String::new(), true),
    ] {
        let (_temp, ctx) = context();
        seed_plan(&ctx, "plan_example");
        let mut producer_record: Value = serde_json::from_str(include_str!(
            "../../../tracker/test_fixtures/br-0.5.7-colon-prefix.jsonl"
        ))
        .unwrap();
        producer_record["description"] = json!(description);
        producer_record["acceptance_criteria"] = json!(acceptance);
        if fill_record {
            // Exercise the public 1 MiB Beads record boundary, including JSON escapes.
            let padding = 1024 * 1024 - serde_json::to_vec(&producer_record).unwrap().len();
            let text = producer_record["description"].as_str().unwrap().to_owned();
            producer_record["description"] = json!(text + &"x".repeat(padding));
            assert_eq!(
                serde_json::to_vec(&producer_record).unwrap().len(),
                1024 * 1024
            );
        }
        fs::create_dir(ctx.root().join(".beads")).unwrap();
        let export_path = ctx.root().join(PRIMARY_EXPORT);
        let export_bytes = format!("{producer_record}\n").into_bytes();
        fs::write(&export_path, &export_bytes).unwrap();
        let export = BeadsExport::open(ctx.root(), WORKSPACE_ID).unwrap();
        let issue = export.issue("team:api-r89").unwrap();
        let snapshot = WorkLinkSnapshotV1::new(
            1,
            &issue.title,
            &issue.description,
            &issue.acceptance_criteria,
        )
        .unwrap();
        let request = WorkLinkRequest::new(
            "plan_example",
            WorkLinkIssueV1::beads(&issue.workspace_id, &issue.id).unwrap(),
            snapshot,
            WorkLinkEstablishedBy::Attach,
        )
        .unwrap();

        let written = attach_work_link(&ctx, &request).unwrap();
        let journal_path = ctx.state_file(WORK_LINKS_FILE);
        let journal_bytes = fs::read(&journal_path).unwrap();
        assert!(journal_bytes.len() <= MAX_WORK_LINK_RECORD_BYTES + 1);
        assert_eq!(fs::read(&export_path).unwrap(), export_bytes);
        assert_eq!(written.snapshot.title, issue.title);
        assert_eq!(written.snapshot.description, issue.description);
        assert_eq!(
            written.snapshot.acceptance_criteria,
            issue.acceptance_criteria
        );
        let WorkLinkProjection::Supported(projected) =
            project_work_link(&ctx, "plan_example").unwrap()
        else {
            panic!("the complete historical snapshot must remain readable");
        };
        assert_eq!(projected.record, written);

        producer_record["description"] = json!("Updated requirements");
        producer_record["acceptance_criteria"] = json!("Updated acceptance");
        fs::write(&export_path, format!("{producer_record}\n")).unwrap();
        let refreshed = BeadsExport::open(ctx.root(), WORKSPACE_ID).unwrap();
        let issue = refreshed.issue("team:api-r89").unwrap();
        let retry = WorkLinkRequest::new(
            "plan_example",
            request.issue,
            WorkLinkSnapshotV1::new(
                2,
                &issue.title,
                &issue.description,
                &issue.acceptance_criteria,
            )
            .unwrap(),
            WorkLinkEstablishedBy::Attach,
        )
        .unwrap();
        assert_eq!(attach_work_link(&ctx, &retry).unwrap(), written);
        assert_eq!(fs::read(&journal_path).unwrap(), journal_bytes);
    }
}

#[test]
fn serialized_snapshot_overflow_refuses_append_without_truncating_history() {
    let (_temp, ctx) = context();
    seed_plan(&ctx, "plan_existing");
    seed_plan(&ctx, "plan_candidate");
    attach_work_link(&ctx, &request("plan_existing", "example-existing")).unwrap();
    let journal_path = ctx.state_file(WORK_LINKS_FILE);
    let before = fs::read(&journal_path).unwrap();
    // Each byte requires six JSON bytes, so raw text length is not the record bound.
    let snapshot = WorkLinkSnapshotV1::new(
        1,
        "Example",
        "\u{1}".repeat(MAX_WORK_LINK_RECORD_BYTES / 6 + 1),
        "Acceptance",
    )
    .unwrap();
    let candidate = WorkLinkRequest::new(
        "plan_candidate",
        WorkLinkIssueV1::beads(WORKSPACE_ID, "example-candidate").unwrap(),
        snapshot,
        WorkLinkEstablishedBy::Attach,
    )
    .unwrap();

    let error = attach_work_link(&ctx, &candidate).unwrap_err().to_string();

    assert!(error.contains("work-link record exceeds"), "{error}");
    assert_eq!(fs::read(&journal_path).unwrap(), before);
    assert!(matches!(
        project_work_link(&ctx, "plan_existing").unwrap(),
        WorkLinkProjection::Supported(_)
    ));
    assert_eq!(
        project_work_link(&ctx, "plan_candidate").unwrap(),
        WorkLinkProjection::Unlinked
    );
}
