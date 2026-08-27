use super::{concise_preview, value_i64, value_str, value_u64};

const RECENT_RECEIPT_SUMMARY_LIMIT: usize = 5;

pub(super) fn format_work_summary(value: &serde_json::Value) -> String {
    let counts = &value["counts"];
    let repo = &value["repo"];
    let repo_name = value_str(repo, "name").unwrap_or("<unknown>");
    let default_branch = value_str(repo, "default_branch").unwrap_or("<unknown>");
    let open_plan_count = value_u64(counts, "open_plans").unwrap_or(0);
    let receipt_count = value_u64(counts, "receipts").unwrap_or(0);
    let failed_receipt_count = value_u64(counts, "failed_receipts").unwrap_or(0);
    let decision_count = value_u64(counts, "decisions").unwrap_or(0);

    let mut lines = vec![
        "Work status:".into(),
        format!("  Plans: {open_plan_count} open"),
        format!("  Receipts: {receipt_count} total, {failed_receipt_count} failed"),
        format!("  Decisions: {decision_count}"),
        format!("Repo: {repo_name} ({default_branch})"),
        format!(
            "Current session: {}",
            value_str(value, "current_session_id").unwrap_or("none")
        ),
    ];

    let open_plans = value["open_plans"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if open_plans.is_empty() {
        lines.push("Open plans: none".into());
    } else {
        lines.push("Open plans:".into());
        for plan in open_plans {
            let plan_id = value_str(plan, "plan_id").unwrap_or("<unknown>");
            let title = value_str(plan, "title").unwrap_or("<untitled>");
            lines.push(format!("  - {plan_id}: {title}"));
        }
    }

    let recent_receipts = value["recent_receipts"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if recent_receipts.is_empty() {
        lines.push("Recent receipts: none".into());
    } else {
        lines.push("Recent receipts:".into());
        for receipt in recent_receipts.iter().take(RECENT_RECEIPT_SUMMARY_LIMIT) {
            let id = value_str(receipt, "id").unwrap_or("<unknown>");
            let tool = value_str(receipt, "tool_name").unwrap_or("<unknown>");
            let exit_status = value_i64(receipt, "exit_status")
                .map(|status| status.to_string())
                .unwrap_or_else(|| "?".into());
            let diff = value_str(receipt, "diff_summary").unwrap_or("unknown diff");
            lines.push(format!("  - {tool} ({id}): exit {exit_status}, {diff}"));
        }
        if recent_receipts.len() > RECENT_RECEIPT_SUMMARY_LIMIT {
            let hidden = recent_receipts.len() - RECENT_RECEIPT_SUMMARY_LIMIT;
            let noun = if hidden == 1 { "receipt" } else { "receipts" };
            lines.push(format!(
                "  (and {hidden} more recent {noun}; rerun with --json for full output)"
            ));
        }
    }

    lines.join("\n")
}

pub(super) fn format_summary(value: &serde_json::Value) -> String {
    let outcome = value_str(value, "outcome").unwrap_or("unknown");
    let repository = &value["repository"];
    let repo_name = value_str(repository, "name").unwrap_or("<unknown>");
    let branch = value_str(repository, "branch").unwrap_or("detached");
    let revision = value_str(repository, "head_revision")
        .map(|revision| revision.chars().take(12).collect::<String>())
        .unwrap_or_else(|| "no HEAD".into());
    let worktree = repository
        .get("dirty")
        .and_then(serde_json::Value::as_bool)
        .map(|dirty| if dirty { "dirty" } else { "clean" })
        .unwrap_or("unknown");

    let work_state = &value["work"]["state"];
    let open_plans = value_u64(&work_state["counts"], "open_plans").unwrap_or(0);
    let session = value_str(work_state, "current_session_id").unwrap_or("none");
    let loops = &value["loops"];
    let leases = loops["leases"].as_array().map(Vec::len).unwrap_or(0);
    let attempts = loops["attempts"].as_array().map(Vec::len).unwrap_or(0);
    let exhausted = loops["needs_attention"]["exhausted_attempts"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);

    let mut lines = vec![
        format!("Collection: {outcome}"),
        format!("Repo: {repo_name} {branch}@{revision} ({worktree})"),
    ];
    if let Some(upstream) = repository.get("upstream").filter(|value| !value.is_null()) {
        let reference = value_str(upstream, "reference").unwrap_or("<unknown>");
        let ahead = value_u64(upstream, "ahead").unwrap_or(0);
        let behind = value_u64(upstream, "behind").unwrap_or(0);
        lines.push(format!(
            "Tracking: {reference} (ahead {ahead}, behind {behind}; local ref)"
        ));
    } else {
        lines.push("Tracking: none".into());
    }
    lines.push(format!(
        "Work: {open_plans} open plan(s), session {session}"
    ));
    lines.push(format!(
        "Loops: {leases} lease(s), {attempts} attempt(s), {exhausted} exhausted"
    ));

    let providers = value["providers"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if providers.is_empty() {
        lines.push("Providers: none configured".into());
    } else {
        lines.push("Providers:".into());
        for provider in providers {
            let id = value_str(provider, "id").unwrap_or("<unknown>");
            let status = value_str(provider, "status").unwrap_or("unknown");
            let duration = value_u64(provider, "duration_ms").unwrap_or(0);
            if let Some(summary) = provider.get("summary").filter(|value| !value.is_null()) {
                let packages = value_u64(summary, "work_packages").unwrap_or(0);
                let blockers = value_u64(summary, "blockers").unwrap_or(0);
                let diagnostics = value_u64(&summary["diagnostics"], "total").unwrap_or(0);
                lines.push(format!(
                    "  - {id}: {status}; {packages} package(s), {blockers} blocker(s), {diagnostics} diagnostic(s), {duration} ms"
                ));
            } else {
                let error = provider["error"]["message"]
                    .as_str()
                    .map(|message| concise_preview(message, 240))
                    .unwrap_or_else(|| "provider failed without a diagnostic".into());
                lines.push(format!("  - {id}: {status}; {duration} ms"));
                lines.push(format!("    {error}"));
            }
            let inputs = provider["input_freshness"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            if !inputs.is_empty() {
                let inputs = inputs
                    .iter()
                    .map(|input| {
                        format!(
                            "{}={}",
                            value_str(input, "name").unwrap_or("<unknown>"),
                            value_str(input, "status").unwrap_or("unknown")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(format!("    Inputs: {inputs}"));
            }
        }
    }

    let errors = value["errors"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    if !errors.is_empty() {
        lines.push("Collection errors:".into());
        for error in errors {
            let scope = value_str(error, "scope").unwrap_or("<unknown>");
            let message = value_str(error, "message")
                .map(|message| concise_preview(message, 240))
                .unwrap_or_else(|| "unknown error".into());
            lines.push(format!("  - {scope}: {message}"));
        }
    }
    lines.push("Full report: rerun with --json".into());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn surfaces_provider_and_freshness_state() {
        let summary = format_summary(&json!({
            "outcome": "partial",
            "repository": {
                "name": "rewrite",
                "branch": "main",
                "head_revision": "1234567890abcdef",
                "dirty": true,
                "upstream": {
                    "reference": "origin/main",
                    "ahead": 2,
                    "behind": 1
                }
            },
            "work": {
                "state": {
                    "current_session_id": "session_1",
                    "counts": { "open_plans": 3 }
                }
            },
            "loops": {
                "leases": [{}],
                "attempts": [{}, {}],
                "needs_attention": { "exhausted_attempts": [{}] }
            },
            "providers": [{
                "id": "example.rewrite",
                "status": "complete",
                "duration_ms": 42,
                "summary": {
                    "work_packages": 130,
                    "blockers": 4,
                    "diagnostics": { "total": 1 }
                },
                "input_freshness": [
                    { "name": "target", "status": "dirty" },
                    { "name": "legacy", "status": "current" }
                ],
                "error": null
            }, {
                "id": "example.failed",
                "status": "failed",
                "duration_ms": 1000,
                "summary": null,
                "input_freshness": [],
                "error": {
                    "message": "provider timed out"
                }
            }],
            "errors": []
        }));

        assert!(summary.contains("Collection: partial"));
        assert!(summary.contains("rewrite main@1234567890ab (dirty)"));
        assert!(summary.contains("origin/main (ahead 2, behind 1; local ref)"));
        assert!(summary.contains("3 open plan(s), session session_1"));
        assert!(summary.contains("1 lease(s), 2 attempt(s), 1 exhausted"));
        assert!(summary.contains("130 package(s), 4 blocker(s), 1 diagnostic(s), 42 ms"));
        assert!(summary.contains("Inputs: target=dirty, legacy=current"));
        assert!(summary.contains("example.failed: failed; 1000 ms"));
        assert!(summary.contains("provider timed out"));
    }
}
