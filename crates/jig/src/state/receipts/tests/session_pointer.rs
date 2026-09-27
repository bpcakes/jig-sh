use super::*;
use fs4::fs_std::FileExt;
use std::fs::OpenOptions;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn receipts_ignore_a_stale_current_session_pointer_and_its_lock() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let pointer = ctx.current_session_path();
    fs::create_dir_all(pointer.parent().unwrap()).unwrap();
    fs::write(&pointer, "session_example\n").unwrap();
    let owner = hold_lock(crate::state::jsonl::state_lock_path(&pointer));
    let receipts_path = ctx.state_file("receipts.jsonl");

    let result = record_receipt_with_cancellation_until(
        &ctx,
        receipt_input(),
        &|| false,
        Instant::now() + Duration::from_secs(2),
    );
    FileExt::unlock(&owner).unwrap();

    let id = result.unwrap();
    let receipts = read_jsonl::<ReceiptRecord>(&receipts_path).unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].id, id);
    assert_eq!(receipts[0].session_id, None);
}

#[test]
fn cancelled_receipt_finalizes_after_journal_contention_clears() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let receipts_path = ctx.state_file("receipts.jsonl");
    let owner = hold_lock(receipts_path.clone());
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
    assert_eq!(receipts[0].session_id, None);
    assert_eq!(receipts[0].exit_status, 1);
    assert_eq!(receipts[0].stderr_preview, "cancelled");
}

#[test]
fn receipt_journal_wait_honors_the_operation_deadline() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let receipts_path = ctx.state_file("receipts.jsonl");
    let journal_owner = hold_lock(receipts_path.clone());
    let (sender, receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(100);
        sender
            .send(record_receipt_with_cancellation_until(
                &ctx,
                receipt_input(),
                &|| false,
                deadline,
            ))
            .unwrap();
    });
    // Release the test-owned lock even if the regression returns, so a failed
    // assertion cannot leave the worker blocked indefinitely.
    let result = receiver.recv_timeout(Duration::from_secs(2));
    FileExt::unlock(&journal_owner).unwrap();
    worker.join().unwrap();
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
        tool_name: crate::tool_defs::tool::TEST,
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
