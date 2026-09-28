//! Seeds an open plan the way the removed `jig work start` recorded it, for
//! tests of the plan readers and `--plan-id` linkage that remain.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

pub fn seed_open_plan(root: &Path, plan_id: &str, title: &str) -> String {
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8(output.stdout).unwrap().trim().to_owned());
    let body_path = format!(".agent/plans/{plan_id}.md");
    fs::create_dir_all(root.join(".agent/plans")).unwrap();
    fs::write(root.join(&body_path), format!("# {title}\n")).unwrap();
    fs::create_dir_all(root.join(".agent/state")).unwrap();
    let timestamp_ms = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let baseline = match &head {
        Some(commit) => json!({"requested_ref": "HEAD", "commit_oid": commit, "error": null}),
        None => json!({"requested_ref": "HEAD", "commit_oid": null, "error": "HEAD is unborn"}),
    };
    let event = json!({
        "id": format!("plan-event_{plan_id}"),
        "plan_id": plan_id,
        "event": "open",
        "timestamp_ms": timestamp_ms,
        "title": title,
        "body_path": body_path,
        "baseline": baseline,
    });
    let mut plans = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(".agent/state/plans.jsonl"))
        .unwrap();
    writeln!(plans, "{event}").unwrap();
    plan_id.to_owned()
}
