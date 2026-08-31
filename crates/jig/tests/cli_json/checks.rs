use super::*;

#[test]
fn direct_file_budget_json_uses_stable_exits_and_creates_no_durable_state() {
    let repo = tempdir().unwrap();
    write_file_budget_repo(repo.path());
    let before = Command::new("git")
        .current_dir(repo.path())
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(before.status.success());
    let before = String::from_utf8(before.stdout).unwrap().trim().to_owned();
    fs::write(repo.path().join("src/lib.rs"), "one\ntwo\n").unwrap();
    let state = repo.path().join(".agent/state");
    assert!(!state.exists());

    let check = jig()
        .current_dir(repo.path())
        .args(["file-budget", "check", "--base", "main", "--json"])
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(1));
    assert!(check.stderr.is_empty());
    let payload: Value = serde_json::from_slice(&check.stdout).unwrap();
    assert_eq!(payload["schema"], "jig.file_budget/report-v1");
    assert_eq!(payload["conclusion"], "failure");
    assert_eq!(payload["exit_status"], 1);
    assert_eq!(payload["report"]["view"], "worktree");

    let audit = jig()
        .current_dir(repo.path())
        .args(["file-budget", "audit", "--json"])
        .output()
        .unwrap();
    assert_eq!(audit.status.code(), Some(0));
    let payload: Value = serde_json::from_slice(&audit.stdout).unwrap();
    assert_eq!(payload["conclusion"], "failure");
    assert_eq!(payload["exit_status"], 0);

    let strict = jig()
        .current_dir(repo.path())
        .args(["file-budget", "audit", "--strict", "--json"])
        .output()
        .unwrap();
    assert_eq!(strict.status.code(), Some(1));
    assert!(
        !state.exists(),
        "direct diagnostics must not create run or receipt state"
    );

    let authored = jig()
        .current_dir(repo.path())
        .args(["check", "repo:file-budget", "--no-receipt", "--json"])
        .output()
        .unwrap();
    assert_eq!(authored.status.code(), Some(1));
    let payload: Value = serde_json::from_slice(&authored.stdout).unwrap();
    let target = &payload["run"]["targets"][0];
    assert_eq!(target["conclusion"], "failure");
    assert!(
        target["native_evidence"]["file_budget"]["evaluation_digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:"))
    );
    assert!(
        !authored
            .stdout
            .windows(b"engine_pending".len())
            .any(|window| window == b"engine_pending")
    );

    let push = jig()
        .current_dir(repo.path())
        .args([
            "check",
            "repo:file-budget",
            "--comparison-exact-tree",
            &before,
            "--comparison-provenance",
            "push_before",
            "--no-receipt",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(push.status.code(), Some(1));
    let payload: Value = serde_json::from_slice(&push.stdout).unwrap();
    let evidence = &payload["run"]["targets"][0]["native_evidence"]["file_budget"];
    assert_eq!(evidence["request"]["kind"], "exact_tree");
    assert_eq!(evidence["request"]["requested_oid"], before);
    assert_eq!(evidence["request"]["provenance"], "push_before");
    assert_eq!(evidence["comparison"]["kind"], "exact_tree");
}

#[test]
fn named_v6_check_uses_aggregate_output_and_exits_unsuccessfully() {
    let repo = tempdir().unwrap();
    write_v6_failing_test_repo(repo.path());

    let output = jig()
        .current_dir(repo.path())
        .args(["check", "test"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Jig check: failed"), "{stdout}");
    assert!(stdout.contains("api:test: failed (exit 7)"), "{stdout}");
}

#[cfg(unix)]
#[test]
fn repository_check_prints_lease_contention_before_the_lease_is_released() {
    let repo = tempdir().unwrap();
    write_v6_failing_test_repo(repo.path());
    fs::create_dir_all(repo.path().join(".agent/.cache")).unwrap();
    let lease = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(repo.path().join(".agent/.cache/repository-execution.lock"))
        .unwrap();
    lease.lock_exclusive().unwrap();
    let stderr_path = repo.path().join("lease-wait.stderr");
    let stderr = File::create(&stderr_path).unwrap();
    let mut child = jig()
        .current_dir(repo.path())
        .args(["check", "api:test", "--no-receipt"])
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    let observed = loop {
        let stderr = fs::read_to_string(&stderr_path).unwrap();
        if stderr.contains("Waiting for another repository execution") {
            break true;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("repository check exited with {status} before reporting lease contention");
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !observed {
        let _ = child.kill();
        let _ = child.wait();
        panic!("lease contention remained buffered while the command was waiting");
    }

    drop(lease);
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(1));
    let stderr = fs::read_to_string(stderr_path).unwrap();
    assert_eq!(
        stderr
            .matches("Waiting for another repository execution")
            .count(),
        1,
        "a final progress flush must not redeliver the wait notice: {stderr}"
    );
}

#[test]
fn external_check_selectors_accept_global_json_and_help_after_the_selector() {
    let repo = tempdir().unwrap();
    write_v6_failing_test_repo(repo.path());

    let json_output = jig()
        .current_dir(repo.path())
        .args(["check", "api:test", "--json"])
        .output()
        .unwrap();
    assert_eq!(json_output.status.code(), Some(1));
    let payload: Value = serde_json::from_slice(&json_output.stdout).unwrap();
    assert_eq!(payload["run"]["conclusion"], "failure");

    let help = jig()
        .current_dir(repo.path())
        .args(["check", "api:test", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("Run configured project checks"), "{help}");
    assert!(!help.contains("unknown check option"), "{help}");
}

#[test]
fn json_mode_wraps_usage_and_pre_output_command_errors() {
    let usage = jig().args(["work", "check", "--json"]).output().unwrap();
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stderr.is_empty());
    let usage: Value = serde_json::from_slice(&usage.stdout).unwrap();
    assert_eq!(usage["ok"], false);
    assert_eq!(usage["error"]["kind"], "usage");
    assert_eq!(usage["exit_status"], 2);

    let repo = tempdir().unwrap();
    let command = jig()
        .current_dir(repo.path())
        .args(["info", "--json"])
        .output()
        .unwrap();
    assert_eq!(command.status.code(), Some(1));
    assert!(command.stderr.is_empty());
    let command: Value = serde_json::from_slice(&command.stdout).unwrap();
    assert_eq!(command["ok"], false);
    assert_eq!(command["error"]["kind"], "command_failed");
    assert_eq!(command["exit_status"], 1);
}

#[test]
fn launcher_handoff_root_is_authoritative_over_cwd_and_environment() {
    let ambient = tempdir().unwrap();
    let authoritative = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(authoritative.join(".agent/jig-contract.json")).unwrap(),
    )
    .unwrap();
    let contract_version = manifest["contract_version"].as_u64().unwrap().to_string();
    let answers: toml::Value =
        toml::from_str(&std::fs::read_to_string(authoritative.join(".jig.toml")).unwrap()).unwrap();
    let repo_name = answers["repo_name"].as_str().unwrap();

    let output = jig()
        .current_dir(ambient.path())
        .env("JIG_REPO_ROOT", ambient.path())
        .arg("--__launcher-contract-version")
        .arg(contract_version)
        .args(["--__launcher-profile", "runtime"])
        .arg("--__launcher-repo-root")
        .arg(&authoritative)
        .args(["info", "--json"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ignored JIG_REPO_ROOT")
            && stderr.contains("generated launcher root")
            && stderr.contains("is authoritative"),
        "expected authoritative-root warning, got:\n{stderr}"
    );
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output["repo"]["name"], repo_name);
}

#[test]
fn json_mode_classifies_output_mode_conflicts_as_usage_errors() {
    for args in [
        vec!["--json", "status", "--tui"],
        vec!["status", "--tui", "--json"],
        vec![
            "--json",
            "work",
            "start",
            "--title",
            "test",
            "--print-plan-id",
        ],
        vec![
            "work",
            "--json",
            "start",
            "--title",
            "test",
            "--print-plan-id",
        ],
        vec![
            "work",
            "start",
            "--json",
            "--title",
            "test",
            "--print-plan-id",
        ],
        vec![
            "work",
            "start",
            "--title",
            "test",
            "--print-plan-id",
            "--json",
        ],
    ] {
        let output = jig().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stderr.is_empty());
        let output: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output["ok"], false);
        assert_eq!(output["error"]["kind"], "usage");
        assert_eq!(output["exit_status"], 2);
    }
}

#[test]
fn mcp_parse_errors_keep_stdout_reserved_for_protocol_frames() {
    let output = jig().args(["mcp", "--json", "--bogus"]).output().unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument '--bogus'"));
}
