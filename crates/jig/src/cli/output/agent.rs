use super::{concise_preview, value_bool, value_str};

pub(super) fn format_agent_doctor_summary(value: &serde_json::Value) -> String {
    let ready = value_bool(value, "ok").unwrap_or(false);
    let codex = &value["codex"];
    let codex_required = value_bool(codex, "required").unwrap_or(false);
    let codex_line = if codex_required {
        let codex_available = codex
            .get("available")
            .and_then(serde_json::Value::as_bool)
            .map(|available| {
                if available {
                    "available"
                } else {
                    "unavailable"
                }
            })
            .unwrap_or("unknown");
        format!("Codex: required ({codex_available})")
    } else {
        "Codex: not required (probe skipped)".into()
    };
    let mut lines = vec![
        format!(
            "Agent tooling: {}",
            if ready { "ready" } else { "needs setup" }
        ),
        codex_line,
    ];

    let marketplaces = value["marketplaces"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if marketplaces.is_empty() {
        lines.push("Marketplaces: none configured".into());
    } else {
        lines.push("Marketplaces:".into());
        for marketplace in marketplaces {
            let id = value_str(marketplace, "id").unwrap_or("<unknown>");
            let source = value_str(marketplace, "source").unwrap_or("<unknown>");
            let registered = value_bool(marketplace, "registered").unwrap_or(false);
            let configured = value_str(marketplace, "configured_source");
            let detail = match (registered, configured) {
                (true, _) => format!("registered ({source})"),
                (false, Some(configured)) => {
                    format!("not registered; repo config expects {source}, Codex has {configured}")
                }
                (false, None) => format!("missing registration for {source}"),
            };
            lines.push(format!("  - {id}: {detail}"));
        }
    }

    let next_steps = value["next_steps"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if next_steps.is_empty() {
        lines.push("Next steps: none".into());
    } else {
        lines.push("Next steps:".into());
        for step in next_steps {
            if let Some(step) = step.as_str() {
                lines.push(format!("  - {step}"));
            }
        }
    }

    lines.join("\n")
}

pub(super) fn format_agent_bootstrap_summary(value: &serde_json::Value) -> String {
    let ok = value_bool(value, "ok").unwrap_or(false);
    let marketplace = value_str(value, "marketplace_source").unwrap_or("<unknown>");
    let mut lines = vec![
        format!("Agent bootstrap: {}", if ok { "ok" } else { "failed" }),
        format!("  Marketplace: {marketplace}"),
    ];
    if let Some(stdout) = value_str(value, "stdout").filter(|text| !text.trim().is_empty()) {
        lines.push(format!("  stdout: {}", concise_preview(stdout, 160)));
    }
    if let Some(stderr) = value_str(value, "stderr").filter(|text| !text.trim().is_empty()) {
        lines.push(format!("  stderr: {}", concise_preview(stderr, 160)));
    }
    if ok {
        lines.push("Next step: scripts/jig agent doctor".into());
    } else {
        lines.push(
            "Next step: inspect the marketplace source, then rerun scripts/jig agent bootstrap"
                .into(),
        );
    }
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}
