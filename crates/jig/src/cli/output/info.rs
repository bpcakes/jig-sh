const DEFAULT_MCP_COMMAND: &str = "scripts/jig mcp";

pub(in crate::cli) fn format_info_summary(value: &serde_json::Value) -> String {
    if value["command"].as_str() == Some("info commands") {
        return crate::info::format_commands_summary(value);
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
