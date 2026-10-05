use super::{concise_preview, value_str, value_u64};

pub(in crate::cli) fn format_summary(value: &serde_json::Value) -> String {
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
        "Loops: {leases} lease(s), {attempts} attempt(s), {exhausted} exhausted"
    ));

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
    fn surfaces_local_repository_loop_and_collection_state() {
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
            "loops": {
                "leases": [{}],
                "attempts": [{}, {}],
                "needs_attention": { "exhausted_attempts": [{}] }
            },
            "errors": [{
                "scope": "loops",
                "message": "one malformed attempt was omitted"
            }]
        }));

        assert!(summary.contains("Collection: partial"));
        assert!(summary.contains("rewrite main@1234567890ab (dirty)"));
        assert!(summary.contains("origin/main (ahead 2, behind 1; local ref)"));
        assert!(!summary.contains("Work:"));
        assert!(summary.contains("1 lease(s), 2 attempt(s), 1 exhausted"));
        assert!(summary.contains("Collection errors:"));
        assert!(summary.contains("loops: one malformed attempt was omitted"));
    }
}
