use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

use super::*;
use crate::test_env::TestRepoBuilder;

fn fixture_context(root: &Path) -> RepoContext {
    TestRepoBuilder::new(root).write();
    RepoContext::load_from_root(root.to_path_buf()).unwrap()
}

#[test]
fn diagnose_missing_state_is_strictly_read_only() {
    let temp = tempdir().unwrap();
    let ctx = fixture_context(temp.path());
    let before = fixture_paths(temp.path());

    let output = state_diagnose(&ctx, StateDiagnoseRequest { deep: true });

    assert_eq!(output["state_dir_exists"], false);
    assert_eq!(output["totals"]["stream_bytes"], 0);
    assert_eq!(output["sessions"]["projected_shallow_bytes"], 0);
    assert_eq!(before, fixture_paths(temp.path()));
    assert!(!ctx.state_dir().exists());
    assert!(!temp.path().join(".git").exists());
}

#[test]
fn diagnose_reports_exact_stream_and_deep_storage_facts() {
    let temp = tempdir().unwrap();
    let ctx = fixture_context(temp.path());
    fs::create_dir_all(ctx.state_dir().join("archive/nested")).unwrap();

    let nested_summary = r#"{"recent_sessions":[{"summary":"braces } ] in a string"}]}"#;
    let recursive = [
            r#"{"id":"outer","session_id":"s2","event":"start","timestamp_ms":2,"summary":{"recent_sessions":[{"id":"inner","session_id":"s1","event":"start","timestamp_ms":1,"summary":"#,
            nested_summary,
            r#"}]}}"#,
        ]
        .concat();
    let ordinary = r#"{"id":"end","session_id":"s1","event":"end","timestamp_ms":3}"#;
    let sessions = format!("{recursive}\n{ordinary}\n");
    fs::write(ctx.state_file("sessions.jsonl"), sessions.as_bytes()).unwrap();

    let malformed_plans = b"{}\n{\"broken\":";
    fs::write(ctx.state_file("plans.jsonl"), malformed_plans).unwrap();

    let args = r#"{"command":"x"}"#;
    let stdout = r#""out""#;
    let stderr = r#""err""#;
    let evidence = r#"{"ok":true}"#;
    let paths = r#"["a","b"]"#;
    let diff = r#"{"files":2,"insertions":1,"deletions":0}"#;
    let receipt = [
        r#"{"id":"r","args":"#,
        args,
        r#","stdout_preview":"#,
        stdout,
        r#","stderr_preview":"#,
        stderr,
        r#","evidence":"#,
        evidence,
        r#","changed_paths":"#,
        paths,
        r#","diff_stat":"#,
        diff,
        "}",
    ]
    .concat();
    fs::write(
        ctx.state_file("receipts.jsonl"),
        format!("{receipt}\n").as_bytes(),
    )
    .unwrap();
    fs::write(ctx.state_dir().join("archive/old.jsonl"), b"abc").unwrap();
    fs::write(ctx.state_dir().join("archive/nested/older.jsonl"), b"12345").unwrap();

    let output = state_diagnose(&ctx, StateDiagnoseRequest { deep: true });

    assert_eq!(
        output["streams"]["sessions"]["bytes"],
        sessions.len() as u64
    );
    assert_eq!(output["streams"]["sessions"]["records"], 2);
    assert_eq!(
        output["streams"]["sessions"]["max_line_bytes"],
        recursive.len() as u64 + 1
    );
    assert_eq!(
        output["streams"]["sessions"]["max_record_bytes"],
        recursive.len() as u64
    );
    assert_eq!(output["streams"]["sessions"]["max_record_line"], 1);
    assert_eq!(output["streams"]["plans"]["records"], 2);
    assert_eq!(output["streams"]["plans"]["malformed_records"], 1);
    assert_eq!(
        output["streams"]["plans"]["malformed_record_samples"][0]["line"],
        2
    );
    assert_eq!(output["streams"]["plans"]["torn_tail"], true);

    let compacted_recursive = recursive.replace(
        &format!(r#""summary":{nested_summary}"#),
        r#""summary":null"#,
    );
    let expected_projection = compacted_recursive.len() + 1 + ordinary.len() + 1;
    assert_eq!(output["sessions"]["recursive_session_records"], 1);
    assert_eq!(output["sessions"]["recursive_summary_values"], 1);
    assert_eq!(
        output["sessions"]["projected_shallow_bytes"],
        expected_projection as u64
    );
    assert_eq!(
        output["sessions"]["estimated_reclaimable_bytes"],
        (sessions.len() - expected_projection) as u64
    );

    assert_eq!(output["receipts"]["analyzed_records"], 1);
    assert_eq!(output["receipts"]["args_bytes"], args.len() as u64);
    assert_eq!(
        output["receipts"]["stdout_preview_bytes"],
        stdout.len() as u64
    );
    assert_eq!(
        output["receipts"]["stderr_preview_bytes"],
        stderr.len() as u64
    );
    assert_eq!(
        output["receipts"]["output_preview_bytes"],
        (stdout.len() + stderr.len()) as u64
    );
    assert_eq!(output["receipts"]["evidence_bytes"], evidence.len() as u64);
    assert_eq!(
        output["receipts"]["changed_paths_bytes"],
        paths.len() as u64
    );
    assert_eq!(output["receipts"]["diff_stat_bytes"], diff.len() as u64);
    assert_eq!(output["legacy_archive"]["files"], 2);
    assert_eq!(output["legacy_archive"]["bytes"], 8);
    assert_eq!(output["totals"]["legacy_archive_bytes"], 8);
    assert_eq!(
        output["recommendations"][0]["command"],
        "jig state compact sessions --dry-run"
    );
}

#[test]
fn diagnose_reports_tracking_ignore_and_union_merge_facts() {
    let temp = tempdir().unwrap();
    let ctx = fixture_context(temp.path());
    fs::create_dir_all(ctx.state_dir()).unwrap();
    fs::write(ctx.state_file("sessions.jsonl"), b"{}\n").unwrap();
    fs::write(
        temp.path().join(".gitattributes"),
        b".agent/state/*.jsonl merge=union\n",
    )
    .unwrap();
    fs::write(
        temp.path().join(".gitignore"),
        b".agent/state/decisions.jsonl\n",
    )
    .unwrap();
    git(temp.path(), &["init", "-q"]);
    git(
        temp.path(),
        &["add", ".gitattributes", ".agent/state/sessions.jsonl"],
    );

    let output = state_diagnose(&ctx, StateDiagnoseRequest { deep: false });

    assert_eq!(output["git"]["repository"], true);
    assert_eq!(output["git"]["paths"]["sessions"]["tracked"], true);
    assert_eq!(output["git"]["paths"]["sessions"]["ignored"], false);
    assert_eq!(
        output["git"]["paths"]["sessions"]["merge_attribute"],
        "union"
    );
    assert_eq!(output["git"]["paths"]["decisions"]["tracked"], false);
    assert_eq!(output["git"]["paths"]["decisions"]["ignored"], true);
    assert_eq!(
        output["git"]["paths"]["decisions"]["merge_attribute"],
        "union"
    );
}

#[test]
fn diagnose_includes_local_maintenance_cache_usage() {
    let temp = tempdir().unwrap();
    let ctx = fixture_context(temp.path());
    let backups = temp.path().join(".agent/.cache/state-backups/session-1");
    let archives = temp.path().join(".agent/.cache/state-archives");
    fs::create_dir_all(&backups).unwrap();
    fs::create_dir_all(&archives).unwrap();
    fs::write(backups.join("sessions.jsonl.gz"), b"backup").unwrap();
    fs::write(backups.join("manifest.json"), b"manifest").unwrap();
    fs::write(archives.join("receipts.jsonl.gz"), b"archive").unwrap();

    let output = state_diagnose(&ctx, StateDiagnoseRequest { deep: false });

    assert_eq!(output["maintenance_cache"]["exists"], true);
    assert_eq!(output["maintenance_cache"]["files"], 3);
    assert_eq!(output["maintenance_cache"]["bytes"], 21);
    assert_eq!(output["maintenance_cache"]["state_backups"]["bytes"], 14);
    assert_eq!(output["maintenance_cache"]["state_archives"]["bytes"], 7);
    assert_eq!(output["totals"]["maintenance_cache_bytes"], 21);
    assert_eq!(
        output["totals"]["local_disk_bytes"],
        output["totals"]["checkout_state_bytes"].as_u64().unwrap() + 21
    );
    assert!(output["recommendations"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| item["kind"] == "review_maintenance_cache")
    }));
}

