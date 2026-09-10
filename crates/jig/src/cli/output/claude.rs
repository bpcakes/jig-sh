use jig_tui::sanitize_text;
use serde_json::Value;

use crate::agent_provider::AgentProvider;
use crate::claude::provider::Claude;

use super::command_display::CommandDisplay;

pub(super) fn homes_summary(value: &Value) -> String {
    let mut lines = vec!["Claude homes".to_owned()];
    if let Some(homes) = value["homes"].as_array() {
        for home in homes {
            let marker = if home["current"] == true { "*" } else { " " };
            lines.push(format!(
                "  {marker} {}  {}{}",
                text(&home["name"]),
                text(&home["path"]),
                if home["default_config"] == true {
                    " [default config]"
                } else {
                    ""
                }
            ));
            if value["usage_included"] == true {
                if let Some(plan) = home["account"]["plan_type"].as_str() {
                    lines.push(format!("      Plan: {}", sanitize_text(plan)));
                }
                if let Some(error) = home["inspection_error"]
                    .as_str()
                    .or_else(|| home["usage_error"].as_str())
                {
                    lines.push(format!("      Usage unavailable: {}", sanitize_text(error)));
                } else {
                    lines.push(format!(
                        "      {}",
                        super::usage::format_limits(
                            &home["rate_limits"],
                            Claude::METADATA.subscription_bucket
                        )
                    ));
                }
            }
        }
        if homes.is_empty() {
            lines.push("  No Claude homes found. Create ~/.claude or ~/.claude-NAME, then launch it with -- auth login.".into());
        } else {
            lines.push("  * = current configuration mode".into());
        }
    }
    if let Some(warnings) = value["warnings"].as_array() {
        for warning in warnings {
            lines.push(format!("Warning: {}", text(warning)));
        }
    }
    lines.join("\n")
}

pub(super) fn launch_summary(value: &Value) -> String {
    let mut display = CommandDisplay::default();
    let args = value["args"]
        .as_array()
        .map(|args| {
            args.iter()
                .map(|arg| format!("{:?}", display.text(arg.as_str().unwrap_or_default())))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let output = format!(
        "Claude launch (dry run)\n  CLAUDE_CONFIG_DIR: {}\n  Executable: {}\n  Arguments (display only; not shell syntax): {args}",
        if value["config_dir"].is_null() {
            "unset (Claude default configuration)".to_owned()
        } else {
            display.text(value["config_dir"].as_str().unwrap_or_default())
        },
        display.text(value["claude_bin"].as_str().unwrap_or_default())
    );
    display.finish(output, value["representation_lossy"] == true)
}

fn text(value: &Value) -> String {
    sanitize_text(value.as_str().unwrap_or_default())
}
