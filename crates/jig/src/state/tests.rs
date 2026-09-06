use std::fs;
use std::io::Read;
use std::path::Path;

use flate2::read::GzDecoder;
use fs4::fs_std::FileExt;
use serde_json::{Value, json};
use tempfile::tempdir;

use super::jsonl::{
    jsonl_end_offset, read_jsonl_with_cancellation, read_jsonl_with_data_lock, read_jsonl_with_io,
    read_receipt_window_with_bytes, read_receipts_reverse_with_cancellation,
    receipts_for_plan_with_lock, scan_jsonl_raw_from, state_lock_path, with_jsonl_write_lock,
    write_jsonl_locked,
};
use super::records::SessionEvent;
use super::*;
use crate::command::StateRestoreRequest;
use crate::context::RepoContext;
use crate::git_receipts::DiffStat;
use crate::test_env::TestRepoBuilder;
use crate::tool_defs::tool;

fn write_fixture_repo(root: &Path) {
    TestRepoBuilder::new(root)
        .required_commands(["rust_fmt_check_command"])
        .write();
}

#[test]
fn appends_jsonl_records() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    append_jsonl(&path, &json!({ "id": 1 })).unwrap();
    append_jsonl(&path, &json!({ "id": 2 })).unwrap();

    let items: Vec<Value> = read_jsonl(&path).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["id"], 1);
    assert_eq!(items[1]["id"], 2);
}

#[test]
fn jsonl_cursor_scans_only_records_appended_after_the_cursor() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    let history = (0..1_000)
        .map(|id| format!("{{\"id\":{id}}}\n"))
        .collect::<String>();
    fs::write(&path, history).unwrap();
    let cursor = jsonl_end_offset(&path).unwrap();
    append_jsonl(&path, &json!({"id": "new"})).unwrap();

    let mut records = Vec::new();
    let (next, stats) = scan_jsonl_raw_from(&path, cursor, &|| false, |record| {
        records.push(serde_json::from_slice::<Value>(record.bytes)?);
        Ok(())
    })
    .unwrap();

    assert_eq!(records, [json!({"id": "new"})]);
    assert_eq!(stats.records, 1);
    assert_eq!(next, fs::metadata(path).unwrap().len());
}

#[test]
fn jsonl_cursor_lock_wait_is_cancellable() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, "{\"id\":1}\n").unwrap();
    let locked = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    locked.lock_exclusive().unwrap();
    let polls = std::cell::Cell::new(0usize);

    let error = scan_jsonl_raw_from(
        &path,
        0,
        &|| {
            polls.set(polls.get() + 1);
            polls.get() > 2
        },
        |_| Ok(()),
    )
    .unwrap_err()
    .to_string();

    FileExt::unlock(&locked).unwrap();
    assert!(error.contains("cancelled"));
}

#[test]
fn missing_jsonl_read_does_not_materialize_state() {
    let temp = tempdir().unwrap();
    let parent = temp.path().join("missing/state");
    let path = parent.join("events.jsonl");

    let items = read_jsonl::<Value>(&path).unwrap();

    assert!(items.is_empty());
    assert!(!parent.exists());
}

#[test]
fn receipt_window_reads_a_bounded_tail_independent_of_old_history() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    for index in 0..100 {
        let output = "x".repeat(4_000);
        record_receipt(
            &ctx,
            ReceiptInput {
                tool_name: "jig.test",
                args: json!({}),
                invoked_command_key: Some("test".into()),
                plan_id: None,
                started_at_ms: index as u64,
                ended_at_ms: index as u64 + 1,
                exit_status: i32::from(index == 50),
                stdout: &output,
                stderr: "",
                evidence: None,
                session_override: None,
                collect_git_metadata: false,
                collect_worktree_fingerprint: false,
                worktree_fingerprint_override: None,
            },
        )
        .unwrap();
    }

    let path = ctx.state_file("receipts.jsonl");
    let file_len = fs::metadata(&path).unwrap().len();
    let (recent, bytes_read) = read_receipt_window_with_bytes(&path, 1).unwrap();

    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].exit_status, 0);
    assert!(bytes_read <= 16 * 1024, "read {bytes_read} bytes");
    assert!(
        bytes_read < file_len / 2,
        "tail read should not scan old history"
    );
}

