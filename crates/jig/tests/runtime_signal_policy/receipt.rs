use super::*;

#[test]
fn check_receipt_session_wait_is_bounded_after_sigint() {
    let temp = tempdir().unwrap();
    let cache = temp.path().join(".agent/.cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "ExampleProject"
default_branch = "main"
jig_version = "0.2.0-beta.1"
rust_test_command = 'touch "$JIG_TEST_CHECK_FINISHED"'
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        r#"{
  "contract_version": 2,
  "jig_version": "0.2.0-beta.1",
  "tool_namespace": "jig",
  "required_commands": ["rust_test_command"],
  "tools": [{
    "name": "jig.test",
    "kind": "command",
    "description": "Run configured tests.",
    "command": "rust_test_command"
  }]
}"#,
    )
    .unwrap();
    let owner = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(cache.join("jig-current-session.txt.lock"))
        .unwrap();
    owner.lock_exclusive().unwrap();
    let marker = temp.path().join("check-finished");
    let mut child = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["check", "test"])
        .current_dir(temp.path())
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD")
        .env("JIG_TEST_CHECK_FINISHED", &marker)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() && Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            let output = child.wait_with_output().unwrap();
            panic!(
                "check exited early: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !marker.exists() {
        let _ = child.kill();
        let _ = child.wait();
        panic!("check did not run its configured command");
    }
    // Allow the successful command to finish and reach receipt publication.
    assert!(
        child
            .wait_timeout(Duration::from_millis(200))
            .unwrap()
            .is_none()
    );
    // SAFETY: the child is still live, blocked behind our session lock.
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGINT) }, 0);
    // Ordinary receipts finalize even after cancellation, but all publication
    // locks must fit inside the production 30-second budget.
    let status = child.wait_timeout(Duration::from_secs(35)).unwrap();
    if status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
        panic!("check receipt publication exceeded its lock budget after SIGINT");
    }
    let output = child.wait_with_output().unwrap();
    FileExt::unlock(&owner).unwrap();
    let status = status.unwrap();
    assert!(
        status.signal() == Some(libc::SIGINT) || status.code() == Some(128 + libc::SIGINT),
        "unexpected status {status}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipts = temp.path().join(".agent/state/receipts.jsonl");
    assert!(!receipts.exists() || fs::read(receipts).unwrap().is_empty());
}
