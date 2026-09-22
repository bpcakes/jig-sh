use super::*;

#[test]
fn exact_state_backup_is_reported_as_recoverable_without_restoring_it() {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    complete_target(&ctx, &run_id);
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    let runs_path = ctx.state_file("runs.jsonl");
    let (backup_dir, _) =
        crate::state::maintenance::create_runs_backup(&ctx, &runs_path, "example-recovery", None)
            .unwrap();
    let original = fs::read(&runs_path).unwrap();
    fs::write(&runs_path, b"").unwrap();
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
    assert_eq!(finding["status"], "recoverable_from_backup");
    let backup_display = backup_dir
        .strip_prefix(ctx.root())
        .unwrap()
        .display()
        .to_string();
    assert_eq!(finding["recovery"]["kind"], "exact_source_backup");
    assert_eq!(finding["recovery"]["backup_path"], backup_display);
    assert_eq!(finding["recovery"]["original_bytes"], original.len() as u64);
    assert_eq!(finding["recovery"]["restore_replaces_whole_stream"], true);
    assert_eq!(
        finding["recovery"]["restore_eligibility"],
        "manual_preflight_required"
    );
    assert_eq!(finding["recovery"]["restore_command_available"], false);
    assert_eq!(finding["recovery"]["current_journal_events"], 0);
    assert_eq!(
        finding["recovery"]["current_journal_comparison_required"],
        false
    );
    assert_eq!(finding["history_sources"][0]["kind"], "state_backup");
    assert_eq!(output["run_linkage"]["runs"]["recoverable_from_backup"], 1);
    let kinds = recommendation_kinds(&output);
    assert!(kinds.contains(&"recover_run_history_from_backup"));
    assert!(!kinds.contains(&"preserve_unlinked_receipt_evidence"));
    let recovery = output["recommendations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "recover_run_history_from_backup")
        .unwrap();
    assert!(recovery["command"].is_null());
    assert!(
        recovery["reason"]
            .as_str()
            .unwrap()
            .contains("cannot establish restore eligibility")
    );
    assert_eq!(
        fs::read(&runs_path).unwrap(),
        b"",
        "diagnosis never restores"
    );
}

#[test]
fn backup_recovery_exposes_current_nonterminal_history_without_a_command() {
    let (_temp, ctx) = fixture_context();
    let (backed_up, backed_up_lease) = start_run(&ctx, plan(), None).unwrap();
    let backed_up_id = backed_up.result.run_id;
    complete_target(&ctx, &backed_up_id);
    complete_run(&ctx, &backed_up_id, RunConclusion::Success).unwrap();
    drop(backed_up_lease);
    let runs_path = ctx.state_file("runs.jsonl");
    crate::state::maintenance::create_runs_backup(&ctx, &runs_path, "example-recovery", None)
        .unwrap();

    let (active, _active_lease) = start_run(&ctx, plan(), None).unwrap();
    let active_id = active.result.run_id;
    let active_records = fs::read_to_string(&runs_path)
        .unwrap()
        .lines()
        .filter(|line| serde_json::from_str::<Value>(line).unwrap()["run_id"] == active_id.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&runs_path, active_records).unwrap();
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[target_receipt(
            "receipt_test",
            "jig.test",
            "api:test",
            Some(&backed_up_id),
        )],
    );

    let output = diagnose(&ctx, true);
    let finding = finding_for(&output, &backed_up_id);
    let recommendation = output["recommendations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "recover_run_history_from_backup")
        .unwrap();

    assert_eq!(finding["status"], "recoverable_from_backup");
    assert_eq!(finding["recovery"]["current_nonterminal_runs"], 1);
    assert_eq!(
        finding["recovery"]["current_journal_comparison_required"],
        true
    );
    assert!(recommendation["command"].is_null());
}

#[test]
fn backup_recovery_exposes_a_lease_only_restore_blocker() {
    let (_temp, ctx) = fixture_context();
    let (backed_up, backed_up_lease) = start_run(&ctx, plan(), None).unwrap();
    let backed_up_id = backed_up.result.run_id;
    complete_target(&ctx, &backed_up_id);
    complete_run(&ctx, &backed_up_id, RunConclusion::Success).unwrap();
    drop(backed_up_lease);
    let runs_path = ctx.state_file("runs.jsonl");
    crate::state::maintenance::create_runs_backup(&ctx, &runs_path, "example-lease-recovery", None)
        .unwrap();

    let (active, _active_lease) = start_run(&ctx, plan(), None).unwrap();
    let active_id = active.result.run_id;
    fs::write(&runs_path, b"").unwrap();
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[target_receipt(
            "receipt_test",
            "jig.test",
            "api:test",
            Some(&backed_up_id),
        )],
    );

    let output = diagnose(&ctx, true);
    let finding = finding_for(&output, &backed_up_id);

    assert_eq!(finding["status"], "recoverable_from_backup");
    assert_eq!(finding["recovery"]["current_nonterminal_runs"], 0);
    assert_eq!(finding["recovery"]["current_active_worker_leases"], 1);
    assert_eq!(
        finding["recovery"]["current_active_worker_lease_ids"],
        json!([active_id])
    );
    assert_eq!(
        finding["recovery"]["restore_eligibility"],
        "blocked_destination_activity"
    );
    assert_eq!(
        finding["recovery"]["restore_blockers"],
        json!(["active_worker_leases"])
    );
    assert_eq!(finding["recovery"]["restore_command_available"], false);
    assert!(
        finding["detail"]
            .as_str()
            .unwrap()
            .contains("1 active worker lease(s) currently block")
    );
}

