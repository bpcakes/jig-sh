use super::*;

#[test]
fn unknown_future_events_are_ignored() {
    let (_temp, ctx) = context();
    let (started, _lease) = start_run(&ctx, plan(), None).unwrap();
    append_event(
        &ctx,
        RunEventRecord {
            id: "run_event_future".into(),
            run_id: started.result.run_id.clone(),
            event: "future_annotation".into(),
            timestamp_ms: now_ms(),
            work_plan_id: None,
            plan: None,
            target: None,
            result: None,
            conclusion: None,
        },
    )
    .unwrap();

    assert_eq!(
        run_by_id(&ctx, &started.result.run_id)
            .unwrap()
            .result
            .status,
        RunStatus::Queued
    );
}

#[test]
fn archive_retains_unknown_only_future_runs_without_treating_them_as_nonterminal() {
    let (_temp, ctx) = context();
    let run_id = "run_future_only";
    append_event(
        &ctx,
        RunEventRecord {
            id: "run_event_future_only".into(),
            run_id: run_id.into(),
            event: "future_annotation".into(),
            timestamp_ms: 1,
            work_plan_id: None,
            plan: None,
            target: None,
            result: None,
            conclusion: None,
        },
    )
    .unwrap();

    let preview = runs_archive(&ctx, &u64::MAX.to_string(), true).unwrap();
    assert_eq!(preview["runs_archived"], 0);
    assert_eq!(preview["runs_retained"], 1);

    let applied = runs_archive(&ctx, &u64::MAX.to_string(), false).unwrap();
    assert_eq!(applied["runs_archived"], 0);
    assert_eq!(applied["runs_retained"], 1);
    assert!(
        fs::read_to_string(ctx.state_file(RUNS_FILE))
            .unwrap()
            .contains(run_id)
    );
}

#[test]
fn corrupt_known_lifecycle_is_rejected() {
    let (_temp, ctx) = context();
    let (started, _lease) = start_run(&ctx, plan(), None).unwrap();
    complete_run(&ctx, &started.result.run_id, RunConclusion::Success).unwrap();

    let error = run_by_id(&ctx, &started.result.run_id).unwrap_err();
    assert!(error.to_string().contains("before every target"));
}
