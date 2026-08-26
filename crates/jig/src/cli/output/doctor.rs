pub(in crate::cli) fn format_doctor_summary(value: &serde_json::Value) -> String {
    let ready = value["ok"].as_bool().unwrap_or(false);
    let mut lines = vec![format!(
        "Jig doctor: {}",
        if ready { "ready" } else { "needs attention" }
    )];
    if let Some(root) = value["repo"]["root"].as_str() {
        lines.push(format!("Repo: {root}"));
    }
    lines.push("Checks:".into());
    for check in value["checks"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let label = check["label"].as_str().unwrap_or("<unknown>");
        let status = check["status"].as_str().unwrap_or("unknown");
        let required = check["required"].as_bool().unwrap_or(false);
        let required_label = if required { "required" } else { "optional" };
        let ok = check["ok"].as_bool().unwrap_or(false);
        let marker = if ok {
            "ok"
        } else if required {
            "needs setup"
        } else {
            "optional setup"
        };
        lines.push(format!(
            "  - {label}: {marker} ({status}, {required_label})"
        ));
        if required
            && (!ok || status == "present_unverified")
            && let Some(detail) = check["detail"].as_str()
            && !detail.trim().is_empty()
        {
            lines.push(format!("    Detail: {detail}"));
        }
    }

    match summary_step(value, "next_required_step", true) {
        Some(step) => lines.push(format!("Next required step: {step}")),
        None => lines.push("Next required step: none".into()),
    }
    match summary_step(value, "optional_setup", false) {
        Some(step) => lines.push(format!("Optional setup: {}", optional_setup_label(step))),
        None => lines.push("Optional setup: none".into()),
    }
    lines.join("\n")
}

fn summary_step<'a>(value: &'a serde_json::Value, key: &str, required: bool) -> Option<&'a str> {
    value[key]
        .as_str()
        .or_else(|| {
            (required || value["ok"].as_bool().unwrap_or(false))
                .then(|| step_from_checks(value, required))
                .flatten()
        })
        .or_else(|| legacy_next_step(value, required))
}

fn step_from_checks(value: &serde_json::Value, required: bool) -> Option<&str> {
    value["checks"]
        .as_array()?
        .iter()
        .find(|check| {
            !check["ok"].as_bool().unwrap_or(false)
                && check["required"].as_bool().unwrap_or(false) == required
        })
        .and_then(|check| check["fix"].as_str())
}

fn legacy_next_step(value: &serde_json::Value, required: bool) -> Option<&str> {
    let ready = value["ok"].as_bool().unwrap_or(false);
    if ready == required {
        return None;
    }
    value["next_step"].as_str()
}

fn optional_setup_label(step: &str) -> &str {
    let Some(rest) = step.strip_prefix("Run `") else {
        return step;
    };
    let Some((command, _)) = rest.split_once('`') else {
        return step;
    };
    if command.starts_with("scripts/jig ") {
        command
    } else {
        step
    }
}