#[test]
fn receipt_window_accepts_a_record_at_the_exact_dashboard_limit() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("receipts.jsonl");
    let receipt = receipt_record("receipt_exact", tool::TEST, 0, DiffStat::default());
    let mut record = serde_json::to_vec(&receipt).unwrap();
    assert!(record.len() < super::jsonl::DASHBOARD_JSONL_RECORD_BYTES);
    record.resize(super::jsonl::DASHBOARD_JSONL_RECORD_BYTES, b' ');
    record.push(b'\n');
    fs::write(&path, record).unwrap();

    let receipts = read_receipts_reverse_with_cancellation(&path, 1, |_| true, &|| false)
        .unwrap()
        .0;

    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].id, "receipt_exact");
}

#[test]
fn legacy_receipt_window_preserves_records_above_the_dashboard_limit() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("receipts.jsonl");
    let mut receipt = receipt_record("receipt_legacy_large", tool::TEST, 0, DiffStat::default());
    receipt.stdout_preview = "x".repeat(super::jsonl::DASHBOARD_JSONL_RECORD_BYTES + 1);
    append_jsonl(&path, &receipt).unwrap();

    let receipts = super::jsonl::read_receipt_window(&path, 1).unwrap();

    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].id, "receipt_legacy_large");
}

#[test]
fn bounded_reverse_receipt_scan_matches_naive_reverse_across_chunk_boundaries() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("receipts.jsonl");
    let target_lengths = [
        257,
        16 * 1024 - 1,
        16 * 1024,
        16 * 1024 + 1,
        2 * 16 * 1024 - 1,
        3 * 16 * 1024 + 7,
        5 * 16 * 1024 + 3,
    ];
    let mut bytes = b" \n\t\n".to_vec();
    for (index, target_len) in target_lengths.into_iter().enumerate() {
        let mut receipt = receipt_record(
            &format!("receipt_{index}"),
            if index % 2 == 0 {
                tool::TEST
            } else {
                tool::CLIPPY
            },
            0,
            DiffStat::default(),
        );
        receipt.stdout_preview = "x".repeat(target_len);
        let record = serde_json::to_vec(&receipt).unwrap();
        bytes.extend_from_slice(&record);
        bytes.extend_from_slice(b"  \n");
        if index % 2 == 0 {
            bytes.extend_from_slice(b"\n");
        }
    }
    fs::write(&path, &bytes).unwrap();

    let mut expected = bytes
        .split(|byte| *byte == b'\n')
        .filter(|record| !record.iter().all(u8::is_ascii_whitespace))
        .map(|record| serde_json::from_slice::<ReceiptRecord>(record).unwrap())
        .collect::<Vec<_>>();
    expected.reverse();

    let all = read_receipts_reverse_with_cancellation(&path, usize::MAX, |_| true, &|| false)
        .unwrap()
        .0;
    let tests_only = read_receipts_reverse_with_cancellation(
        &path,
        usize::MAX,
        |receipt| receipt.tool_name == tool::TEST,
        &|| false,
    )
    .unwrap()
    .0;

    assert_eq!(
        all.iter().map(|receipt| &receipt.id).collect::<Vec<_>>(),
        expected
            .iter()
            .map(|receipt| &receipt.id)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        tests_only
            .iter()
            .map(|receipt| &receipt.id)
            .collect::<Vec<_>>(),
        expected
            .iter()
            .filter(|receipt| receipt.tool_name == tool::TEST)
            .map(|receipt| &receipt.id)
            .collect::<Vec<_>>()
    );
}

