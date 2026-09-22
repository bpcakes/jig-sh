use super::*;

#[test]
fn an_arbitrary_event_with_a_matching_run_id_is_not_a_verified_lifecycle() {
    let (_temp, ctx) = fixture_context();
    write_orphan_batch(&ctx);
    write_records(
        &ctx.state_file("runs.jsonl"),
        &[json!({"id": "run_event_note", "run_id": RUN_A, "event": "note", "timestamp_ms": 1})],
    );

    let output = diagnose(&ctx, true);

    let finding = finding_for(&output, RUN_A);
    assert_eq!(finding["status"], "unverifiable");
    assert_eq!(finding["journal_events"], 1);
    assert_eq!(output["run_linkage"]["runs"]["unverifiable"], 1);
    assert_eq!(output["run_linkage"]["journal"]["unrecognized_events"], 1);
}

#[test]
fn a_complete_lifecycle_with_an_unknown_event_is_unverifiable() {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    complete_target(&ctx, &run_id);
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    append_record(
        &ctx.state_file("runs.jsonl"),
        &json!({
            "id": "run_event_future_annotation",
            "run_id": run_id,
            "event": "future_annotation",
            "timestamp_ms": 4,
        }),
    );
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[target_receipt(
            "receipt_test",
            "jig.test",
            "api:test",
            Some(&run_id),
        )],
    );

    let output = diagnose(&ctx, true);
    let finding = finding_for(&output, &run_id);

    assert_eq!(finding["status"], "unverifiable");
    assert_eq!(output["run_linkage"]["journal"]["authority"], "damaged");
    assert_eq!(output["run_linkage"]["journal"]["unrecognized_events"], 1);
    assert_eq!(output["run_linkage"]["complete"], false);
}

#[test]
fn unrelated_invalid_active_lifecycle_prevents_a_clean_verdict() {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    complete_target(&ctx, &run_id);
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    append_record(
        &ctx.state_file("runs.jsonl"),
        &json!({
            "id": "run_event_unrelated_completed",
            "run_id": "run_unrelated",
            "event": "completed",
            "timestamp_ms": 5,
            "conclusion": "success",
        }),
    );
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[target_receipt(
            "receipt_test",
            "jig.test",
            "api:test",
            Some(&run_id),
        )],
    );

    let output = diagnose(&ctx, true);
    let linkage = &output["run_linkage"];

    assert_eq!(linkage["verdict"], "incomplete");
    assert_eq!(linkage["complete"], false);
    assert_eq!(linkage["runs"]["completed"], 1);
    assert_eq!(linkage["finding_count"], 0);
    assert_eq!(linkage["journal"]["inconsistent_lifecycles"], 1);
    assert_string_array_contains(
        &linkage["incomplete_reasons"],
        "lifecycle(s) rejected by authoritative validation",
    );
}

#[test]
fn unrelated_invalid_archive_lifecycle_makes_history_unverifiable() {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    complete_target(&ctx, &run_id);
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    append_record(
        &ctx.state_file("runs.jsonl"),
        &json!({
            "id": "run_event_unrelated_completed",
            "run_id": "run_unrelated",
            "event": "completed",
            "timestamp_ms": 5,
            "conclusion": "success",
        }),
    );
    let archive = ctx
        .root()
        .join(".agent/.cache/state-archives/runs-before-10-EXAMPLE.jsonl.gz");
    write_gzip(&archive, &fs::read(ctx.state_file("runs.jsonl")).unwrap());
    fs::write(ctx.state_file("runs.jsonl"), b"").unwrap();
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[target_receipt(
            "receipt_test",
            "jig.test",
            "api:test",
            Some(&run_id),
        )],
    );

    let output = diagnose(&ctx, true);
    let finding = finding_for(&output, &run_id);

    assert_eq!(finding["status"], "unverifiable");
    assert_eq!(output["run_linkage"]["sources"]["archives_scanned"], 1);
    assert_eq!(output["run_linkage"]["sources"]["error_count"], 1);
    assert_string_array_contains(
        &output["run_linkage"]["sources"]["errors"],
        "run 'run_unrelated' has a completed event before queued",
    );
    assert_eq!(output["run_linkage"]["complete"], false);
}

