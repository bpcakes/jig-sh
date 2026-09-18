use super::*;

#[test]
fn retirement_receipt_failure_reports_committed_closure() {
    assert_receipt_failure_reports_committed_closure(true);
}

#[test]
fn finish_receipt_failure_reports_committed_closure() {
    assert_receipt_failure_reports_committed_closure(false);
}

fn assert_receipt_failure_reports_committed_closure(retiring: bool) {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path()).write();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let started = crate::runtime::work::start(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "ExampleProject closure".into(),
            body: Some("Exercise partial completion.".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let plan_id = started["plan"]["plan_id"].as_str().unwrap();
    let owner = started["session"]["session_id"].as_str().unwrap();
    let receipts_path = ctx.state_file("receipts.jsonl");
    let receipts_before = fs::read(&receipts_path).unwrap();
    let lock_path = temp
        .path()
        .join(".agent/.cache/state-locks/receipts.jsonl.lock");
    fs::remove_file(&lock_path).unwrap();
    fs::create_dir(&lock_path).unwrap();

    let close = || {
        if retiring {
            crate::runtime::work::retire(
                &ctx,
                crate::command::WorkRetireRequest {
                    plan_id: plan_id.into(),
                    disposition: "cancelled".into(),
                    reason: "No longer needed.".into(),
                    superseded_by: None,
                },
            )
        } else {
            crate::runtime::work::finish_with_cancellation(
                &ctx,
                crate::command::WorkFinishRequest {
                    plan_id: plan_id.into(),
                    resolution: Some("Delivered.".into()),
                    outcome: None,
                },
                &|| false,
            )
        }
    };
    let error = close().unwrap_err();
    let partial = error
        .downcast_ref::<crate::state::PlanClosurePartialFailure>()
        .unwrap()
        .details();
    assert_eq!(partial["plan_id"], plan_id);
    assert_eq!(partial["plan_state"], "closed");
    assert_eq!(partial["receipt"]["status"], "not_recorded");
    assert_eq!(partial["receipt"]["receipt_id"], Value::Null);
    assert_eq!(partial["session_teardown"]["status"], "not_attempted");
    assert_eq!(partial["retry_safe"], false);
    assert_eq!(
        crate::state::plan_status(&ctx, plan_id).unwrap(),
        Some(crate::state::PlanStatus::Closed)
    );
    assert_eq!(fs::read(&receipts_path).unwrap(), receipts_before);
    assert_eq!(
        crate::state::current_session(&ctx).unwrap().as_deref(),
        Some(owner)
    );
    let closed_events = fs::read(ctx.state_file("plans.jsonl")).unwrap();
    let events: Vec<Value> = String::from_utf8(closed_events.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let closes: Vec<_> = events
        .iter()
        .filter(|event| event["event"] == "close")
        .collect();
    assert_eq!(closes.len(), 1);
    assert_eq!(partial["close_event_id"], closes[0]["id"]);
    assert_eq!(partial["retirement"].is_null(), !retiring);
    fs::remove_dir(&lock_path).unwrap();
    assert!(close().unwrap_err().to_string().contains("already closed"));
    assert_eq!(
        fs::read(ctx.state_file("plans.jsonl")).unwrap(),
        closed_events
    );
    assert!(error.to_string().contains("already closed"), "{error:#}");
    assert!(
        error
            .to_string()
            .contains("Session teardown was not attempted"),
        "{error:#}"
    );
}

#[test]
fn session_teardown_failure_reports_closed_plan_and_recorded_receipt() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path()).write();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let started = crate::runtime::work::start(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "ExampleProject teardown".into(),
            body: Some("Cleanup.".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let plan_id = started["plan"]["plan_id"].as_str().unwrap();
    let sessions_before = fs::read(ctx.state_file("sessions.jsonl")).unwrap();
    let lock_path = temp
        .path()
        .join(".agent/.cache/state-locks/sessions.jsonl.lock");
    fs::remove_file(&lock_path).unwrap();
    fs::create_dir(&lock_path).unwrap();
    let error = crate::runtime::work::retire(
        &ctx,
        crate::command::WorkRetireRequest {
            plan_id: plan_id.into(),
            disposition: "obsolete".into(),
            reason: "No longer needed.".into(),
            superseded_by: None,
        },
    )
    .unwrap_err();
    let partial = error
        .downcast_ref::<crate::state::PlanClosurePartialFailure>()
        .unwrap()
        .details();
    assert_eq!(partial["plan_state"], "closed");
    assert_eq!(partial["failed_stage"], "session_teardown");
    assert_eq!(partial["receipt"]["status"], "recorded");
    assert_eq!(partial["session_teardown"]["status"], "unknown");
    let receipts: Vec<Value> = fs::read_to_string(ctx.state_file("receipts.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(
        receipts
            .iter()
            .any(|receipt| receipt["id"] == partial["receipt"]["receipt_id"]
                && receipt["plan_id"] == plan_id
                && receipt["args"]["operation"] == "plan_retire")
    );
    assert_eq!(
        fs::read(ctx.state_file("sessions.jsonl")).unwrap(),
        sessions_before
    );
    assert_eq!(
        crate::state::current_session(&ctx).unwrap().as_deref(),
        started["session"]["session_id"].as_str()
    );
}

#[test]
fn pre_closure_failure_does_not_claim_partial_completion() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = crate::runtime::work::retire(
        &ctx,
        crate::command::WorkRetireRequest {
            plan_id: "plan_missing".into(),
            disposition: "obsolete".into(),
            reason: "No longer needed.".into(),
            superseded_by: None,
        },
    )
    .unwrap_err();
    assert!(
        error
            .downcast_ref::<crate::state::PlanClosurePartialFailure>()
            .is_none()
    );
    assert!(error.to_string().contains("Plan not found"));
}