#[test]
fn receipt_window_rejects_an_unterminated_final_record() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    record_receipt(
        &ctx,
        ReceiptInput {
            tool_name: "jig.test",
            args: json!({}),
            invoked_command_key: Some("test".into()),
            plan_id: None,
            started_at_ms: 1,
            ended_at_ms: 2,
            exit_status: 0,
            stdout: "",
            stderr: "",
            evidence: None,
            session_override: None,
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: None,
        },
    )
    .unwrap();
    let path = ctx.state_file("receipts.jsonl");
    let mut bytes = fs::read(&path).unwrap();
    assert_eq!(bytes.pop(), Some(b'\n'));
    fs::write(&path, bytes).unwrap();

    let error = read_receipt_window_with_bytes(&path, 1).unwrap_err();

    assert!(
        error.to_string().contains("not newline-terminated"),
        "{error:#}"
    );
}

#[test]
fn receipt_plan_query_uses_stable_snapshot_when_advisory_locks_are_unsupported() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    record_receipt(
        &ctx,
        ReceiptInput {
            tool_name: "jig.test",
            args: json!({}),
            invoked_command_key: Some("test".into()),
            plan_id: Some("plan_fallback".into()),
            started_at_ms: 1,
            ended_at_ms: 2,
            exit_status: 0,
            stdout: "",
            stderr: "",
            evidence: None,
            session_override: None,
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: None,
        },
    )
    .unwrap();

    let receipts = receipts_for_plan_with_lock(
        &ctx.state_file("receipts.jsonl"),
        "plan_fallback",
        50,
        |_| Err(std::io::Error::from(std::io::ErrorKind::Unsupported)),
    )
    .unwrap();

    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].plan_id.as_deref(), Some("plan_fallback"));
}

#[test]
fn receipt_fallback_ignores_an_unterminated_final_record() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("receipts.jsonl");
    let first = receipt_record("receipt_complete", tool::TEST, 0, DiffStat::default());
    let second = receipt_record("receipt_partial", tool::TEST, 0, DiffStat::default());
    let mut bytes = serde_json::to_vec(&first).unwrap();
    bytes.push(b'\n');
    let partial = serde_json::to_vec(&second).unwrap();
    bytes.extend_from_slice(&partial[..partial.len() / 2]);
    fs::write(&path, bytes).unwrap();

    let receipts = receipts_for_plan_with_lock(&path, "plan_1", 50, |_| {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    })
    .unwrap();

    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].id, "receipt_complete");
}

#[test]
fn jsonl_read_without_cache_lock_does_not_create_one() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n").unwrap();
    let cache_lock = state_lock_path(&path);
    assert!(!cache_lock.exists());

    let items = read_jsonl::<Value>(&path).unwrap();

    assert_eq!(items, vec![json!({ "id": 1 })]);
    assert!(!cache_lock.exists());
}

#[cfg(unix)]
#[test]
fn jsonl_read_accepts_read_only_data_file() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();

    let items = read_jsonl::<Value>(&path).unwrap();

    assert_eq!(items, vec![json!({ "id": 1 })]);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o444
    );
}

#[test]
fn unlocked_jsonl_snapshot_reports_stable_invalid_unterminated_tail() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n{\"id\":").unwrap();

    let error = read_jsonl_with_data_lock::<Value>(&path, |_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "test lock seam",
        ))
    })
    .unwrap_err();

    assert!(error.to_string().contains("record 2"));
}

#[test]
fn unlocked_jsonl_snapshot_accepts_partial_then_complete_samples() {
    use std::collections::VecDeque;

    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"seed").unwrap();
    let mut snapshots = VecDeque::from([
        b"{\"id\":1}\n{\"id\":".to_vec(),
        b"{\"id\":1}\n{\"id\":2}\n".to_vec(),
    ]);

    let items = read_jsonl_with_io::<Value>(
        &path,
        |_| Err(std::io::Error::from(std::io::ErrorKind::Unsupported)),
        |_| Ok(snapshots.pop_front().unwrap()),
    )
    .unwrap();

    assert_eq!(items, vec![json!({ "id": 1 }), json!({ "id": 2 })]);
    assert!(snapshots.is_empty());
}

