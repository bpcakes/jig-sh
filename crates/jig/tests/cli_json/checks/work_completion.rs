use super::*;

#[test]
fn compact_work_check_failure_keeps_one_json_result_and_nonzero_exit() {
    let repo = tempdir().unwrap();
    write_v6_failing_test_repo(repo.path());
    let path = repo.path().join(".jig.toml");
    let mut config = fs::read_to_string(&path).unwrap();
    config.push_str("\n[[work.gates]]\nid = 'verify'\nkind = 'evidence'\nprofile = 'verify'\n");
    fs::write(path, config).unwrap();
    let invalid = jig()
        .current_dir(repo.path())
        .args([
            "work",
            "check",
            "--plan-id",
            "plan_missing",
            "--projection",
            "future",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(!repo.path().join(".agent/state").exists());
    let started = jig()
        .current_dir(repo.path())
        .args([
            "work",
            "start",
            "--title",
            "Example completion",
            "--body",
            "Validate failure output.",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        started.status.success(),
        "{}",
        String::from_utf8_lossy(&started.stderr)
    );
    let started: Value = serde_json::from_slice(&started.stdout).unwrap();
    let plan = started["plan"]["plan_id"].as_str().unwrap();
    let failed = jig()
        .current_dir(repo.path())
        .args([
            "work",
            "check",
            "--plan-id",
            plan,
            "--projection",
            "agent-v1",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        failed.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&failed.stderr)
    );
    let report: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(report["ok"], false, "{report:#}");
    assert_eq!(report["finish_ready"], false);
    assert_eq!(report["command"], "work check");
    assert_eq!(report["activity"][0]["status"], "failed");
    let inspected = jig()
        .current_dir(repo.path())
        .args([
            "work",
            "gates",
            "--plan-id",
            plan,
            "--projection",
            "agent-v1",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(inspected.status.success());
    let inspected: Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(inspected["ok"], true);
    assert_eq!(inspected["finish_ready"], false);
    let standard = jig()
        .current_dir(repo.path())
        .args(["work", "gates", "--plan-id", plan, "--json"])
        .output()
        .unwrap();
    assert!(standard.status.success());
    let standard: Value = serde_json::from_slice(&standard.stdout).unwrap();
    assert_eq!(standard["gates_ok"], false);
    assert!(standard.get("finish_ready").is_none());
    let mut mcp = jig()
        .current_dir(repo.path())
        .args(["mcp", "--surface", "agent-v1"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "jig.work_check", "arguments": {"plan_id": plan}}});
    std::io::Write::write_all(
        &mut mcp.stdin.take().unwrap(),
        format!("{request}\n").as_bytes(),
    )
    .unwrap();
    let output = mcp.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["result"]["isError"], true, "{response:#}");
    assert_eq!(response["result"]["structuredContent"]["ok"], false);
    assert_eq!(
        response["result"]["structuredContent"]["finish_ready"],
        false
    );
}