#[test]
fn recovery_uses_newest_verified_complete_backup_not_a_newer_partial_one() {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    let runs_path = ctx.state_file("runs.jsonl");
    let (partial_dir, _) =
        crate::state::maintenance::create_runs_backup(&ctx, &runs_path, "newer-partial", None)
            .unwrap();
    complete_target(&ctx, &run_id);
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    let (complete_dir, _) =
        crate::state::maintenance::create_runs_backup(&ctx, &runs_path, "older-complete", None)
            .unwrap();
    set_backup_created_at(&partial_dir, 300);
    set_backup_created_at(&complete_dir, 200);
    fs::write(&runs_path, b"").unwrap();
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[target_receipt(
            "receipt_test",
            "jig.test",
            "api:test",
            Some(&run_id),
        )],
    );

    let recovered = diagnose(&ctx, true);
    let finding = finding_for(&recovered, &run_id);
    assert_eq!(finding["status"], "recoverable_from_backup");
    assert_eq!(
        finding["recovery"]["backup_path"],
        complete_dir
            .strip_prefix(ctx.root())
            .unwrap()
            .display()
            .to_string()
    );
    assert_eq!(recovered["run_linkage"]["sources"]["backups_scanned"], 2);

    fs::remove_dir_all(&complete_dir).unwrap();
    let partial_only = diagnose(&ctx, true);
    assert_eq!(
        finding_for(&partial_only, &run_id)["status"],
        "unverifiable"
    );
    assert!(finding_for(&partial_only, &run_id)["recovery"].is_null());
}

