use super::{value_bool, value_i64, value_str, value_u64};

pub(super) fn format_dev_summary(value: &serde_json::Value) -> String {
    let routes = value["routes"].as_array().map(Vec::len).unwrap_or(0);
    if value_bool(value, "stopped").unwrap_or(false) {
        let reason = value_str(value, "stop_reason").unwrap_or("requested");
        let mut lines = vec![format!("Dev: stopped ({reason})")];
        append_recovery_messages(&mut lines, value);
        lines.push("  full report: rerun with --json".into());
        return lines.join("\n");
    }
    if value_bool(value, "interrupted").unwrap_or(false) {
        let signal = value_str(value, "termination_signal").unwrap_or("signal");
        let mut lines = vec![format!("Dev: stopped ({signal})")];
        append_recovery_messages(&mut lines, value);
        lines.push("  full report: rerun with --json".into());
        return lines.join("\n");
    }

    let ok = value_bool(value, "ok").unwrap_or(false);
    let mut lines = vec![
        format!("Dev: {}", if ok { "ok" } else { "failed" }),
        format!("  Routes: {routes}"),
    ];
    if let Some(app) = value_str(&value["first_exit"], "app") {
        let exit = value_i64(&value["first_exit"], "exit_status")
            .map(|status| status.to_string())
            .unwrap_or_else(|| "?".into());
        lines.push(format!("  First exit: {app} (exit {exit})"));
    }
    if value_bool(value, "proxy_failed").unwrap_or(false) {
        lines.push("  Proxy: failed".into());
    }
    if value_bool(value, "cleanup_unconfirmed").unwrap_or(false) {
        lines.push("  Cleanup: unconfirmed; session retained for inspection".into());
    } else if let Some(error) = value_str(value, "error") {
        lines.push(format!("  Error: {error}"));
    }
    append_recovery_messages(&mut lines, value);
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

pub(super) fn format_dev_status_summary(value: &serde_json::Value) -> String {
    let sessions = value["sessions"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let running = value_bool(value, "running").unwrap_or_else(|| {
        sessions.iter().any(|session| {
            value_str(session, "status") == Some("running")
                || value_bool(session, "supervisor_alive").unwrap_or(false)
        })
    });
    let mut lines = vec![format!(
        "Dev status: {}",
        if running { "running" } else { "stopped" }
    )];
    append_dev_repo_and_state(&mut lines, value);
    lines.push(format!("  Sessions: {}", sessions.len()));

    for session in sessions {
        let session_id = value_str(session, "session_id")
            .or_else(|| value_str(session, "id"))
            .unwrap_or("<unknown>");
        let status = value_str(session, "status").unwrap_or_else(|| {
            if value_bool(session, "supervisor_alive").unwrap_or(false) {
                "running"
            } else {
                "stale"
            }
        });
        let supervisor_pid = value_u64(session, "supervisor_pid")
            .or_else(|| value_u64(&session["supervisor"], "pid"));
        let app_count = session["apps"].as_array().map(Vec::len).unwrap_or(0);
        let pid = supervisor_pid
            .map(|pid| format!("supervisor PID {pid}"))
            .unwrap_or_else(|| "supervisor PID unknown".into());
        let app_label = if app_count == 1 { "app" } else { "apps" };
        lines.push(format!(
            "  - {session_id}: {status}, {pid}, {app_count} {app_label}"
        ));
    }

    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

pub(super) fn format_dev_stop_summary(value: &serde_json::Value) -> String {
    let ok = value_bool(value, "ok").unwrap_or(false);
    let sessions = value["sessions"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let matched = value_u64(value, "matched_sessions").unwrap_or(sessions.len() as u64);
    let stopped = value_u64(value, "stopped_sessions").unwrap_or_else(|| {
        sessions
            .iter()
            .filter(|session| {
                matches!(
                    value_str(session, "outcome"),
                    Some("stopped" | "already-stopped")
                )
            })
            .count() as u64
    });
    let status = if !ok {
        "incomplete"
    } else if matched == 0 {
        "nothing running"
    } else {
        "stopped"
    };
    let mut lines = vec![format!("Dev stop: {status}")];
    append_dev_repo_and_state(&mut lines, value);
    lines.push(format!("  Sessions matched: {matched}"));
    lines.push(format!("  Sessions stopped: {stopped}"));
    if let Some(apps) = value_u64(value, "stopped_apps") {
        lines.push(format!("  Apps stopped: {apps}"));
    }
    if let Some(warning) = value_str(value, "warning") {
        lines.push(format!("  Warning: {warning}"));
    }
    if let Some(warnings) = value["warnings"].as_array() {
        for warning in warnings.iter().filter_map(serde_json::Value::as_str) {
            lines.push(format!("  Warning: {warning}"));
        }
    }
    append_recovery_messages(&mut lines, value);
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

fn append_recovery_messages(lines: &mut Vec<String>, value: &serde_json::Value) {
    if let Some(recoveries) = value["recoveries"].as_array() {
        for recovery in recoveries {
            if let Some(message) = value_str(recovery, "message") {
                lines.push(format!("  Recovery: {message}"));
            }
        }
    }
}

fn append_dev_repo_and_state(lines: &mut Vec<String>, value: &serde_json::Value) {
    match (value_str(value, "repo_name"), value_str(value, "repo_root")) {
        (Some(name), Some(root)) => lines.push(format!("  Repo: {name} ({root})")),
        (Some(name), None) => lines.push(format!("  Repo: {name}")),
        (None, Some(root)) => lines.push(format!("  Repo: {root}")),
        (None, None) => {}
    }
    if let Some(state_dir) = value_str(value, "state_dir") {
        lines.push(format!("  State: {state_dir}"));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn dev_summary_reports_ctrl_c_as_stopped() {
        let summary = format_dev_summary(&json!({
            "ok": false,
            "interrupted": true,
            "exit_status": 130,
            "exit_signal": 2,
            "termination_signal": "SIGINT",
            "first_exit": null,
            "proxy_failed": false,
            "routes": [],
            "recoveries": [{
                "message": "retired an earlier dead orphan"
            }]
        }));

        assert!(summary.contains("Dev: stopped (SIGINT)"));
        assert!(summary.contains("Recovery: retired an earlier dead orphan"));
        assert!(!summary.contains("Routes:"));
        assert!(!summary.contains("failed"));
        assert!(!summary.contains("First exit"));
    }

    #[test]
    fn dev_summary_reports_management_requested_stop() {
        let summary = format_dev_summary(&json!({
            "ok": true,
            "interrupted": false,
            "stopped": true,
            "stop_reason": "dev stop",
            "routes": [],
            "recoveries": [{
                "message": "retired an earlier dead orphan"
            }]
        }));

        assert!(summary.contains("Dev: stopped (dev stop)"));
        assert!(summary.contains("Recovery: retired an earlier dead orphan"));
        assert!(!summary.contains("Dev: ok"));
    }

    #[test]
    fn dev_summary_reports_unconfirmed_cleanup_failure() {
        let summary = format_dev_summary(&json!({
            "ok": false,
            "interrupted": false,
            "stopped": false,
            "cleanup_unconfirmed": true,
            "routes": [],
            "recoveries": [{
                "message": "retired an earlier dead orphan"
            }]
        }));

        assert!(summary.contains("Dev: failed"));
        assert!(summary.contains("Cleanup: unconfirmed; session retained for inspection"));
        assert!(summary.contains("Recovery: retired an earlier dead orphan"));
    }

    #[test]
    fn dev_summary_reports_replacement_recovery() {
        let summary = format_dev_summary(&json!({
            "ok": true,
            "first_exit": {"app": "web", "exit_status": 0},
            "proxy_failed": false,
            "routes": [],
            "recoveries": [{
                "kind": "dead-orphan-retired",
                "message": "session 'dev_123': retired a dead orphan"
            }]
        }));

        assert!(summary.contains("Dev: ok"));
        assert!(summary.contains("Recovery: session 'dev_123': retired a dead orphan"));
    }

    #[test]
    fn dev_summary_reports_failure_after_replacement_recovery() {
        let summary = format_dev_summary(&json!({
            "ok": false,
            "error": "replacement launch failed",
            "first_exit": null,
            "proxy_failed": false,
            "routes": [],
            "recoveries": [{
                "kind": "dead-orphan-retired",
                "message": "session 'dev_123': retired a dead orphan"
            }]
        }));

        assert!(summary.contains("Dev: failed"));
        assert!(summary.contains("Error: replacement launch failed"));
        assert!(summary.contains("Recovery: session 'dev_123': retired a dead orphan"));
    }

    #[test]
    fn dev_status_summary_reports_registered_sessions() {
        let summary = format_dev_status_summary(&json!({
            "ok": true,
            "repo_name": "demo",
            "repo_root": "/tmp/demo",
            "state_dir": "/tmp/proxy",
            "running": true,
            "sessions": [{
                "session_id": "dev_123",
                "status": "running",
                "supervisor_pid": 4242,
                "apps": [
                    {"name": "api"},
                    {"name": "web"}
                ]
            }]
        }));

        assert!(summary.contains("Dev status: running"));
        assert!(summary.contains("Repo: demo (/tmp/demo)"));
        assert!(summary.contains("State: /tmp/proxy"));
        assert!(summary.contains("Sessions: 1"));
        assert!(summary.contains("dev_123: running"));
        assert!(summary.contains("supervisor PID 4242"));
        assert!(summary.contains("2 apps"));
    }

    #[test]
    fn dev_status_summary_reports_no_running_sessions() {
        let summary = format_dev_status_summary(&json!({
            "ok": true,
            "repo_root": "/tmp/demo",
            "running": false,
            "sessions": []
        }));

        assert!(summary.contains("Dev status: stopped"));
        assert!(summary.contains("Sessions: 0"));
    }

    #[test]
    fn dev_status_summary_distinguishes_a_recoverable_orphan_from_running() {
        let summary = format_dev_status_summary(&json!({
            "ok": true,
            "repo_name": "demo",
            "running": false,
            "sessions": [{
                "session_id": "dev_recoverable",
                "status": "recoverable",
                "recoverable": true,
                "supervisor_pid": 4242,
                "apps": [{"name": "web"}]
            }]
        }));

        assert!(summary.contains("Dev status: stopped"));
        assert!(summary.contains("dev_recoverable: recoverable"));
    }

    #[test]
    fn dev_stop_summary_distinguishes_idempotent_and_incomplete_results() {
        let nothing_running = format_dev_stop_summary(&json!({
            "ok": true,
            "repo_name": "demo",
            "repo_root": "/tmp/demo",
            "matched_sessions": 0,
            "stopped_sessions": 0,
            "stopped_apps": 0,
            "sessions": [],
            "recoveries": [],
            "warnings": []
        }));
        assert!(nothing_running.contains("Dev stop: nothing running"));
        assert!(nothing_running.contains("Sessions matched: 0"));

        let incomplete = format_dev_stop_summary(&json!({
            "ok": false,
            "repo_name": "demo",
            "repo_root": "/tmp/demo",
            "matched_sessions": 1,
            "stopped_sessions": 0,
            "sessions": [{
                "session_id": "dev_123",
                "outcome": "orphaned"
            }],
            "warnings": ["exact process identity could not be confirmed stopped"]
        }));
        assert!(incomplete.contains("Dev stop: incomplete"));
        assert!(incomplete.contains("Sessions matched: 1"));
        assert!(incomplete.contains("Sessions stopped: 0"));
        assert!(incomplete.contains("exact process identity could not be confirmed stopped"));

        let recovered = format_dev_stop_summary(&json!({
            "ok": true,
            "matched_sessions": 1,
            "stopped_sessions": 1,
            "stopped_apps": 0,
            "sessions": [],
            "recoveries": [{
                "kind": "dead-orphan-retired",
                "message": "session 'dev_456': retired a dead orphan; retired app diagnostics: web (target 127.0.0.1:4005, last PID 4242, spawn registered)"
            }],
            "warnings": []
        }));
        assert!(recovered.contains("Dev stop: stopped"));
        assert!(recovered.contains("Recovery: session 'dev_456'"));
        assert!(recovered.contains("web (target 127.0.0.1:4005, last PID 4242"));
        assert!(!recovered.contains("Warning:"));
    }
}