#[test]
fn unknown_event_in_a_second_gzip_member_makes_history_unverifiable() {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    complete_target(&ctx, &run_id);
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    let archive = ctx
        .root()
        .join(".agent/.cache/state-archives/runs-before-10-EXAMPLE.jsonl.gz");
    write_gzip(&archive, &fs::read(ctx.state_file("runs.jsonl")).unwrap());
    let unknown_event = serde_json::to_vec(&json!({
        "id": "run_event_future_annotation",
        "run_id": run_id,
        "event": "future_annotation",
        "timestamp_ms": 5,
    }))
    .unwrap();
    let mut second_member = unknown_event;
    second_member.push(b'\n');
    append_gzip_member(&archive, &second_member);
    fs::write(ctx.state_file("runs.jsonl"), b"").unwrap();
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[target_receipt(
            "receipt_test",
            "jig.test",
            "api:test",
            Some(&run_id),
        )],
    );

    let output = diagnose(&ctx, true);
    let linkage = &output["run_linkage"];
    let finding = finding_for(&output, &run_id);

    assert_eq!(finding["status"], "unverifiable");
    assert_eq!(linkage["runs"]["archived_verified"], 0);
    assert_eq!(linkage["sources"]["archives_scanned"], 1);
    assert_eq!(linkage["sources"]["error_count"], 1);
    assert_string_array_contains(
        &linkage["sources"]["errors"],
        "trailing data after the first member",
    );
    assert_eq!(linkage["complete"], false);
}

#[test]
fn events_before_queued_are_reported_as_inconsistent_not_healthy() {
    let (_temp, ctx) = fixture_context();
    write_orphan_batch(&ctx);
    write_records(
        &ctx.state_file("runs.jsonl"),
        &[
            json!({"id": "run_event_x", "run_id": RUN_A, "event": "completed", "timestamp_ms": 1, "conclusion": "success"}),
        ],
    );

    let output = diagnose(&ctx, true);

    let finding = finding_for(&output, RUN_A);
    assert_eq!(finding["status"], "inconsistent");
    assert_eq!(
        finding["journal_anomalies"],
        json!([format!("run '{RUN_A}' has a completed event before queued")])
    );
    assert_eq!(output["run_linkage"]["runs"]["inconsistent"], 1);
    assert!(recommendation_kinds(&output).contains(&"preserve_unlinked_receipt_evidence"));
}

#[test]
fn live_and_completed_lifecycles_written_by_the_runtime_are_not_orphans() {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    mark_run_running(&ctx, &run_id).unwrap();
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[
            target_receipt("receipt_test", "jig.test", "api:test", Some(&run_id)),
            work_check_targets_receipt("receipt_batch", &[("api:test", "receipt_test", &run_id)]),
        ],
    );

    let active = diagnose(&ctx, true);
    assert_eq!(active["run_linkage"]["verdict"], "clean");
    assert_eq!(active["run_linkage"]["runs"]["active"], 1);
    assert_eq!(active["run_linkage"]["finding_count"], 0);
    assert_eq!(active["integrity"]["run_linkage"], "clean");
    assert!(recommendation_kinds(&active).is_empty());

    complete_target(&ctx, &run_id);
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    let completed = diagnose(&ctx, true);
    assert_eq!(completed["run_linkage"]["verdict"], "clean");
    assert_eq!(completed["run_linkage"]["runs"]["completed"], 1);
    assert_eq!(completed["run_linkage"]["journal"]["lifecycles"], 1);
}

#[test]
fn verified_archived_history_is_not_an_orphan() {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    mark_run_running(&ctx, &run_id).unwrap();
    complete_target(&ctx, &run_id);
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    let archived = crate::state::state_archive(
        &ctx,
        crate::command::StateArchiveRequest {
            before: u64::MAX.to_string(),
            include_runs: true,
            dry_run: false,
        },
    )
    .unwrap();
    assert_eq!(archived["runs_archived"], 1);
    assert_eq!(fs::read(ctx.state_file("runs.jsonl")).unwrap(), b"");
    // Receipts written after the archive keep referencing the archived run.
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[target_receipt(
            "receipt_test",
            "jig.test",
            "api:test",
            Some(&run_id),
        )],
    );

    let output = diagnose(&ctx, true);

    let linkage = &output["run_linkage"];
    assert_eq!(linkage["verdict"], "clean");
    assert_eq!(linkage["runs"]["archived_verified"], 1);
    assert_eq!(linkage["runs"]["missing"], 0);
    assert_eq!(linkage["sources"]["archives_scanned"], 1);
    assert_eq!(
        linkage["sources"]["backups_scanned"], 0,
        "a verified archive resolves the wanted run before backup scanning"
    );
    assert_eq!(linkage["sources"]["error_count"], 0);
    assert!(recommendation_kinds(&output).contains(&"review_maintenance_cache"));
    assert!(!recommendation_kinds(&output).contains(&"preserve_unlinked_receipt_evidence"));
}

