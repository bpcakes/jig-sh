const DEFAULT_MCP_COMMAND: &str = "scripts/jig mcp";

pub(in crate::cli) fn format_info_summary(value: &serde_json::Value) -> String {
    if value["command"].as_str() == Some("info commands") {
        return crate::info::format_commands_summary(value);
    }
    if value["command"]
        .as_str()
        .is_some_and(|command| command.starts_with("info "))
    {
        return format_repository_info(value);
    }

    let repo = &value["repo"];
    let mut lines = vec![
        format!("Jig info: {}", repo["name"].as_str().unwrap_or("<unknown>")),
        format!(
            "Template source: {} @ {}",
            repo["template_source"].as_str().unwrap_or("<unknown>"),
            repo["template_commit"].as_str().unwrap_or("<unknown>")
        ),
        format!(
            "Runtime: jig {} · contract v{}",
            repo["runtime_version"].as_str().unwrap_or("<unknown>"),
            repo["contract_version"].as_u64().unwrap_or(0)
        ),
    ];

    lines.push(format!(
        "Capabilities: {}",
        enabled_capabilities(value).join(", ")
    ));
    lines.push(format!(
        "Check tools: {}",
        string_list(value["check_tools"].as_array()).join(", ")
    ));
    lines.push(format!(
        "Work gates: {}",
        value["work_gates"].as_array().map(Vec::len).unwrap_or(0)
    ));
    lines.push(format!(
        "Dev apps: {}",
        value["dev_apps"].as_array().map(Vec::len).unwrap_or(0)
    ));
    let mcp_source = value["mcp_command_source"].as_str().unwrap_or("default");
    lines.push(format!(
        "MCP command ({}): {}",
        mcp_source,
        value["mcp_command"].as_str().unwrap_or(DEFAULT_MCP_COMMAND)
    ));
    if let Some(error) = value["mcp_command_error"].as_str() {
        lines.push(format!("MCP command fallback: {error}"));
    }
    lines.join("\n")
}

fn format_repository_info(value: &serde_json::Value) -> String {
    let command = value["command"].as_str().unwrap_or("info workspace");
    let workspace = &value["workspace"];
    let mut lines = vec![format!(
        "Jig {}: {} (contract v{})",
        command.trim_start_matches("info "),
        workspace["name"].as_str().unwrap_or("<unknown>"),
        workspace["contract_version"].as_u64().unwrap_or(0)
    )];

    if let Some(component) = value.get("component") {
        lines.push(format!(
            "  Component: {} · root {}",
            component["id"].as_str().unwrap_or("?"),
            component["root"].as_str().unwrap_or("?")
        ));
    }
    if let Some(profile) = value.get("profile") {
        lines.push(format!(
            "  Profile: {}{}",
            profile["id"].as_str().unwrap_or("?"),
            if value["is_default_check_profile"].as_bool().unwrap_or(false) {
                " (default)"
            } else {
                ""
            }
        ));
    }
    if let Some(components) = value["components"].as_array() {
        lines.push(format!("  Components: {}", components.len()));
        for item in components {
            let component = item.get("component").unwrap_or(item);
            lines.push(format!(
                "  - {} · root {}",
                component["id"].as_str().unwrap_or("?"),
                component["root"].as_str().unwrap_or("?")
            ));
        }
    }
    if let Some(target) = value.get("target") {
        lines.push(format!("  Target: {}", target_text(&target["id"])));
        lines.push(format!(
            "  Intent: {} · effects {}",
            target["intent"].as_str().unwrap_or("?"),
            string_list(target["effects"].as_array()).join(", ")
        ));
    }
    if let Some(targets) = value["targets"].as_array() {
        lines.push(format!("  Targets: {}", targets.len()));
        for target in targets {
            lines.push(format!("  - {}", target_text(&target["id"])));
        }
    }
    if let Some(profiles) = value["profiles"].as_array() {
        lines.push(format!("  Profiles: {}", profiles.len()));
        for profile in profiles {
            lines.push(format!(
                "  - {} · {} targets",
                profile["id"].as_str().unwrap_or("?"),
                profile["targets"].as_array().map_or(0, Vec::len)
            ));
        }
    }
    lines.push("  full record: rerun with --json".into());
    lines.join("\n")
}

fn target_text(value: &serde_json::Value) -> String {
    format!(
        "{}:{}",
        value["component"].as_str().unwrap_or("?"),
        value["action"].as_str().unwrap_or("?")
    )
}

fn enabled_capabilities(value: &serde_json::Value) -> Vec<&'static str> {
    let capabilities = &value["capabilities"];
    let mut enabled = Vec::new();
    for (key, label) in [
        ("sqlx", "SQLx"),
        ("schema_dumps", "schema dumps"),
        ("frontend_apps", "frontend apps"),
        ("dev_proxy", "dev proxy"),
    ] {
        if capabilities[key].as_bool().unwrap_or(false) {
            enabled.push(label);
        }
    }
    if capabilities["vault_initialized"].as_bool().unwrap_or(false) {
        enabled.push("vault initialized");
    } else if capabilities["vault_available"].as_bool().unwrap_or(false) {
        enabled.push("vault available (not initialized)");
    }
    if enabled.is_empty() {
        enabled.push("none");
    }
    enabled
}

fn string_list(values: Option<&Vec<serde_json::Value>>) -> Vec<String> {
    match values {
        Some(values) => values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_string)
            .collect(),
        None => Vec::new(),
    }
}
