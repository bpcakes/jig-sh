use jig_tui::sanitize_text;

use super::{value_bool, value_str};

pub(super) fn format_codex_homes_summary(value: &serde_json::Value) -> String {
    format_codex_homes(value)
}

fn format_codex_homes(value: &serde_json::Value) -> String {
    let homes = value["homes"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let usage_included = value_bool(value, "usage_included").unwrap_or(false);
    let outcome = sanitize_text(value_str(value, "outcome").unwrap_or("complete"));
    let mut lines = vec![format!("Codex homes: {} found ({outcome})", homes.len())];
    if homes.is_empty() {
        lines.push("  No directories found under ~/.codex or ~/.codex-*".into());
    } else {
        for home in homes {
            let marker = if value_bool(home, "current").unwrap_or(false) {
                "*"
            } else {
                " "
            };
            let prefix = format!(" {marker}");
            lines.push(format!(
                "{prefix} {}",
                format_codex_home_fields(home, usage_included)
            ));
        }
        lines.push("  * = current CODEX_HOME".into());
    }
    append_discovery_warnings(value, &mut lines);
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

fn append_discovery_warnings(value: &serde_json::Value, lines: &mut Vec<String>) {
    let warnings = value["errors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|error| value_str(error, "kind") == Some("discovery"))
        .filter_map(|error| value_str(error, "message"))
        .map(sanitize_text)
        .collect::<Vec<_>>();
    if warnings.is_empty() {
        return;
    }
    lines.push("  Discovery warnings:".into());
    lines.extend(
        warnings
            .into_iter()
            .map(|warning| format!("    - {warning}")),
    );
}

fn format_codex_home_fields(home: &serde_json::Value, usage_included: bool) -> String {
    let name = sanitize_text(value_str(home, "name").unwrap_or("<unknown>"));
    let account = format_codex_account(&home["account"]);
    let plan = sanitize_text(value_str(&home["account"], "plan_type").unwrap_or("-"));
    let account_observed = home["account"].is_object();
    let inspection_error = value_str(home, "inspection_error");
    let usage_error = value_str(home, "usage_error");
    let mut fields = vec![name, account, plan];
    if usage_included && account_observed && inspection_error.is_none() && usage_error.is_none() {
        fields.push(format_codex_limits(&home["rate_limits"]));
    }
    if let Some(status) = value_str(home, "status") {
        fields.push(sanitize_text(status));
    }
    if let Some(error) = inspection_error {
        fields.push(format!("inspection error: {}", sanitize_text(error)));
    }
    if let Some(error) = usage_error.filter(|_| account_observed) {
        fields.push(format!("usage error: {}", sanitize_text(error)));
    }
    fields.join("  |  ")
}

fn format_codex_account(account: &serde_json::Value) -> String {
    if account.is_null() {
        return "-".into();
    }
    sanitize_text(
        value_str(account, "email")
            .or_else(|| value_str(account, "type"))
            .unwrap_or("-"),
    )
}

fn format_codex_limits(value: &serde_json::Value) -> String {
    let Some(buckets) = value.as_array() else {
        return "usage unavailable".into();
    };
    if buckets.is_empty() {
        return "usage unavailable".into();
    }
    buckets
        .iter()
        .map(|bucket| {
            let is_codex = value_str(bucket, "id") == Some("codex");
            let label = sanitize_text(
                value_str(bucket, "name")
                    .or_else(|| value_str(bucket, "id"))
                    .unwrap_or("limit"),
            );
            let mut windows = [&bucket["primary"], &bucket["secondary"]]
                .into_iter()
                .filter(|window| window.is_object())
                .collect::<Vec<_>>();
            windows.sort_by_key(|window| {
                window
                    .get("duration_minutes")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(u64::MAX)
            });
            match windows.as_slice() {
                [window] if is_codex => format!(
                    "{label}: weekly {}",
                    format_codex_window(window).expect("window was checked above")
                ),
                [window] => format!(
                    "{label}: {}",
                    format_codex_window(window).expect("window was checked above")
                ),
                [first, second] if is_codex => format!(
                    "{label}: {}, {}",
                    format_codex_window_with_duration_role(first),
                    format_codex_window_with_duration_role(second)
                ),
                [first, second] => format!(
                    "{label}: {}, {}",
                    format_codex_window(first).expect("window was checked above"),
                    format_codex_window(second).expect("window was checked above")
                ),
                [] => format!("{label}: unavailable"),
                _ => unreachable!("a Codex rate-limit bucket has at most two windows"),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn format_codex_window_with_duration_role(window: &serde_json::Value) -> String {
    let rendered = format_codex_window(window).expect("window was checked above");
    match window["duration_minutes"].as_u64() {
        Some(300) => format!("5h {rendered}"),
        Some(10_080) => format!("weekly {rendered}"),
        _ => rendered,
    }
}

fn format_codex_window(window: &serde_json::Value) -> Option<String> {
    let object = window.as_object()?;
    let used = object
        .get("used_percent")
        .and_then(serde_json::Value::as_f64)
        .map(|used| {
            if used.fract() == 0.0 {
                format!("{used:.0}%")
            } else {
                format!("{used:.1}%")
            }
        })
        .unwrap_or_else(|| "-%".into());
    let duration = object
        .get("duration_minutes")
        .and_then(serde_json::Value::as_u64)
        .map(format_codex_duration)
        .unwrap_or_else(|| "window ?".into());
    let reset = object
        .get("resets_at")
        .and_then(serde_json::Value::as_i64)
        .and_then(format_codex_reset)
        .map(|reset| format!(", resets in {reset}"))
        .unwrap_or_default();
    Some(format!("{used}/{duration}{reset}"))
}

fn format_codex_duration(minutes: u64) -> String {
    if minutes > 0 && minutes % (60 * 24) == 0 {
        format!("{}d", minutes / (60 * 24))
    } else if minutes > 0 && minutes % 60 == 0 {
        format!("{}h", minutes / 60)
    } else {
        format!("{minutes}m")
    }
}

fn format_codex_reset(timestamp: i64) -> Option<String> {
    let timestamp = u64::try_from(timestamp).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    format_codex_reset_from(timestamp, now)
}

fn format_codex_reset_from(timestamp: u64, now: u64) -> Option<String> {
    let remaining = timestamp
        .checked_sub(now)
        .filter(|remaining| *remaining > 0)?;
    if remaining < 60 * 60 {
        Some(format!("{}m", remaining / 60))
    } else if remaining < 60 * 60 * 24 {
        Some(format!("{}h", remaining / (60 * 60)))
    } else {
        Some(format!("{}d", remaining / (60 * 60 * 24)))
    }
}

pub(super) fn format_codex_launch_summary(value: &serde_json::Value) -> String {
    let (home, home_sanitized) = sanitized_display(value_str(value, "home").unwrap_or("<unknown>"));
    let (codex_bin, codex_bin_sanitized) =
        sanitized_display(value_str(value, "codex_bin").unwrap_or("codex"));
    let mut display_sanitized = home_sanitized || codex_bin_sanitized;
    let mut command = vec![crate::shell::quote(&codex_bin)];
    command.extend(
        value["args"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(|argument| {
                let (argument, sanitized) = sanitized_display(argument);
                display_sanitized |= sanitized;
                crate::shell::quote(&argument)
            }),
    );
    let mut lines = vec![
        "Codex launch: dry run".into(),
        format!("  CODEX_HOME: {home}"),
        format!("  Command (POSIX shell): {}", command.join(" ")),
    ];
    if value_bool(value, "representation_lossy").unwrap_or(false) {
        lines.push("  Warning: command contains non-UTF-8 values; display is lossy".into());
    }
    if display_sanitized {
        lines.push(
            "  Warning: terminal controls were replaced; displayed command is not launch-equivalent"
                .into(),
        );
    }
    lines.join("\n")
}

fn sanitized_display(value: &str) -> (String, bool) {
    let sanitized = sanitize_text(value);
    let changed = sanitized != value;
    (sanitized, changed)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn codex_homes_summary_uses_server_window_durations_and_all_buckets() {
        let summary = format_codex_homes_summary(&json!({
            "outcome": "complete",
            "usage_included": true,
            "homes": [{
                "name": "codex-work",
                "home": "/tmp/.codex-work",
                "current": true,
                "account": {
                    "type": "chatgpt",
                    "email": "person@example.com",
                    "plan_type": "pro"
                },
                "rate_limits": [{
                    "id": "codex",
                    "name": null,
                    "primary": {
                        "used_percent": 25,
                        "duration_minutes": 10080,
                        "resets_at": null
                    },
                    "secondary": null
                }, {
                    "id": "spark",
                    "name": "Spark",
                    "primary": {
                        "used_percent": 5.5,
                        "duration_minutes": 60,
                        "resets_at": null
                    },
                    "secondary": null
                }],
                "status": null
            }]
        }));

        assert!(summary.contains("Codex homes: 1 found (complete)"));
        assert!(summary.contains("* codex-work"));
        assert!(summary.contains("person@example.com"));
        assert!(summary.contains("codex: weekly 25%/7d"));
        assert!(summary.contains("Spark: 5.5%/1h"));
    }

    #[test]
    fn codex_homes_summary_renders_sanitized_discovery_warnings() {
        let summary = format_codex_homes_summary(&json!({
            "outcome": "partial",
            "usage_included": false,
            "homes": [{
                "name": "codex",
                "current": true,
                "account": { "email": "person@example.com", "plan_type": "pro" },
                "inspection_error": null,
                "usage_error": null
            }],
            "errors": [{
                "home": null,
                "kind": "discovery",
                "message": "permission denied\u{1b}[2J\nspoofed"
            }, {
                "home": "/tmp/.codex-broken",
                "kind": "inspection",
                "message": "already rendered on its home row"
            }]
        }));

        assert!(summary.contains("Discovery warnings:"));
        assert!(summary.contains("permission denied\u{fffd}[2J\u{fffd}spoofed"));
        assert!(!summary.contains('\u{1b}'));
        assert!(!summary.contains("already rendered on its home row"));
    }

    #[test]
    fn empty_codex_homes_summary_retains_discovery_warnings_and_json_hint() {
        let summary = format_codex_homes_summary(&json!({
            "outcome": "partial",
            "homes": [],
            "errors": [{
                "home": null,
                "kind": "discovery",
                "message": "directory scan denied"
            }]
        }));

        assert!(summary.contains("No directories found"));
        assert!(summary.contains("Discovery warnings:"));
        assert!(summary.contains("directory scan denied"));
        assert!(summary.contains("full report: rerun with --json"));
    }

    #[test]
    fn codex_homes_summary_treats_logged_out_usage_as_not_applicable() {
        let summary = format_codex_homes_summary(&json!({
            "outcome": "partial",
            "usage_included": true,
            "homes": [{
                "name": "codex-scratch",
                "current": false,
                "account": null,
                "rate_limits": [],
                "status": "not logged in",
                "inspection_error": null,
                "usage_error": "usage unavailable"
            }]
        }));

        assert!(summary.contains("not logged in"));
        assert!(!summary.contains("usage unavailable"));
        assert!(!summary.contains("usage error"));
    }

    #[test]
    fn codex_homes_summary_does_not_add_usage_noise_to_inspection_errors() {
        let summary = format_codex_homes_summary(&json!({
            "outcome": "partial",
            "usage_included": true,
            "homes": [{
                "name": "codex-broken",
                "current": false,
                "account": null,
                "rate_limits": [],
                "status": "unknown",
                "inspection_error": "app-server unavailable",
                "usage_error": null
            }]
        }));

        assert!(summary.contains("inspection error: app-server unavailable"));
        assert!(!summary.contains("usage unavailable"));
    }

    #[test]
    fn codex_homes_summary_renders_logged_in_usage_errors_once() {
        let summary = format_codex_homes_summary(&json!({
            "outcome": "partial",
            "usage_included": true,
            "homes": [{
                "name": "codex-work",
                "current": false,
                "account": { "email": "person@example.com", "plan_type": "pro" },
                "rate_limits": [],
                "status": null,
                "inspection_error": null,
                "usage_error": "rate limit request failed"
            }]
        }));

        assert!(summary.contains("usage error: rate limit request failed"));
        assert!(!summary.contains("usage unavailable"));
    }

    #[test]
    fn codex_launch_summary_preserves_argument_boundaries() {
        let summary = format_codex_launch_summary(&json!({
            "home": "/tmp/.codex-work",
            "codex_bin": "/opt/Codex CLI/codex",
            "args": ["--search", "prompt with spaces"]
        }));

        assert!(summary.contains("CODEX_HOME: /tmp/.codex-work"));
        assert!(summary.contains("'/opt/Codex CLI/codex'"));
        assert!(summary.contains("--search 'prompt with spaces'"));
    }

    #[test]
    fn codex_launch_summary_quotes_shell_expansions() {
        let summary = format_codex_launch_summary(&json!({
            "home": "/tmp/.codex-work",
            "codex_bin": "codex",
            "args": ["$HOME", "`touch /tmp/nope`", "line one\nline two"]
        }));

        assert!(summary.contains("'$HOME'"));
        assert!(summary.contains("'`touch /tmp/nope`'"));
        assert!(summary.contains("'line one\u{fffd}line two'"));
        assert!(summary.contains("displayed command is not launch-equivalent"));
    }

    #[test]
    fn codex_homes_summary_sanitizes_terminal_controls_and_bidi_text() {
        let summary = format_codex_homes_summary(&json!({
            "outcome": "complete\u{1b}[2J",
            "usage_included": false,
            "homes": [{
                "name": "codex\u{1b}[31m-work",
                "current": false,
                "account": {
                    "email": "person\u{202e}@example.com",
                    "plan_type": "pro\nspoofed"
                },
                "status": "authenticated\u{2069}",
                "inspection_error": null,
                "usage_error": null
            }]
        }));

        assert!(!summary.contains('\u{1b}'));
        assert!(!summary.contains('\u{202e}'));
        assert!(!summary.contains('\u{2069}'));
        assert!(!summary.contains("pro\nspoofed"));
        assert!(summary.contains("codex\u{fffd}[31m-work"));
        assert!(summary.contains("person\u{fffd}@example.com"));
    }

    #[test]
    fn codex_launch_summary_sanitizes_every_displayed_command_field() {
        let summary = format_codex_launch_summary(&json!({
            "home": "/tmp/.codex\u{1b}[2J",
            "codex_bin": "codex\u{202e}",
            "args": ["prompt\nspoofed", "safe"]
        }));

        assert!(!summary.contains('\u{1b}'));
        assert!(!summary.contains('\u{202e}'));
        assert!(!summary.contains("prompt\nspoofed"));
        assert!(summary.contains("/tmp/.codex\u{fffd}[2J"));
        assert!(summary.contains("prompt\u{fffd}spoofed"));
        assert!(summary.contains("displayed command is not launch-equivalent"));
    }

    #[test]
    fn codex_launch_summary_warns_when_the_report_is_lossy() {
        let summary = format_codex_launch_summary(&json!({
            "home": "/tmp/.codex-work",
            "codex_bin": "codex",
            "args": [],
            "representation_lossy": true
        }));

        assert!(summary.contains("Warning: command contains non-UTF-8 values"));
    }

    #[test]
    fn codex_homes_summary_labels_two_windows_as_5h_and_weekly() {
        let summary = format_codex_homes_summary(&json!({
            "usage_included": true,
            "homes": [{
                "name": "codex",
                "account": { "type": "chatgpt" },
                "rate_limits": [{
                    "id": "codex",
                    "primary": { "used_percent": 10, "duration_minutes": 300 },
                    "secondary": { "used_percent": 20, "duration_minutes": 10080 }
                }]
            }]
        }));

        assert!(summary.contains("codex: 5h 10%/5h, weekly 20%/7d"));
    }

    #[test]
    fn codex_homes_summary_labels_duplicate_window_durations_consistently() {
        let summary = format_codex_homes_summary(&json!({
            "usage_included": true,
            "homes": [{
                "name": "codex",
                "account": { "type": "chatgpt" },
                "rate_limits": [{
                    "id": "codex",
                    "primary": { "used_percent": 10, "duration_minutes": 300 },
                    "secondary": { "used_percent": 20, "duration_minutes": 300 }
                }]
            }]
        }));

        assert!(summary.contains("codex: 5h 10%/5h, 5h 20%/5h"));
    }

    #[test]
    fn codex_homes_summary_treats_the_only_codex_window_as_weekly() {
        let summary = format_codex_homes_summary(&json!({
            "usage_included": true,
            "homes": [{
                "name": "codex",
                "account": { "type": "chatgpt" },
                "rate_limits": [{
                    "id": "codex",
                    "primary": { "used_percent": 10, "duration_minutes": 300 },
                    "secondary": null
                }]
            }]
        }));

        assert!(summary.contains("codex: weekly 10%/5h"));
    }

    #[test]
    fn codex_reset_uses_epoch_seconds_and_omits_elapsed_resets() {
        assert_eq!(
            format_codex_reset_from(10_000 + 90 * 60, 10_000),
            Some("1h".into())
        );
        assert_eq!(
            format_codex_reset_from(10_000 + 2 * 24 * 60 * 60, 10_000),
            Some("2d".into())
        );
        assert_eq!(format_codex_reset_from(10_000, 10_000), None);
        assert_eq!(format_codex_reset_from(9_999, 10_000), None);
    }
}
