use super::*;

const WORKSPACE_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

fn configured_repo(root: &Path, extra_config: &str) {
    TestRepoBuilder::new(root)
        .config(format!(
            "[work.tracker]\nkind = \"beads\"\nworkspace_id = \"{WORKSPACE_ID}\"\n{extra_config}"
        ))
        .write();
    fs::create_dir(root.join(".beads")).unwrap();
}

fn write_issue(root: &Path, relative: &str) {
    fs::write(
        root.join(relative),
        r#"{"id":"example-123","title":"Example task","description":"Example context","acceptance_criteria":"Example result","status":"open","priority":2,"issue_type":"task","created_at":"2026-01-02T03:04:05Z","updated_at":"2026-01-03T04:05:06Z","future_field":{"preserved":true}}
"#,
    )
    .unwrap();
}

#[test]
fn unconfigured_tracker_does_not_inspect_an_existing_store() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    fs::create_dir(temp.path().join(".beads")).unwrap();
    fs::write(temp.path().join(".beads/issues.jsonl"), b"not JSON").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let result = super::super::tracker::tracker_check(&ctx);

    assert!(result.ok);
    assert!(!result.required);
    assert_eq!(result.status, "not configured");
}

#[test]
fn configured_tracker_reads_jsonl_without_process_control() {
    let temp = tempdir().unwrap();
    configured_repo(temp.path(), "");
    write_issue(temp.path(), ".beads/issues.jsonl");
    fs::write(temp.path().join(".beads/beads.db"), b"ignored database").unwrap();
    let database_before = fs::read(temp.path().join(".beads/beads.db")).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let result = super::super::tracker::tracker_check(&ctx);

    assert!(result.ok, "{}", result.detail);
    assert!(result.required);
    assert_eq!(result.status, "ready");
    assert_eq!(result.data["profile"], crate::tracker::INPUT_PROFILE);
    assert_eq!(result.data["export"], ".beads/issues.jsonl");
    assert_eq!(result.data["issues"], 1);
    assert_eq!(
        result.data["supported_operations"][0],
        "read_issue_snapshot"
    );
    assert_eq!(result.data["write_authority"], false);
    assert_eq!(
        fs::read(temp.path().join(".beads/beads.db")).unwrap(),
        database_before
    );
}

#[test]
fn configured_tracker_accepts_the_legacy_export_name() {
    let temp = tempdir().unwrap();
    configured_repo(temp.path(), "");
    write_issue(temp.path(), ".beads/beads.jsonl");
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let result = super::super::tracker::tracker_check(&ctx);

    assert!(result.ok, "{}", result.detail);
    assert_eq!(result.data["export"], ".beads/beads.jsonl");
}

#[test]
fn missing_and_ambiguous_exports_have_actionable_failures() {
    let temp = tempdir().unwrap();
    configured_repo(temp.path(), "");
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let missing = super::super::tracker::tracker_check(&ctx);
    assert!(!missing.ok);
    assert_eq!(missing.status, "missing export");
    assert!(missing.fix.unwrap().contains(".beads/issues.jsonl"));

    write_issue(temp.path(), ".beads/issues.jsonl");
    write_issue(temp.path(), ".beads/beads.jsonl");
    let ambiguous = super::super::tracker::tracker_check(&ctx);
    assert!(!ambiguous.ok);
    assert_eq!(ambiguous.status, "ambiguous export");
    assert!(ambiguous.detail.contains("both .beads/issues.jsonl"));
}

#[test]
fn configured_manual_export_guidance_is_used_for_recovery() {
    let temp = tempdir().unwrap();
    configured_repo(
        temp.path(),
        "manual_export_guidance = \"Run the repository privacy-safe export helper.\"\n",
    );
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let result = super::super::tracker::tracker_check(&ctx);

    assert_eq!(
        result.fix.as_deref(),
        Some("Run the repository privacy-safe export helper.")
    );
}

#[test]
fn missing_tracker_directory_uses_manual_export_guidance_without_creating_it() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(format!(
            "[work.tracker]\nkind = \"beads\"\nworkspace_id = \"{WORKSPACE_ID}\"\nmanual_export_guidance = \"Run the ExampleProject export helper.\"\n"
        ))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let result = super::super::tracker::tracker_check(&ctx);

    assert!(!result.ok);
    assert_eq!(result.status, "missing export");
    assert_eq!(
        result.fix.as_deref(),
        Some("Run the ExampleProject export helper.")
    );
    assert!(!temp.path().join(".beads").exists());
}

#[cfg(unix)]
#[test]
fn configured_manual_export_guidance_does_not_hide_unsafe_export_repair() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    configured_repo(
        temp.path(),
        "manual_export_guidance = \"Run the repository privacy-safe export helper.\"\n",
    );
    let outside = temp.path().join("outside.jsonl");
    fs::write(&outside, "").unwrap();
    symlink(&outside, temp.path().join(".beads/issues.jsonl")).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let result = super::super::tracker::tracker_check(&ctx);

    assert_eq!(result.status, "unsafe export");
    let fix = result.fix.unwrap();
    assert!(
        fix.contains("real, private files rather than links"),
        "{fix}"
    );
    assert!(!fix.contains("privacy-safe export helper"), "{fix}");
}

#[test]
fn invalid_export_reports_structure_without_task_body_content() {
    let temp = tempdir().unwrap();
    configured_repo(temp.path(), "");
    fs::write(
        temp.path().join(".beads/issues.jsonl"),
        "{\"id\":\"private-task\",\"description\":\"private body\"}\n",
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let result = super::super::tracker::tracker_check(&ctx);

    assert!(!result.ok);
    assert_eq!(result.status, "invalid export");
    assert!(result.detail.contains("line 1"));
    assert!(!result.detail.contains("private"));
}

#[cfg(unix)]
#[test]
fn symlinked_export_is_rejected() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    configured_repo(temp.path(), "");
    let outside = temp.path().join("outside.jsonl");
    fs::write(&outside, "").unwrap();
    symlink(&outside, temp.path().join(".beads/issues.jsonl")).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let result = super::super::tracker::tracker_check(&ctx);

    assert!(!result.ok);
    assert_eq!(result.status, "unsafe export");
}

#[cfg(unix)]
#[test]
fn tracker_only_repository_does_not_require_a_process_signal_session() {
    let temp = tempdir().unwrap();
    configured_repo(
        temp.path(),
        "\n[dev]\nworkspace_discovery = false\n\n[agent_tooling.codex]\nmarketplaces = []\n",
    );
    write_issue(temp.path(), ".beads/issues.jsonl");
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    assert!(!ctx.sqlx_enabled());
    assert!(!rust_runtime_probe_required(&ctx));
    assert!(!go_runtime_probe_required(&ctx));
    assert!(!node_runtime_probe_required(&ctx));
    assert!(ctx.codex_marketplaces().is_empty());
    assert!(!proxy_configured(&ctx));
    assert!(!doctor_process_session_required(&ctx));
    assert!(super::super::tracker::tracker_check(&ctx).ok);
}
