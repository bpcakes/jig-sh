use std::fs;
use std::sync::{Arc, Barrier};
use std::thread;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::state::jsonl::{
    DurableAppendFailurePoint, JsonlRecordTooLarge, fail_next_durable_append_at,
};

fn context() -> (TempDir, RepoContext) {
    let temp = TempDir::new().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .repo_name("ExampleProject")
        .contract_version(crate::context::TRACKER_JOURNAL_CONTRACT_VERSION)
        .config(
            r#"[repository]
default_check_profile = "verify"
components = []
actions = []
profiles = []"#,
        )
        .write();
    let contract_path = temp.path().join(".agent/jig-contract.json");
    let mut contract: Value = serde_json::from_slice(&fs::read(&contract_path).unwrap()).unwrap();
    contract["default_check_profile"] = json!("verify");
    fs::write(contract_path, serde_json::to_vec_pretty(&contract).unwrap()).unwrap();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
    (temp, ctx)
}

fn request(plan_id: &str, issue_id: &str) -> WorkLinkRequest {
    WorkLinkRequest::new(
        plan_id,
        WorkLinkIssueV1::beads("01EXAMPLEWORKSPACE", issue_id).unwrap(),
        WorkLinkSnapshotV1::new(
            1_700_000_000_000,
            "Example issue",
            "Acceptance: preserve the historical plan.",
        )
        .unwrap(),
        WorkLinkEstablishedBy::Attach,
    )
    .unwrap()
}

fn seed_plan(ctx: &RepoContext, plan_id: &str) {
    super::super::plans::seed_open_plan_for_test(ctx, plan_id, "Example plan", "Body").unwrap();
}

#[test]
fn missing_journal_projects_unlinked_without_creating_files() {
    let (_temp, ctx) = context();
    let path = ctx.state_file(WORK_LINKS_FILE);

    assert_eq!(
        project_work_link(&ctx, "plan_example").unwrap(),
        WorkLinkProjection::Unlinked
    );
    assert!(!path.exists());
    assert!(!super::super::jsonl::state_lock_path(&path).exists());
}

#[test]
fn exact_retry_returns_original_event_without_appending() {
    let (_temp, ctx) = context();
    seed_plan(&ctx, "plan_example");
    let request = request("plan_example", "example-123");

    let first = attach_work_link(&ctx, &request).unwrap();
    let path = ctx.state_file(WORK_LINKS_FILE);
    let first_bytes = fs::read(&path).unwrap();
    let second = attach_work_link(&ctx, &request).unwrap();

    assert_eq!(second.id, first.id);
    assert_eq!(fs::read(&path).unwrap(), first_bytes);
    assert!(first_bytes.ends_with(b"\n"));
}

#[test]
fn exact_retry_reconfirms_durability_after_each_ambiguous_sync_failure() {
    for failure in [
        DurableAppendFailurePoint::BeforeFileSync,
        DurableAppendFailurePoint::BeforeParentSync,
    ] {
        let (_temp, ctx) = context();
        seed_plan(&ctx, "plan_example");
        let request = request("plan_example", "example-123");
        let path = ctx.state_file(WORK_LINKS_FILE);
        fail_next_durable_append_at(failure);

        let error = attach_work_link(&ctx, &request).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("injected durable append failure")
        );
        let visible_bytes = fs::read(&path).unwrap();
        assert!(visible_bytes.ends_with(b"\n"));

        let retried = attach_work_link(&ctx, &request).unwrap();
        assert_eq!(retried.plan_id, "plan_example");
        assert_eq!(retried.issue.issue_id, "example-123");
        assert_eq!(fs::read(&path).unwrap(), visible_bytes);
    }
}

#[test]
fn terminated_and_unterminated_oversized_records_are_bounded_before_decode() {
    for terminated in [true, false] {
        let (_temp, ctx) = context();
        let path = ctx.state_file(WORK_LINKS_FILE);
        fs::create_dir_all(ctx.state_dir()).unwrap();
        let mut bytes = vec![b'x'; MAX_WORK_LINK_RECORD_BYTES + 1];
        if terminated {
            bytes.push(b'\n');
        }
        fs::write(&path, bytes).unwrap();

        let error = project_work_link(&ctx, "plan_example").unwrap_err();
        let oversized = error.downcast_ref::<JsonlRecordTooLarge>().unwrap();
        assert_eq!(oversized.start_offset(), 0);
        assert_eq!(oversized.limit(), MAX_WORK_LINK_RECORD_BYTES);
    }
}