#[cfg(unix)]
#[test]
fn symlinked_run_archive_makes_missing_history_unverifiable() {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    complete_target(&ctx, &run_id);
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    let run_history = fs::read(ctx.state_file("runs.jsonl")).unwrap();
    fs::write(ctx.state_file("runs.jsonl"), b"").unwrap();
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[target_receipt(
            "receipt_test",
            "jig.test",
            "api:test",
            Some(&run_id),
        )],
    );
    let durable_archive = ctx.root().join("durable-history/runs.jsonl.gz");
    write_gzip(&durable_archive, &run_history);
    let archive_link = ctx
        .root()
        .join(".agent/.cache/state-archives/runs-before-10-EXAMPLE.jsonl.gz");
    fs::create_dir_all(archive_link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&durable_archive, archive_link).unwrap();

    let output = diagnose(&ctx, true);
    let linkage = &output["run_linkage"];
    let finding = finding_for(&output, &run_id);

    assert_eq!(linkage["verdict"], "findings");
    assert_eq!(linkage["complete"], false);
    assert_eq!(linkage["sources"]["symlinks_skipped"], 1);
    assert_eq!(linkage["sources"]["archives_scanned"], 0);
    assert_eq!(linkage["runs"]["unverifiable"], 1);
    assert_eq!(linkage["runs"]["missing"], 0);
    assert_eq!(finding["status"], "unverifiable");
    assert_string_array_contains(
        &linkage["incomplete_reasons"],
        "symlinked local run history candidate(s) were skipped",
    );
}

#[cfg(unix)]
#[test]
fn symlinked_run_archive_root_makes_missing_history_unverifiable() {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    complete_target(&ctx, &run_id);
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    let run_history = fs::read(ctx.state_file("runs.jsonl")).unwrap();
    fs::write(ctx.state_file("runs.jsonl"), b"").unwrap();
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[target_receipt(
            "receipt_test",
            "jig.test",
            "api:test",
            Some(&run_id),
        )],
    );
    let durable_root = ctx.root().join("durable-history/state-archives");
    write_gzip(
        &durable_root.join("runs-before-10-EXAMPLE.jsonl.gz"),
        &run_history,
    );
    let archive_root = ctx.root().join(".agent/.cache/state-archives");
    fs::create_dir_all(archive_root.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&durable_root, &archive_root).unwrap();

    let output = diagnose(&ctx, true);
    let linkage = &output["run_linkage"];
    let finding = finding_for(&output, &run_id);

    assert_eq!(linkage["verdict"], "findings");
    assert_eq!(linkage["complete"], false);
    assert_eq!(linkage["sources"]["symlinks_skipped"], 1);
    assert_eq!(linkage["sources"]["archives_scanned"], 0);
    assert_eq!(linkage["runs"]["unverifiable"], 1);
    assert_eq!(linkage["runs"]["missing"], 0);
    assert_eq!(finding["status"], "unverifiable");
}

#[cfg(unix)]
#[test]
fn unrelated_archive_symlink_does_not_make_missing_history_incomplete() {
    let (_temp, ctx) = fixture_context();
    write_orphan_batch(&ctx);
    let durable_receipts = ctx.root().join("durable-history/receipts.jsonl.gz");
    write_gzip(&durable_receipts, b"not relevant to run history");
    let receipt_archive_link = ctx
        .root()
        .join(".agent/.cache/state-archives/receipts-before-10-EXAMPLE.jsonl.gz");
    fs::create_dir_all(receipt_archive_link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&durable_receipts, receipt_archive_link).unwrap();

    let output = diagnose(&ctx, true);
    let linkage = &output["run_linkage"];

    assert_eq!(linkage["complete"], true);
    assert_eq!(linkage["sources"]["symlinks_skipped"], 0);
    assert_eq!(linkage["runs"]["missing"], 1);
}
