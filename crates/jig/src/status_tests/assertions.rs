use super::*;

#[cfg(unix)]
pub(super) fn assert_current_status_snapshot(current: &Value, root: &Path) {
    assert_eq!(current["outcome"], "complete");
    assert_eq!(current["repository"]["dirty"], false);
    assert_eq!(current["work"]["state"]["counts"]["open_plans"], 0);
    assert_eq!(current["work"]["gates"], json!([]));
    assert!(current["loops"]["leases"].as_array().unwrap().is_empty());
    assert_eq!(current["providers"][0]["status"], "complete");
    assert_eq!(
        current["providers"][0]["input_freshness"][0]["status"],
        "current"
    );
    assert_eq!(current["providers"][0]["summary"]["blockers"], 1);
    assert!(!root.join(".agent/state").exists());
}

#[cfg(unix)]
pub(super) fn assert_changed_status_snapshots(ctx: &RepoContext, root: &Path, report_path: &Path) {
    fs::write(root.join("untracked.txt"), "local change").unwrap();
    let dirty = snapshot(ctx).unwrap();
    assert_eq!(dirty["outcome"], "complete");
    assert_eq!(dirty["repository"]["dirty"], true);
    assert_eq!(
        dirty["providers"][0]["input_freshness"][0]["status"],
        "dirty"
    );

    git(root, &["add", "untracked.txt"]);
    git(root, &["commit", "-m", "advance target"]);
    let stale = snapshot(ctx).unwrap();
    assert_eq!(stale["repository"]["dirty"], false);
    assert_eq!(
        stale["providers"][0]["input_freshness"][0]["status"],
        "stale"
    );
    assert_eq!(stale["outcome"], "complete");

    let partial_report = report_value(
        "partial",
        Some(&git_text_for_test(root, &["rev-parse", "HEAD"])),
    );
    fs::write(report_path, serde_json::to_vec(&partial_report).unwrap()).unwrap();
    let partial = snapshot(ctx).unwrap();
    assert_eq!(partial["providers"][0]["status"], "partial");
    assert_eq!(partial["outcome"], "partial");

    fs::write(report_path, "not JSON").unwrap();
    let failed = snapshot(ctx).unwrap();
    assert_eq!(failed["providers"][0]["status"], "failed");
    assert_eq!(failed["providers"][0]["report"], Value::Null);
    assert_eq!(failed["providers"][0]["error"]["code"], "invalid_json");
    assert_eq!(failed["outcome"], "partial");
    assert!(!root.join(".agent/state").exists());
}
