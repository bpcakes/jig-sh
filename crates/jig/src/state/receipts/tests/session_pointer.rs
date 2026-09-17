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
            },
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
