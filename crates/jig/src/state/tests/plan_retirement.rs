use super::*;

fn open_plan(ctx: &RepoContext, title: &str) -> String {
    plans_open(
        ctx,
        PlanOpenRequest {
            title: title.into(),
            body: Some("Initial body".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap()["plan_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn retire_request(plan_id: &str, disposition: &'static str) -> PlanRetireRequest {
    PlanRetireRequest {
        plan_id: plan_id.into(),
        disposition,
        reason: "Superseded by the ExampleProject redesign.".into(),
        superseded_by: None,
    }
}

#[test]
fn plans_retire_records_structured_disposition_and_reports_lifecycle() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_plan(&ctx, "Retire me");

    let mut request = retire_request(&plan_id, "superseded");
    request.superseded_by = Some("plan_example_002".into());
    let output = plans_retire(&ctx, request).unwrap();

    let lifecycle = plan_lifecycle(&ctx, &plan_id).unwrap().unwrap();
    let retirement = lifecycle.retirement.unwrap();
    assert_eq!(output["ok"], true);
    assert_eq!(lifecycle.status, PlanStatus::Closed);
    assert_eq!(retirement.disposition, "superseded");
    assert_eq!(
        retirement.reason,
        "Superseded by the ExampleProject redesign."
    );
    assert_eq!(
        retirement.superseded_by.as_deref(),
        Some("plan_example_002")
    );
}

#[test]
fn plans_close_keeps_its_historical_meaning_with_no_retirement() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_plan(&ctx, "Close me");

    plans_close(
        &ctx,
        PlanCloseRequest {
            plan_id: plan_id.clone(),
            resolution: Some("done".into()),
        },
    )
    .unwrap();

    let lifecycle = plan_lifecycle(&ctx, &plan_id).unwrap().unwrap();
    assert_eq!(lifecycle.status, PlanStatus::Closed);
    assert!(lifecycle.retirement.is_none());
}

#[test]
fn plan_events_written_before_retirement_still_decode() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    let path = ctx.state_file("plans.jsonl");
    // Exactly the shape an older runtime appended: no `retirement` key at all.
    fs::write(
        &path,
        concat!(
            r#"{"id":"1","plan_id":"plan_legacy","event":"open","timestamp_ms":1,"title":"Legacy","body_path":null,"resolution":null}"#,
            "\n",
            r#"{"id":"2","plan_id":"plan_legacy","event":"close","timestamp_ms":2,"title":null,"body_path":null,"resolution":"done"}"#,
            "\n",
            // A disposition this runtime does not know must still decode.
            r#"{"id":"3","plan_id":"plan_future","event":"open","timestamp_ms":3,"title":"Future","body_path":null,"resolution":null}"#,
            "\n",
            r#"{"id":"4","plan_id":"plan_future","event":"close","timestamp_ms":4,"title":null,"body_path":null,"resolution":null,"retirement":{"disposition":"deferred","reason":"Later."}}"#,
            "\n",
        ),
    )
    .unwrap();

    let legacy = plan_lifecycle(&ctx, "plan_legacy").unwrap().unwrap();
    let future = plan_lifecycle(&ctx, "plan_future").unwrap().unwrap();
    let future_retirement = future.retirement.unwrap();

    assert_eq!(legacy.status, PlanStatus::Closed);
    assert!(legacy.retirement.is_none());
    assert_eq!(future.status, PlanStatus::Closed);
    assert_eq!(future_retirement.disposition, "deferred");
    assert_eq!(future_retirement.reason, "Later.");
    assert!(future_retirement.superseded_by.is_none());
}

#[test]
fn retiring_a_plan_round_trips_through_the_append_only_stream() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_plan(&ctx, "Round trip");
    plans_retire(&ctx, retire_request(&plan_id, "obsolete")).unwrap();

    let events = read_jsonl::<PlanEvent>(&ctx.state_file("plans.jsonl")).unwrap();
    let reserialized = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let stored = fs::read_to_string(ctx.state_file("plans.jsonl")).unwrap();

    assert_eq!(reserialized, stored.trim_end());
}

#[test]
fn plans_retire_rejects_an_active_linked_repository_run() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_plan(&ctx, "Run before retire");
    let run_plan = jig_contract::RunPlan::new(
        "run-plan_empty",
        "sha256:config",
        jig_contract::SourceIdentity::new(Some("abc".into()), "sha256:worktree"),
        Vec::new(),
        Vec::new(),
    );
    let (run, lease) = start_run(&ctx, run_plan, Some(plan_id.clone())).unwrap();

    let error = plans_retire(&ctx, retire_request(&plan_id, "cancelled"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("active linked repository runs"), "{error}");
    assert_eq!(plan_status(&ctx, &plan_id).unwrap(), Some(PlanStatus::Open));

    complete_run(
        &ctx,
        &run.result.run_id,
        jig_contract::RunConclusion::Success,
    )
    .unwrap();
    drop(lease);
    plans_retire(&ctx, retire_request(&plan_id, "cancelled")).unwrap();
    assert_eq!(
        plan_status(&ctx, &plan_id).unwrap(),
        Some(PlanStatus::Closed)
    );
}

#[test]
fn a_run_cannot_start_against_a_plan_that_retirement_already_closed() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_plan(&ctx, "Retire before run");
    plans_retire(&ctx, retire_request(&plan_id, "cancelled")).unwrap();

    let run_plan = jig_contract::RunPlan::new(
        "run-plan_empty",
        "sha256:config",
        jig_contract::SourceIdentity::new(Some("abc".into()), "sha256:worktree"),
        Vec::new(),
        Vec::new(),
    );
    let error = match start_run(&ctx, run_plan, Some(plan_id.clone())) {
        Ok(_) => panic!("a retired plan accepted a new linked run"),
        Err(error) => error.to_string(),
    };

    assert!(
        error.contains(&format!("Plan is already closed: {plan_id}")),
        "{error}"
    );
}

#[test]
fn plan_owner_session_is_proven_by_the_plan_open_receipt() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let unowned = open_plan(&ctx, "Opened without a session");
    let session_id = session_start(&ctx).unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let owned = open_plan(&ctx, "Opened inside a session");

    assert_eq!(plan_owner_session(&ctx, &unowned).unwrap(), None);
    assert_eq!(plan_owner_session(&ctx, &owned).unwrap(), Some(session_id));
    assert_eq!(plan_owner_session(&ctx, "plan_missing").unwrap(), None);
}