#[test]
fn unlocked_jsonl_snapshot_returns_prefix_after_three_changing_tails() {
    use std::collections::VecDeque;

    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"seed").unwrap();
    let mut snapshots = VecDeque::from([
        b"{\"id\":1}\n{".to_vec(),
        b"{\"id\":1}\n{\"i".to_vec(),
        b"{\"id\":1}\n{\"id\":".to_vec(),
    ]);

    let items = read_jsonl_with_io::<Value>(
        &path,
        |_| Err(std::io::Error::from(std::io::ErrorKind::Unsupported)),
        |_| Ok(snapshots.pop_front().unwrap()),
    )
    .unwrap();

    assert_eq!(items, vec![json!({ "id": 1 })]);
    assert!(snapshots.is_empty());
}

#[test]
fn jsonl_read_retries_interrupted_data_lock() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n").unwrap();
    let mut attempts = 0;

    let items = read_jsonl_with_io::<Value>(
        &path,
        |file| {
            attempts += 1;
            if attempts == 1 {
                Err(std::io::Error::from(std::io::ErrorKind::Interrupted))
            } else {
                FileExt::lock_shared(file)
            }
        },
        |_| panic!("locked read must use the original handle"),
    )
    .unwrap();

    assert_eq!(attempts, 2);
    assert_eq!(items, vec![json!({ "id": 1 })]);
}

#[test]
fn jsonl_read_propagates_nonunsupported_data_lock_errors() {
    for kind in [
        std::io::ErrorKind::PermissionDenied,
        std::io::ErrorKind::WouldBlock,
        std::io::ErrorKind::Other,
    ] {
        let temp = tempdir().unwrap();
        let path = temp.path().join("events.jsonl");
        fs::write(&path, b"{\"id\":1}\n").unwrap();

        let error = read_jsonl_with_io::<Value>(
            &path,
            |_| Err(std::io::Error::from(kind)),
            |_| panic!("non-unsupported lock error must not enter fallback"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("Failed to shared-lock"));
    }
}

#[test]
fn unlocked_jsonl_snapshot_rejects_malformed_terminated_record() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\nnot-json\n{\"id\":").unwrap();

    let error = read_jsonl_with_data_lock::<Value>(&path, |_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "test lock seam",
        ))
    })
    .unwrap_err();

    assert!(error.to_string().contains("record 2"));
}

#[test]
fn unlocked_jsonl_snapshot_keeps_valid_unterminated_final_record() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n{\"id\":2}").unwrap();

    let items = read_jsonl_with_data_lock::<Value>(&path, |_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "test lock seam",
        ))
    })
    .unwrap();

    assert_eq!(items, vec![json!({ "id": 1 }), json!({ "id": 2 })]);
}

#[test]
fn locked_jsonl_read_rejects_invalid_unterminated_final_record() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n{\"id\":").unwrap();

    let error = read_jsonl::<Value>(&path).unwrap_err();

    assert!(error.to_string().contains("record 2"));
}

#[test]
fn jsonl_read_waits_for_atomic_rewrite_and_observes_replacement() {
    use std::sync::mpsc;
    use std::time::Duration;

    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    append_jsonl(&path, &json!({ "id": "before" })).unwrap();
    let writer_path = path.clone();
    let reader_path = path;
    let (rewritten_tx, rewritten_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (read_tx, read_rx) = mpsc::channel();

    std::thread::scope(|scope| {
        scope.spawn(move || {
            with_jsonl_write_lock(&writer_path, |guard| {
                write_jsonl_locked(guard, &writer_path, &[json!({ "id": "after" })])?;
                rewritten_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            })
            .unwrap();
        });
        rewritten_rx.recv().unwrap();
        scope.spawn(move || {
            let result = read_jsonl::<Value>(&reader_path).unwrap();
            read_tx.send(result).unwrap();
        });

        assert!(read_rx.recv_timeout(Duration::from_millis(100)).is_err());
        release_tx.send(()).unwrap();
        let values = read_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(values, vec![json!({ "id": "after" })]);
    });
}

#[test]
fn jsonl_read_reopens_a_preopened_inode_after_atomic_rewrite() {
    use std::cell::Cell;
    use std::io::Write;

    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":\"before\"}\n").unwrap();
    let lock_attempts = Cell::new(0usize);

    let values = read_jsonl_with_io::<Value>(
        &path,
        |_| {
            let attempt = lock_attempts.get();
            lock_attempts.set(attempt + 1);
            if attempt == 0 {
                let mut replacement = tempfile::NamedTempFile::new_in(temp.path()).unwrap();
                replacement.write_all(b"{\"id\":\"after\"}\n").unwrap();
                replacement.persist(&path).unwrap();
            }
            Ok(())
        },
        |_| unreachable!("the injected lock remains supported"),
    )
    .unwrap();

    assert!(lock_attempts.get() >= 2);
    assert_eq!(values, vec![json!({ "id": "after" })]);
}

