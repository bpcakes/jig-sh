use super::*;
use fs4::fs_std::FileExt;
use std::fs::OpenOptions;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn receipt_session_pointer_wait_honors_operation_deadline() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let lock_path = crate::state::jsonl::state_lock_path(&ctx.current_session_path());
    fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
    let owner = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .unwrap();
    owner.lock_exclusive().unwrap();
    let receipts_path = ctx.state_file("receipts.jsonl");
    let (sender, receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = record_receipt_with_cancellation_until(
            &ctx,
            receipt_input(),
            &|| false,
            Instant::now() + Duration::from_millis(50),
        );
        sender.send(result).unwrap();
    });
    // Release the test-owned lock even if the regression returns, so a failed
    // assertion cannot leave the worker blocked indefinitely.
    let result = receiver.recv_timeout(Duration::from_secs(2));
    FileExt::unlock(&owner).unwrap();
    worker.join().unwrap();
    let error = result
        .expect("receipt must time out while the session-pointer lock is still held")
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Timed out waiting for current-session lock"),
        "{error:#}"
    );
    assert!(
        read_jsonl::<ReceiptRecord>(&receipts_path)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn cancelled_receipt_finalizes_after_session_contention_clears() {
    assert_cancelled_receipt_finalizes_after_contention(true);
}

#[test]
fn cancelled_receipt_finalizes_after_journal_contention_clears() {
    assert_cancelled_receipt_finalizes_after_contention(false);
}

fn assert_cancelled_receipt_finalizes_after_contention(session_lock: bool) {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let pointer = ctx.current_session_path();
    fs::create_dir_all(pointer.parent().unwrap()).unwrap();
    fs::write(&pointer, "session_example\n").unwrap();
    let receipts_path = ctx.state_file("receipts.jsonl");
    let owner = hold_lock(if session_lock {
        crate::state::jsonl::state_lock_path(&pointer)
    } else {
        receipts_path.clone()
    });
    let (sender, receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut input = receipt_input();
        input.exit_status = 1;
        input.stderr = "cancelled";
        sender
            .send(record_receipt_with_cancellation(&ctx, input, &|| true))
            .unwrap();
    });
    let early = receiver.recv_timeout(Duration::from_millis(100));
    FileExt::unlock(&owner).unwrap();
    worker.join().unwrap();
    assert!(matches!(early, Err(mpsc::RecvTimeoutError::Timeout)));
    let id = receiver.recv().unwrap().unwrap();
    let receipts = read_jsonl::<ReceiptRecord>(&receipts_path).unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].id, id);
    assert_eq!(receipts[0].session_id.as_deref(), Some("session_example"));
    assert_eq!(receipts[0].exit_status, 1);
    assert_eq!(receipts[0].stderr_preview, "cancelled");
}

#[test]
fn receipt_journal_wait_keeps_the_session_read_deadline() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let pointer_owner = hold_lock(crate::state::jsonl::state_lock_path(
        &ctx.current_session_path(),
    ));
    let receipts_path = ctx.state_file("receipts.jsonl");
    let journal_owner = hold_lock(receipts_path.clone());
    let (sender, receiver) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(500);
        ready_tx.send(()).unwrap();
        sender
            .send(record_receipt_with_cancellation_until(
                &ctx,
                receipt_input(),
                &|| false,
                deadline,
            ))
            .unwrap();
    });
    ready_rx.recv().unwrap();
    let early = receiver.recv_timeout(Duration::from_millis(100));
    FileExt::unlock(&pointer_owner).unwrap();
    let result = receiver.recv_timeout(Duration::from_secs(2));
    FileExt::unlock(&journal_owner).unwrap();
    worker.join().unwrap();
    assert!(matches!(early, Err(mpsc::RecvTimeoutError::Timeout)));
    let error = result
        .expect("journal wait must retain the operation deadline")
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Timed out waiting for legacy receipt journal lock"),
        "{error:#}"
    );
    assert!(fs::read(receipts_path).unwrap().is_empty());
}

#[test]
fn expired_receipt_deadline_prevents_append_even_with_session_override() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut input = receipt_input();
    input.session_override = Some("session_example".into());
    let error =
        record_receipt_with_cancellation_until(&ctx, input, &|| false, Instant::now()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Timed out waiting for legacy receipt journal lock"),
        "{error:#}"
    );
    assert!(
        read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))
            .unwrap()
            .is_empty()
    );
}

fn hold_lock(path: std::path::PathBuf) -> fs::File {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    file.lock_exclusive().unwrap();
    file
}

fn receipt_input() -> ReceiptInput<'static> {
    ReceiptInput {
        tool_name: tool::TEST,
        args: json!({}),
        invoked_command_key: None,
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
    }
}