#[test]
fn same_issue_retry_ignores_a_fresh_historical_snapshot() {
    let (_temp, ctx) = context();
    seed_plan(&ctx, "plan_example");
    let original = request("plan_example", "example-123");
    let first = attach_work_link(&ctx, &original).unwrap();
    let path = ctx.state_file(WORK_LINKS_FILE);
    let before = fs::read(&path).unwrap();
    let refreshed = WorkLinkRequest::new(
        "plan_example",
        original.issue,
        WorkLinkSnapshotV1::new(1_700_000_000_001, "Updated title", "Updated acceptance").unwrap(),
        WorkLinkEstablishedBy::Start,
    )
    .unwrap();

    let retried = attach_work_link(&ctx, &refreshed).unwrap();

    assert_eq!(retried, first);
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn union_merged_same_issue_links_choose_one_deterministic_snapshot() {
    let (_temp, ctx) = context();
    let first_request = request("plan_example", "example-123");
    let second_request = WorkLinkRequest::new(
        "plan_example",
        first_request.issue.clone(),
        WorkLinkSnapshotV1::new(1_700_000_000_001, "Updated title", "Updated acceptance").unwrap(),
        WorkLinkEstablishedBy::Start,
    )
    .unwrap();
    let first = WorkLinkRecordV1::from_request("work-link_b".into(), &first_request);
    let second = WorkLinkRecordV1::from_request("work-link_a".into(), &second_request);
    fs::create_dir_all(ctx.state_dir()).unwrap();
    let path = ctx.state_file(WORK_LINKS_FILE);
    fs::write(
        &path,
        format!(
            "{}\n{}\n",
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap()
        ),
    )
    .unwrap();

    let WorkLinkProjection::Supported(link) = project_work_link(&ctx, "plan_example").unwrap()
    else {
        panic!("same issue links must converge");
    };
    assert_eq!(link.record.id, "work-link_a");
    assert_eq!(
        link.event_ids,
        ["work-link_a".to_string(), "work-link_b".to_string()]
    );
}

#[test]
fn concurrent_exact_retries_commit_one_line_and_return_one_event() {
    let (_temp, ctx) = context();
    seed_plan(&ctx, "plan_example");
    let ctx = Arc::new(ctx);
    let request = Arc::new(request("plan_example", "example-123"));
    let worker_count = 8;
    let barrier = Arc::new(Barrier::new(worker_count));
    let handles = (0..worker_count)
        .map(|_| {
            let ctx = Arc::clone(&ctx);
            let request = Arc::clone(&request);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                attach_work_link(&ctx, &request).unwrap().id
            })
        })
        .collect::<Vec<_>>();
    let event_ids = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();

    assert!(event_ids.iter().all(|event_id| event_id == &event_ids[0]));
    let bytes = fs::read(ctx.state_file(WORK_LINKS_FILE)).unwrap();
    assert!(bytes.ends_with(b"\n"));
    assert_eq!(
        bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .count(),
        1
    );
}