#[test]
fn jsonl_read_waits_for_existing_data_file_lock() {
    use std::sync::mpsc;
    use std::time::Duration;

    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n").unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    FileExt::lock_exclusive(&lock).unwrap();
    let reader_path = path;
    let (started_tx, started_rx) = mpsc::channel();
    let (read_tx, read_rx) = mpsc::channel();

    std::thread::scope(|scope| {
        scope.spawn(move || {
            started_tx.send(()).unwrap();
            read_tx.send(read_jsonl::<Value>(&reader_path)).unwrap();
        });
        started_rx.recv().unwrap();
        assert!(read_rx.recv_timeout(Duration::from_millis(100)).is_err());
        FileExt::unlock(&lock).unwrap();
        let values = read_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(values, vec![json!({ "id": 1 })]);
    });
}

#[test]
fn cancellable_jsonl_read_stops_while_data_file_lock_is_held() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::Duration;

    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n").unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    FileExt::lock_exclusive(&lock).unwrap();

    let reader_path = path;
    let cancelled = Arc::new(AtomicBool::new(false));
    let reader_cancelled = Arc::clone(&cancelled);
    let (started_tx, started_rx) = mpsc::channel();
    let (read_tx, read_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let result = read_jsonl_with_cancellation::<Value>(&reader_path, &|| {
            reader_cancelled.load(Ordering::SeqCst)
        });
        read_tx.send(result).unwrap();
    });

    started_rx.recv().unwrap();
    assert!(read_rx.recv_timeout(Duration::from_millis(100)).is_err());
    cancelled.store(true, Ordering::SeqCst);
    let result = match read_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => result,
        Err(error) => {
            FileExt::unlock(&lock).unwrap();
            reader.join().unwrap();
            panic!("cancellable read stayed blocked on the data lock: {error}");
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
fn cancellable_jsonl_read_stops_while_cache_lock_is_held() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::Duration;

    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n").unwrap();
    let cache_path = state_lock_path(&path);
    fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(cache_path)
        .unwrap();
    FileExt::lock_exclusive(&lock).unwrap();

    let cancelled = Arc::new(AtomicBool::new(false));
    let reader_cancelled = Arc::clone(&cancelled);
    let (read_tx, read_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        read_tx
            .send(read_jsonl_with_cancellation::<Value>(&path, &|| {
                reader_cancelled.load(Ordering::SeqCst)
            }))
            .unwrap();
    });

    assert!(read_rx.recv_timeout(Duration::from_millis(100)).is_err());
    cancelled.store(true, Ordering::SeqCst);
    let result = read_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    FileExt::unlock(&lock).unwrap();
    reader.join().unwrap();

    assert!(crate::cancellation::is_status_collection_cancellation(
        &result.unwrap_err()
    ));
}

#[test]
fn cancellable_jsonl_read_checks_between_records() {
    use std::cell::Cell;

    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    let records = (0..100)
        .map(|id| format!("{{\"id\":{id}}}\n"))
        .collect::<String>();
    fs::write(&path, records).unwrap();
    let checks = Cell::new(0);

    let error = read_jsonl_with_cancellation::<Value>(&path, &|| {
        let current = checks.get();
        checks.set(current + 1);
        current >= 12
    })
    .unwrap_err();

    assert_eq!(error.to_string(), "status collection was cancelled");
    assert!(checks.get() > 12);
}

mod receipt_cases;
use receipt_cases::receipt_record;
mod session_and_plans;

mod archive_validation;

include!("tests_parts/part_01.rs");