#[test]
fn diagnose_recommends_receipt_retention_and_export_before_repair() {
    let mut streams = BTreeMap::new();
    streams.insert(
        "receipts".into(),
        StreamDiagnostics {
            bytes: RECEIPT_RETENTION_RECOMMENDATION_BYTES,
            deep_analysis_error_count: 2,
            ..StreamDiagnostics::default()
        },
    );
    streams.insert(
        "runs".into(),
        StreamDiagnostics {
            bytes: RECEIPT_RETENTION_RECOMMENDATION_BYTES,
            ..StreamDiagnostics::default()
        },
    );
    let receipts = ReceiptPayloadDiagnostics {
        total_top_level_value_bytes: RECEIPT_RETENTION_RECOMMENDATION_BYTES,
        ..ReceiptPayloadDiagnostics::default()
    };

    let recommendations = recommendations(
        true,
        &streams,
        &SessionCompactionDiagnostics::default(),
        &receipts,
        &LegacyArchiveDiagnostics::default(),
        &MaintenanceCacheDiagnostics::default(),
    );

    assert!(recommendations.iter().any(|recommendation| {
        recommendation["kind"] == "archive_receipts"
            && recommendation["command"]
                .as_str()
                .is_some_and(|command| command.contains("state archive"))
            && recommendation["alternative_command"]
                .as_str()
                .is_some_and(|command| command.contains("state export receipts"))
    }));
    assert!(recommendations.iter().any(|recommendation| {
        recommendation["kind"] == "export_receipts_before_repair"
            && recommendation["command"]
                .as_str()
                .is_some_and(|command| command.contains("state export receipts"))
    }));
    assert!(recommendations.iter().any(|recommendation| {
        recommendation["kind"] == "archive_runs"
            && recommendation["command"]
                .as_str()
                .is_some_and(|command| command.contains("state archive"))
    }));
}

#[test]
fn raw_session_analysis_handles_escaped_keys_and_shallow_nulls() {
    let record = br#"{
            "summ\u0061ry": {
                "recent_sessions": [
                    {"summary": null},
                    {"summary": {"value": "escaped \" } ]"}}
                ]
            }
        }"#;
    serde_json::from_slice::<IgnoredAny>(record).unwrap();

    let projection = analyze_session_record(record).unwrap();

    assert_eq!(projection.recursive_summary_values, 1);
    assert_eq!(
        projection.projected_record_bytes,
        record.len() as u64 - br#"{"value": "escaped \" } ]"}"#.len() as u64 + 4
    );
}

fn fixture_paths(root: &Path) -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            paths.insert(path.strip_prefix(root).unwrap().to_path_buf());
            if entry.file_type().unwrap().is_dir() {
                pending.push(path);
            }
        }
    }
    paths
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}