#[cfg(unix)]
#[test]
fn inaccessible_backup_is_unverifiable_until_access_is_restored() {
    use std::os::unix::fs::PermissionsExt;

    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    complete_target(&ctx, &run_id);
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    let runs_path = ctx.state_file("runs.jsonl");
    let (backup_dir, _) = crate::state::maintenance::create_runs_backup(
        &ctx,
        &runs_path,
        "inaccessible-history",
        None,
    )
    .unwrap();
    fs::write(&runs_path, b"").unwrap();
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[target_receipt(
            "receipt_test",
            "jig.test",
            "api:test",
            Some(&run_id),
        )],
    );

    let backup_before = snapshot(&backup_dir);
    fs::set_permissions(&backup_dir, fs::Permissions::from_mode(0o600)).unwrap();
    let inaccessible = state_diagnose(&ctx, StateDiagnoseRequest { deep: true });
    fs::set_permissions(&backup_dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(snapshot(&backup_dir), backup_before);

    let linkage = &inaccessible["run_linkage"];
    assert_eq!(linkage["verdict"], "findings");
    assert_eq!(linkage["complete"], false);
    assert_eq!(linkage["runs"]["unverifiable"], 1);
    assert_eq!(linkage["runs"]["missing"], 0);
    assert_eq!(linkage["sources"]["backups_scanned"], 0);
    assert_eq!(linkage["sources"]["error_count"], 1);
    assert_string_array_contains(&linkage["sources"]["errors"], "Permission denied");
    assert_eq!(
        finding_for(&inaccessible, &run_id)["status"],
        "unverifiable"
    );

    let restored = diagnose(&ctx, true);
    assert_eq!(restored["run_linkage"]["verdict"], "findings");
    assert_eq!(restored["run_linkage"]["complete"], true);
    assert_eq!(
        finding_for(&restored, &run_id)["status"],
        "recoverable_from_backup"
    );
    assert_eq!(restored["run_linkage"]["sources"]["backups_scanned"], 1);
    assert_eq!(restored["run_linkage"]["sources"]["error_count"], 0);
}

#[test]
fn recovery_skips_newer_backup_with_wrong_source_path() {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    complete_target(&ctx, &run_id);
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    let runs_path = ctx.state_file("runs.jsonl");
    let (older_valid, _) =
        crate::state::maintenance::create_runs_backup(&ctx, &runs_path, "older-valid", None)
            .unwrap();
    let (newer_wrong_source, _) =
        crate::state::maintenance::create_runs_backup(&ctx, &runs_path, "newer-wrong-source", None)
            .unwrap();
    set_backup_created_at(&older_valid, 200);
    set_backup_created_at(&newer_wrong_source, 300);
    let manifest_path = newer_wrong_source.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["source_path"] = json!(".agent/state/not-runs.jsonl");
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    fs::write(&runs_path, b"").unwrap();
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
    assert!(finding["recovery"].is_null());
    assert_eq!(output["run_linkage"]["sources"]["backups_scanned"], 2);
    assert_string_array_contains(
        &output["run_linkage"]["sources"]["errors"],
        "unsupported stream runs at .agent/state/not-runs.jsonl",
    );
}

#[test]
fn recovery_skips_newer_backup_with_unrelated_invalid_lifecycle() {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    complete_target(&ctx, &run_id);
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    let runs_path = ctx.state_file("runs.jsonl");
    let (older_valid, _) =
        crate::state::maintenance::create_runs_backup(&ctx, &runs_path, "older-valid", None)
            .unwrap();
    let mut bytes = fs::read(&runs_path).unwrap();
    bytes.extend(
        serde_json::to_vec(&json!({
            "id": "run_event_unrelated_completed",
            "run_id": "run_unrelated",
            "event": "completed",
            "timestamp_ms": 10,
            "conclusion": "success",
        }))
        .unwrap(),
    );
    bytes.push(b'\n');
    fs::write(&runs_path, bytes).unwrap();
    let (newer_invalid, _) = crate::state::maintenance::create_runs_backup(
        &ctx,
        &runs_path,
        "newer-invalid-lifecycle",
        None,
    )
    .unwrap();
    set_backup_created_at(&older_valid, 200);
    set_backup_created_at(&newer_invalid, 300);
    fs::write(&runs_path, b"").unwrap();
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
    assert!(finding["recovery"].is_null());
    assert_eq!(output["run_linkage"]["sources"]["backups_scanned"], 2);
    assert_string_array_contains(
        &output["run_linkage"]["sources"]["errors"],
        "run 'run_unrelated' has a completed event before queued",
    );
}

