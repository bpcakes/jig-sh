use super::*;

#[test]
fn damaged_journal_reports_absent_lifecycles_as_unverifiable() {
    let (_temp, ctx) = fixture_context();
    write_orphan_batch(&ctx);
    fs::write(ctx.state_file("runs.jsonl"), b"{\"broken\":\n").unwrap();

    let output = diagnose(&ctx, true);

    assert_eq!(output["run_linkage"]["journal"]["authority"], "damaged");
    assert_eq!(output["run_linkage"]["journal"]["malformed_records"], 1);
    assert_eq!(finding_for(&output, RUN_A)["status"], "unverifiable");
    assert_eq!(output["run_linkage"]["complete"], false);
    assert!(
        output["run_linkage"]["incomplete_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason.as_str().unwrap().contains("run journal contains"))
    );
    assert!(recommendation_kinds(&output).contains(&"repair_malformed_state"));
}

#[test]
fn structurally_unrecognized_run_records_make_linkage_incomplete() {
    let (_temp, ctx) = fixture_context();
    write_orphan_batch(&ctx);
    write_records(
        &ctx.state_file("runs.jsonl"),
        &[json!({"id": "run_event_incomplete", "run_id": RUN_A, "event": "queued"})],
    );

    let output = diagnose(&ctx, true);

    assert_eq!(output["run_linkage"]["journal"]["authority"], "damaged");
    assert_eq!(output["run_linkage"]["journal"]["unrecognized_records"], 1);
    assert_eq!(output["run_linkage"]["complete"], false);
    assert_eq!(finding_for(&output, RUN_A)["status"], "unverifiable");
}

#[test]
fn incomplete_scans_never_yield_a_clean_linkage_verdict() {
    let (_temp, ctx) = fixture_context();
    // A receipt whose id is not a string cannot be analyzed for references.
    fs::write(
        ctx.state_file("receipts.jsonl"),
        b"{\"id\":5,\"run_id\":\"run_unknown\"}\n",
    )
    .unwrap();

    let unanalyzed = diagnose(&ctx, true);
    assert_eq!(unanalyzed["run_linkage"]["verdict"], "incomplete");
    assert_eq!(unanalyzed["run_linkage"]["complete"], false);
    assert_eq!(unanalyzed["run_linkage"]["finding_count"], 0);
    assert_eq!(
        unanalyzed["streams"]["receipts"]["deep_analysis_error_count"],
        1
    );
    assert!(
        unanalyzed["run_linkage"]["incomplete_reasons"][0]
            .as_str()
            .unwrap()
            .contains("1 receipt records could not be analyzed")
    );
    assert!(recommendation_kinds(&unanalyzed).contains(&"complete_run_linkage_check"));

    // An unreadable run journal is a failed scan, not an empty one.
    fs::write(ctx.state_file("receipts.jsonl"), b"").unwrap();
    fs::create_dir_all(ctx.state_file("runs.jsonl")).unwrap();
    let unreadable = diagnose(&ctx, true);
    assert_eq!(unreadable["run_linkage"]["verdict"], "incomplete");
    assert_eq!(
        unreadable["run_linkage"]["journal"]["authority"],
        "unreadable"
    );
    assert!(unreadable["streams"]["runs"]["scan_error"].is_string());
}
