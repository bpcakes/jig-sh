use super::*;

#[test]
fn guide_and_map_checks_report_commonmark_source_lines() {
    let root = fixture("# Root\r[Broken](missing.md)\r\n[Owner][undefined]\n");
    let output = run(root.path(), &["check", "agent-guides", "--json"]);
    assert!(!output.status.success());
    let report = parse(&output);
    assert_eq!(report["diagnostics"][0]["line"], 2);
    assert_eq!(report["diagnostics"][1]["line"], 3);

    for epoch in 2..=9 {
        let root = legacy_fixture(epoch);
        fs::write(
            root.path().join("agent-map.md"),
            "[Root](AGENTS.md)\r[Broken](missing.md)\r\n[Owner][undefined]\n",
        )
        .unwrap();
        let output = run(root.path(), &["check", "agent-map", "--json"]);
        assert!(!output.status.success(), "epoch {epoch}");
        let report = parse(&output);
        let broken = report["broken_links"].as_array().unwrap();
        assert_eq!(broken.len(), 2);
        assert!(broken[0].as_str().unwrap().starts_with("agent-map.md:2:"));
        assert!(broken[1].as_str().unwrap().starts_with("agent-map.md:3:"));
    }
}

#[cfg(unix)]
#[test]
fn unreadable_discovery_directories_report_errors_and_other_guides() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;

    let root = fixture("[Broken](missing.md)\n");
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let blocked = root.path().join("example-data");
    fs::create_dir(&blocked).unwrap();
    fs::write(blocked.join("AGENTS.md"), "[Hidden](hidden.md)\n").unwrap();
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    command
        .args(["check", "agent-guides", "--json"])
        .env_remove("JIG_REPO_ROOT")
        .current_dir(root.path());
    // Even a root test runner must exercise real permission failures.
    if unsafe { libc::geteuid() } == 0 {
        command.uid(65534).gid(65534);
    }
    let output = command.output().unwrap();
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!output.status.success());
    let report = parse(&output);
    assert_eq!(report["ok"], false);
    assert_eq!(report["guide_count"], 1);
    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert!(diagnostics.iter().any(|d| d["code"] == "guide_unreadable"
        && d["guide"] == "example-data"
        && d["severity"] == "error"));
    assert!(
        diagnostics
            .iter()
            .any(|d| d["code"] == "reference_missing" && d["guide"] == "AGENTS.md")
    );
    assert!(!report.to_string().contains("hidden.md"));
    for field in ["missing_guides", "missing_sections", "missing_entry_ref"] {
        assert_eq!(report[field], json!([]));
    }
}