#[test]
fn tampered_backup_and_corrupt_archive_make_history_unverifiable_not_missing() {
    let (_temp, ctx) = fixture_context();
    write_orphan_batch(&ctx);
    let archives = ctx.root().join(".agent/.cache/state-archives");
    fs::create_dir_all(&archives).unwrap();
    fs::write(archives.join("runs-before-1-EXAMPLE.jsonl.gz"), b"not gzip").unwrap();
    fs::write(
        archives.join("receipts-before-1-EXAMPLE.jsonl.gz"),
        b"ignored",
    )
    .unwrap();

    let output = diagnose(&ctx, true);

    let finding = finding_for(&output, RUN_A);
    assert_eq!(finding["status"], "unverifiable");
    assert!(
        finding["detail"]
            .as_str()
            .unwrap()
            .contains("could not be verified")
    );
    assert_eq!(output["run_linkage"]["sources"]["archives_scanned"], 1);
    assert_eq!(output["run_linkage"]["sources"]["error_count"], 1);
    assert_eq!(output["run_linkage"]["runs"]["unverifiable"], 1);
    assert_eq!(output["run_linkage"]["runs"]["missing"], 0);

    // A backup whose bytes no longer match its manifest is not an exact source.
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    drop(lease);
    let runs_path = ctx.state_file("runs.jsonl");
    let (backup_dir, _) =
        crate::state::maintenance::create_runs_backup(&ctx, &runs_path, "tampered", None).unwrap();
    fs::write(&runs_path, b"").unwrap();
    fs::remove_dir_all(&archives).unwrap();
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(backup_dir.join("manifest.json")).unwrap()).unwrap();
    manifest["original_sha256"] = json!("sha256:not-the-real-digest");
    fs::write(
        backup_dir.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
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
    assert_eq!(output["run_linkage"]["sources"]["backups_scanned"], 1);
    assert_eq!(output["run_linkage"]["sources"]["error_count"], 1);
    assert!(
        output["run_linkage"]["sources"]["errors"][0]
            .as_str()
            .unwrap()
            .contains("does not match its manifest")
    );
    assert!(!recommendation_kinds(&output).contains(&"recover_run_history_from_backup"));
}

#[cfg(unix)]
#[test]
fn symlinked_backup_directory_makes_missing_history_unverifiable() {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    complete_target(&ctx, &run_id);
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    let runs_path = ctx.state_file("runs.jsonl");
    let (backup_dir, _) =
        crate::state::maintenance::create_runs_backup(&ctx, &runs_path, "symlinked", None).unwrap();
    let durable_backup = ctx.root().join("durable-history/state-backup");
    fs::create_dir_all(durable_backup.parent().unwrap()).unwrap();
    fs::rename(&backup_dir, &durable_backup).unwrap();
    std::os::unix::fs::symlink(&durable_backup, &backup_dir).unwrap();
    fs::write(&runs_path, b"").unwrap();
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

    assert_eq!(linkage["verdict"], "findings");
    assert_eq!(linkage["complete"], false);
    assert_eq!(linkage["sources"]["symlinks_skipped"], 1);
    assert_eq!(linkage["sources"]["backups_scanned"], 0);
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
fn symlinked_backup_root_makes_missing_history_unverifiable() {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    complete_target(&ctx, &run_id);
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    let runs_path = ctx.state_file("runs.jsonl");
    crate::state::maintenance::create_runs_backup(&ctx, &runs_path, "symlinked-root", None)
        .unwrap();
    let backup_root = ctx.root().join(".agent/.cache/state-backups");
    let durable_root = ctx.root().join("durable-history/state-backups");
    fs::create_dir_all(durable_root.parent().unwrap()).unwrap();
    fs::rename(&backup_root, &durable_root).unwrap();
    std::os::unix::fs::symlink(&durable_root, &backup_root).unwrap();
    fs::write(&runs_path, b"").unwrap();
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

    assert_eq!(linkage["verdict"], "findings");
    assert_eq!(linkage["complete"], false);
    assert_eq!(linkage["sources"]["symlinks_skipped"], 1);
    assert_eq!(linkage["sources"]["backups_scanned"], 0);
    assert_eq!(linkage["runs"]["unverifiable"], 1);
    assert_eq!(linkage["runs"]["missing"], 0);
    assert_eq!(finding["status"], "unverifiable");
}