#[test]
fn distinct_link_is_rejected_without_appending() {
    let (_temp, ctx) = context();
    seed_plan(&ctx, "plan_example");
    attach_work_link(&ctx, &request("plan_example", "example-123")).unwrap();
    let path = ctx.state_file(WORK_LINKS_FILE);
    let before = fs::read(&path).unwrap();

    let error = attach_work_link(&ctx, &request("plan_example", "example-456"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("already has immutable work link"));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn future_schema_is_scoped_to_its_plan_and_has_no_authority() {
    let (_temp, ctx) = context();
    fs::create_dir_all(ctx.state_dir()).unwrap();
    let value = json!({
        "id": "work-link_future",
        "schema_version": 2,
        "plan_id": "plan_future",
        "future_authority": {"enabled": true}
    });
    fs::write(
        ctx.state_file(WORK_LINKS_FILE),
        format!("{}\n", serde_json::to_string(&value).unwrap()),
    )
    .unwrap();

    assert!(matches!(
        project_work_link(&ctx, "plan_future").unwrap(),
        WorkLinkProjection::Unsupported(_)
    ));
    assert_eq!(
        project_work_link(&ctx, "plan_unrelated").unwrap(),
        WorkLinkProjection::Unlinked
    );
}

#[test]
fn torn_tail_is_corrupt_and_blocks_append_without_changing_bytes() {
    let (_temp, ctx) = context();
    seed_plan(&ctx, "plan_example");
    let request = request("plan_example", "example-123");
    let record = WorkLinkRecordV1::from_request("work-link_torn".into(), &request);
    fs::create_dir_all(ctx.state_dir()).unwrap();
    let torn = serde_json::to_vec(&record).unwrap();
    let path = ctx.state_file(WORK_LINKS_FILE);
    fs::write(&path, &torn).unwrap();

    assert!(matches!(
        project_work_link(&ctx, "plan_example").unwrap(),
        WorkLinkProjection::Corrupt(_)
    ));
    assert!(attach_work_link(&ctx, &request).is_err());
    assert_eq!(fs::read(path).unwrap(), torn);
}

#[test]
fn corruption_for_one_plan_blocks_writes_but_not_reads_for_another_plan() {
    let (_temp, ctx) = context();
    seed_plan(&ctx, "plan_unrelated");
    fs::create_dir_all(ctx.state_dir()).unwrap();
    let path = ctx.state_file(WORK_LINKS_FILE);
    let corrupt = concat!(
        r#"{"id":"work-link_corrupt","schema_version":1,"plan_id":"plan_damaged","issue":{"provider":"beads","workspace_id":"bad/workspace","issue_id":"example-123","tracker_root":".beads"},"snapshot":{"observed_at_ms":1,"title":"Example","acceptance_context":"","context_digest":"invalid"},"established_by":"attach"}"#,
        "\n"
    );
    fs::write(&path, corrupt).unwrap();

    assert_eq!(
        project_work_link(&ctx, "plan_unrelated").unwrap(),
        WorkLinkProjection::Unlinked
    );
    let before = fs::read(&path).unwrap();
    let error = attach_work_link(&ctx, &request("plan_unrelated", "example-456"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("Refusing authoritative work-link write"));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn closed_plan_can_be_attached_without_rewriting_plan_or_receipt_bytes() {
    let (_temp, ctx) = context();
    seed_plan(&ctx, "plan_example");
    super::super::plans::plans_close(
        &ctx,
        super::super::plans::PlanCloseRequest {
            plan_id: "plan_example".into(),
            resolution: Some("Example work completed".into()),
        },
    )
    .unwrap();
    let plans_path = ctx.state_file("plans.jsonl");
    let receipts_path = ctx.state_file("receipts.jsonl");
    let plans_before = fs::read(&plans_path).unwrap();
    let receipts_before = fs::read(&receipts_path).unwrap();

    attach_work_link(&ctx, &request("plan_example", "example-123")).unwrap();

    assert!(matches!(
        project_work_link(&ctx, "plan_example").unwrap(),
        WorkLinkProjection::Supported(_)
    ));
    assert_eq!(fs::read(plans_path).unwrap(), plans_before);
    assert_eq!(fs::read(receipts_path).unwrap(), receipts_before);
}

#[test]
fn duplicate_event_id_compares_unknown_fields_as_complete_semantics() {
    let (_temp, ctx) = context();
    fs::create_dir_all(ctx.state_dir()).unwrap();
    let first = json!({
        "id": "work-link_future",
        "schema_version": 2,
        "plan_id": "plan_example",
        "future_authority": {"generation": 1}
    });
    let second = json!({
        "id": "work-link_future",
        "schema_version": 2,
        "plan_id": "plan_example",
        "future_authority": {"generation": 2}
    });
    fs::write(
        ctx.state_file(WORK_LINKS_FILE),
        format!(
            "{}\n{}\n",
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap()
        ),
    )
    .unwrap();

    assert!(matches!(
        project_work_link(&ctx, "plan_example").unwrap(),
        WorkLinkProjection::Conflict(_)
    ));
}

#[test]
fn duplicate_json_keys_are_rejected_by_the_authority_projection() {
    let (_temp, ctx) = context();
    fs::create_dir_all(ctx.state_dir()).unwrap();
    fs::write(
            ctx.state_file(WORK_LINKS_FILE),
            concat!(
                r#"{"id":"work-link_duplicate","schema_version":1,"plan_id":"plan_example","plan_id":"plan_other"}"#,
                "\n"
            ),
        )
        .unwrap();

    let WorkLinkProjection::Corrupt(diagnostics) = project_work_link(&ctx, "plan_example").unwrap()
    else {
        panic!("duplicate keys must make work-link authority corrupt");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("duplicate JSON object key"))
    );
}

#[test]
fn additive_v1_fields_survive_an_exact_retry_byte_for_byte() {
    let (_temp, ctx) = context();
    seed_plan(&ctx, "plan_example");
    let request = request("plan_example", "example-123");
    let record = WorkLinkRecordV1::from_request("work-link_extended".into(), &request);
    let mut value = serde_json::to_value(record).unwrap();
    value["extension"] = json!({"future_observation": "preserve me"});
    fs::create_dir_all(ctx.state_dir()).unwrap();
    let path = ctx.state_file(WORK_LINKS_FILE);
    let bytes = format!("{}\n", serde_json::to_string(&value).unwrap()).into_bytes();
    fs::write(&path, &bytes).unwrap();

    assert!(matches!(
        project_work_link(&ctx, "plan_example").unwrap(),
        WorkLinkProjection::Supported(_)
    ));
    attach_work_link(&ctx, &request).unwrap();
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn attach_requires_an_existing_plan_before_creating_journal() {
    let (_temp, ctx) = context();
    let path = ctx.state_file(WORK_LINKS_FILE);

    let error = attach_work_link(&ctx, &request("plan_missing", "example-123"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("Plan not found"));
    assert!(!path.exists());
}

#[test]
fn journal_diagnostics_classify_authority_without_writing() {
    let temp = TempDir::new().unwrap();
    let missing = temp.path().join("missing/work-links.jsonl");
    let empty = work_link_journal_diagnostics_from_path(&missing);
    assert_eq!(empty.authority, WorkLinkJournalAuthority::Empty);
    assert!(!missing.parent().unwrap().exists());

    let supported_path = temp.path().join("supported.jsonl");
    let supported_record = WorkLinkRecordV1::from_request(
        "work-link_supported".into(),
        &request("plan_example", "example-123"),
    );
    fs::write(
        &supported_path,
        format!("{}\n", serde_json::to_string(&supported_record).unwrap()),
    )
    .unwrap();
    let supported = work_link_journal_diagnostics_from_path(&supported_path);
    assert_eq!(supported.authority, WorkLinkJournalAuthority::Supported);
    assert_eq!(supported.supported_plans, 1);
    assert_eq!(supported.supported_records, 1);

    let unsupported_path = temp.path().join("unsupported.jsonl");
    fs::write(
        &unsupported_path,
        b"{\"id\":\"work-link_future\",\"schema_version\":2,\"plan_id\":\"plan_example\"}\n",
    )
    .unwrap();
    let unsupported = work_link_journal_diagnostics_from_path(&unsupported_path);
    assert_eq!(unsupported.authority, WorkLinkJournalAuthority::Unsupported);
    assert_eq!(unsupported.unsupported_plans, 1);

    let conflicting_path = temp.path().join("conflicting.jsonl");
    let first = WorkLinkRecordV1::from_request(
        "work-link_first".into(),
        &request("plan_example", "example-123"),
    );
    let second = WorkLinkRecordV1::from_request(
        "work-link_second".into(),
        &request("plan_example", "example-456"),
    );
    fs::write(
        &conflicting_path,
        format!(
            "{}\n{}\n",
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap()
        ),
    )
    .unwrap();
    let conflicting = work_link_journal_diagnostics_from_path(&conflicting_path);
    assert_eq!(conflicting.authority, WorkLinkJournalAuthority::Conflicting);
    assert_eq!(conflicting.conflicting_plans, 1);

    let corrupt_path = temp.path().join("corrupt.jsonl");
    fs::write(
        &corrupt_path,
        b"{\"id\":\"work-link_corrupt\",\"schema_version\":1,\"plan_id\":\"plan_example\"}\n",
    )
    .unwrap();
    let corrupt = work_link_journal_diagnostics_from_path(&corrupt_path);
    assert_eq!(corrupt.authority, WorkLinkJournalAuthority::Corrupt);
    assert_eq!(corrupt.corrupt_plans, 1);
    assert_eq!(corrupt.error_count, 1);

    let torn_path = temp.path().join("torn.jsonl");
    fs::write(&torn_path, b"{").unwrap();
    let torn = work_link_journal_diagnostics_from_path(&torn_path);
    assert_eq!(torn.authority, WorkLinkJournalAuthority::Torn);
    assert!(torn.torn_tail);
    assert_eq!(torn.errors.len(), 1);

    let oversized_path = temp.path().join("oversized.jsonl");
    fs::write(&oversized_path, vec![b'x'; MAX_WORK_LINK_RECORD_BYTES + 1]).unwrap();
    let oversized = work_link_journal_diagnostics_from_path(&oversized_path);
    assert_eq!(oversized.authority, WorkLinkJournalAuthority::Corrupt);
    assert_eq!(oversized.error_count, 1);
    assert!(oversized.errors[0].message.contains("JSONL record at byte"));
}
