use super::*;

fn queued_run(run_id: &str) -> Value {
    json!({
        "id": format!("run_event_{run_id}_queued"),
        "run_id": run_id,
        "event": "queued",
        "timestamp_ms": 1,
    })
}

fn target_id(target: &str) -> Value {
    let (component, action) = target.split_once(':').unwrap();
    json!({"component": component, "action": action})
}

fn completed_target(run_id: &str, target: &str, conclusion: &str, ended_at_ms: u64) -> Value {
    let target = target_id(target);
    json!({
        "id": format!("run_event_{run_id}_{}", target["action"]),
        "run_id": run_id,
        "event": "target_completed",
        "timestamp_ms": ended_at_ms,
        "target": target,
        "result": {
            "target": target,
            "status": "completed",
            "conclusion": conclusion,
            "started_at_ms": 1,
            "ended_at_ms": ended_at_ms,
            "exit_code": i32::from(conclusion != "success"),
            "config_digest": "sha256:config",
            "input_digest": "sha256:input",
        },
    })
}

fn write_run_history(ctx: &RepoContext, records: &[Value]) {
    let lines = records
        .iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>();
    fs::create_dir_all(ctx.state_dir()).unwrap();
    fs::write(ctx.state_file("runs.jsonl"), lines).unwrap();
}

#[test]
fn state_summary_is_read_only_and_counts_run_history() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    // Results written before receipts were removed carry retired fields.
    let mut legacy = completed_target("run_1", "api:test", "success", 5);
    legacy["result"]["receipt_id"] = json!("receipt_legacy");
    legacy["result"]["target_freshness"] = json!({"status": "complete"});
    write_run_history(
        &ctx,
        &[
            queued_run("run_1"),
            legacy,
            completed_target("run_1", "api:lint", "failure", 9),
        ],
    );
    let before = fs::read(ctx.state_file("runs.jsonl")).unwrap();

    let output = state_summary(&ctx).unwrap();

    assert_eq!(fs::read(ctx.state_file("runs.jsonl")).unwrap(), before);
    assert_eq!(output["ok"], true);
    assert_eq!(
        output["counts"],
        json!({"runs": 1, "target_results": 2, "failed_target_results": 1})
    );
    let recent = &output["recent_target_results"];
    assert_eq!(recent[0]["target"], target_id("api:lint"));
    assert_eq!(recent[0]["conclusion"], "failure");
    assert_eq!(recent[0]["exit_code"], 1);
    assert_eq!(recent[1]["run_id"], "run_1");
    assert_eq!(recent[1]["target"], target_id("api:test"));
    assert!(recent[1].get("receipt_id").is_none());
    assert!(output.get("recent_receipts").is_none());
}

#[test]
fn legacy_state_summary_accepts_records_above_the_dashboard_limit() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut large = completed_target("run_1", "api:test", "failure", 5);
    large["result"]["output_tail"] = json!({
        "stdout": "x".repeat(super::jsonl::DASHBOARD_JSONL_RECORD_BYTES + 1),
        "stderr": "",
    });
    write_run_history(&ctx, &[large]);

    let summary = state_summary(&ctx).unwrap();
    let bounded_error =
        super::summary::state_summary_with_cancellation(&ctx, &|| false).unwrap_err();

    assert_eq!(summary["counts"]["target_results"], 1);
    assert!(
        bounded_error
            .downcast_ref::<super::jsonl::JsonlRecordTooLarge>()
            .is_some()
    );
}

#[test]
fn cancellable_state_summary_stops_during_stream_collection() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::Duration;

    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    fs::create_dir_all(ctx.state_dir()).unwrap();
    let runs_path = ctx.state_file("runs.jsonl");
    fs::write(&runs_path, b"").unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&runs_path)
        .unwrap();
    FileExt::lock_exclusive(&lock).unwrap();

    let reader_ctx = ctx;
    let cancelled = Arc::new(AtomicBool::new(false));
    let reader_cancelled = Arc::clone(&cancelled);
    let (started_tx, started_rx) = mpsc::channel();
    let (summary_tx, summary_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let result = super::summary::state_summary_with_cancellation(&reader_ctx, &|| {
            reader_cancelled.load(Ordering::SeqCst)
        });
        summary_tx.send(result).unwrap();
    });

    started_rx.recv().unwrap();
    assert!(summary_rx.recv_timeout(Duration::from_millis(100)).is_err());
    cancelled.store(true, Ordering::SeqCst);
    let result = match summary_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => result,
        Err(error) => {
            FileExt::unlock(&lock).unwrap();
            reader.join().unwrap();
            panic!("state summary stayed blocked on a state stream: {error}");
        }
    };

    assert_eq!(
        result.unwrap_err().to_string(),
        "status collection was cancelled"
    );
    FileExt::unlock(&lock).unwrap();
    reader.join().unwrap();
}

#[test]
fn state_summary_on_uninitialized_repo_creates_nothing() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    assert!(!ctx.state_dir().exists());
    assert!(!temp.path().join(".agent/.cache").exists());

    let output = state_summary(&ctx).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["counts"]["runs"], 0);
    assert_eq!(output["recent_target_results"], json!([]));
    assert!(!ctx.state_dir().exists());
    assert!(!temp.path().join(".agent/.cache").exists());
}

#[cfg(unix)]
#[test]
fn state_summary_reads_existing_read_only_state() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let runs_path = ctx.state_file("runs.jsonl");
    append_jsonl(
        &runs_path,
        &completed_target("run_1", "api:test", "success", 5),
    )
    .unwrap();
    let state_dir = ctx.state_dir();
    let cache_dir = temp.path().join(".agent/.cache");
    let lock_dir = cache_dir.join("state-locks");
    for path in [runs_path.clone(), super::jsonl::state_lock_path(&runs_path)] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o444)).unwrap();
    }
    for path in [&state_dir, &lock_dir, &cache_dir] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o555)).unwrap();
    }

    let output = state_summary(&ctx).unwrap();

    assert_eq!(output["counts"]["target_results"], 1);
    for path in [&cache_dir, &lock_dir, &state_dir] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}
