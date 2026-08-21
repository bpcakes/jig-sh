use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::ANSWERS_FILE;
use crate::context::RepoContext;
use crate::tool_defs;

const GENERATED_FRONTEND_COMMAND_DEFAULTS: &[(&str, &str)] = &[
    ("typescript_lint_command", "scripts/check-webapps.sh lint"),
    (
        "typescript_typecheck_command",
        "scripts/check-webapps.sh typecheck",
    ),
    ("typescript_build_command", "scripts/check-webapps.sh build"),
    (
        "typescript_coverage_command",
        "scripts/check-webapps.sh coverage",
    ),
];

pub(super) fn reconcile_runtime_config(
    seed_repo_path: Option<&Path>,
    destination: &Path,
    preferred_rendered_commands: &BTreeSet<String>,
) -> Result<()> {
    let Some(seed_repo_path) = seed_repo_path else {
        return Ok(());
    };
    let existing_path = seed_repo_path.join(ANSWERS_FILE);
    let rendered_path = destination.join(ANSWERS_FILE);
    let existing_text = fs::read_to_string(&existing_path)
        .with_context(|| format!("Failed to read {}", existing_path.display()))?;
    let rendered_text = fs::read_to_string(&rendered_path)
        .with_context(|| format!("Failed to read {}", rendered_path.display()))?;
    let existing = toml::from_str::<toml::Value>(&existing_text)
        .with_context(|| format!("Failed to parse {}", existing_path.display()))?;
    let mut rendered = toml::from_str::<toml::Value>(&rendered_text)
        .with_context(|| format!("Failed to parse {}", rendered_path.display()))?;
    let staged_context =
        RepoContext::load_from_root(destination.to_path_buf()).with_context(|| {
            format!(
                "Failed to load rendered runtime contract from {}",
                destination.display()
            )
        })?;
    let original_rendered = rendered.clone();
    let existing_table = existing
        .as_table()
        .ok_or_else(|| anyhow::anyhow!("{} is not a TOML table", existing_path.display()))?;
    let rendered_table = rendered
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("{} is not a TOML table", rendered_path.display()))?;

    reconcile_commands(
        existing_table,
        rendered_table,
        &staged_context,
        &existing_path,
        preferred_rendered_commands,
    )?;
    reconcile_work(existing_table, rendered_table, &staged_context)?;
    if let Some(existing_loop) = existing_table.get("loop") {
        rendered_table.insert("loop".into(), existing_loop.clone());
    }
    if rendered == original_rendered {
        return Ok(());
    }

    let serialized = toml::to_string_pretty(&rendered)
        .with_context(|| format!("Failed to serialize {}", rendered_path.display()))?;
    fs::write(&rendered_path, serialized)
        .with_context(|| format!("Failed to write {}", rendered_path.display()))
}

fn reconcile_commands(
    existing: &toml::Table,
    rendered: &mut toml::Table,
    staged_context: &RepoContext,
    existing_path: &Path,
    preferred_rendered_commands: &BTreeSet<String>,
) -> Result<()> {
    let Some(existing_commands) = existing.get("commands") else {
        return Ok(());
    };
    let existing_commands = existing_commands.as_table().ok_or_else(|| {
        anyhow::anyhow!(
            "Failed to reconcile {}: [commands] is not a TOML table",
            existing_path.display()
        )
    })?;
    let rendered_commands = rendered
        .entry("commands")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("Rendered [commands] is not a TOML table"))?;
    let retired_repository_commands = existing_repository_command_keys(existing);

    for (key, value) in existing_commands {
        if preferred_rendered_commands.contains(key) && rendered_commands.contains_key(key) {
            continue;
        }
        if staged_context.contract_version() >= 6 {
            if let Some(replacement) = v6_command_replacement(key) {
                if preferred_rendered_commands.contains(replacement)
                    && rendered_commands.contains_key(replacement)
                {
                    continue;
                }
                let replacement_is_required = staged_context
                    .required_commands()
                    .iter()
                    .any(|required| required == replacement);
                if replacement_is_required
                    && value
                        .as_str()
                        .is_some_and(|command| !command.trim().is_empty())
                {
                    rendered_commands.insert(replacement.into(), value.clone());
                } else if !replacement_is_required
                    && value
                        .as_str()
                        .is_some_and(|command| !command.trim().is_empty())
                    && !is_generated_frontend_default(key, value)
                {
                    rendered_commands.insert(key.clone(), value.clone());
                }
                continue;
            }
            if retired_repository_commands.contains(key)
                && !staged_context
                    .required_commands()
                    .iter()
                    .any(|required| required == key)
            {
                continue;
            }
        }
        if is_generated_frontend_default(key, value)
            && !staged_context
                .required_commands()
                .iter()
                .any(|required| required == key)
        {
            continue;
        }
        let would_empty_required_command = staged_context
            .required_commands()
            .iter()
            .any(|required| required == key)
            && value
                .as_str()
                .is_some_and(|command| command.trim().is_empty())
            && staged_context.command_for_key(key).is_ok();
        if would_empty_required_command {
            continue;
        }
        rendered_commands.insert(key.clone(), value.clone());
    }
    Ok(())
}

fn existing_repository_command_keys(existing: &toml::Table) -> BTreeSet<String> {
    existing
        .get("repository")
        .and_then(toml::Value::as_table)
        .and_then(|repository| repository.get("actions"))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table)
        .filter_map(|action| action.get("runner"))
        .filter_map(toml::Value::as_table)
        .filter(|runner| runner.get("kind").and_then(toml::Value::as_str) == Some("command"))
        .filter_map(|runner| runner.get("command"))
        .filter_map(toml::Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn v6_command_replacement(legacy: &str) -> Option<&'static str> {
    match legacy {
        "rust_fmt_check_command" | "go_fmt_check_command" => Some("api_fmt_command"),
        "rust_clippy_command" => Some("api_clippy_command"),
        "go_lint_command" => Some("api_lint_command"),
        "rust_test_command" | "go_test_command" => Some("api_test_command"),
        "rust_test_locked_command" | "go_test_locked_command" => Some("api_test_locked_command"),
        "sqlx_check_command" => Some("api_sqlx_command"),
        "schema_dump_command" => Some("api_schema_dump_command"),
        "sqlc_check_command" => Some("api_sqlc_command"),
        "typescript_lint_command" => Some("repo_compat_typescript_lint_command"),
        "typescript_typecheck_command" => Some("repo_compat_typescript_typecheck_command"),
        "typescript_build_command" => Some("repo_compat_typescript_build_command"),
        "typescript_coverage_command" => Some("repo_compat_typescript_coverage_command"),
        _ => None,
    }
}

fn is_generated_frontend_default(key: &str, value: &toml::Value) -> bool {
    GENERATED_FRONTEND_COMMAND_DEFAULTS
        .iter()
        .any(|(generated_key, generated_value)| {
            key == *generated_key && value.as_str() == Some(generated_value)
        })
}

fn reconcile_work(
    existing: &toml::Table,
    rendered: &mut toml::Table,
    staged_context: &RepoContext,
) -> Result<()> {
    let Some(existing_work) = existing.get("work") else {
        return Ok(());
    };
    let existing_work = existing_work
        .as_table()
        .ok_or_else(|| anyhow::anyhow!("Existing [work] is not a TOML table"))?;
    let rendered_work = rendered
        .entry("work")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("Rendered [work] is not a TOML table"))?;
    reconcile_work_checks(existing_work, rendered_work, staged_context);
    reconcile_work_gates(existing_work, rendered_work, staged_context)?;
    reconcile_work_refinements(existing_work, rendered_work);
    Ok(())
}

fn reconcile_work_checks(
    existing: &toml::Table,
    rendered: &mut toml::Table,
    staged_context: &RepoContext,
) {
    let Some(existing_checks) = existing.get("checks").and_then(toml::Value::as_array) else {
        return;
    };
    let mut seen = BTreeSet::new();
    let checks = existing_checks
        .iter()
        .filter_map(toml::Value::as_str)
        .filter(|tool| {
            staged_context
                .tool_spec(tool)
                .is_some_and(tool_defs::is_no_arg_execution_tool)
                && seen.insert((*tool).to_string())
        })
        .map(|tool| toml::Value::String(tool.to_string()))
        .collect();
    rendered.insert("checks".into(), toml::Value::Array(checks));
}

fn reconcile_work_gates(
    existing: &toml::Table,
    rendered: &mut toml::Table,
    staged_context: &RepoContext,
) -> Result<()> {
    let existing_gates = existing
        .get("gates")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let rendered_gates = rendered
        .get("gates")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut reconciled = Vec::new();
    let mut seen_ids = BTreeSet::new();

    for mut generated in rendered_gates {
        let generated_table = generated
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("Rendered work gate is not a TOML table"))?;
        let generated_id = work_gate_string(generated_table, "id", "rendered")?.to_string();
        let generated_kind = work_gate_string(generated_table, "kind", "rendered")?;
        let generated_tool = generated_table.get("tool").and_then(toml::Value::as_str);

        if generated_kind == "check" {
            if let Some(generated_tool) = generated_tool {
                let required = existing_gates.iter().find_map(|gate| {
                    let gate = gate.as_table()?;
                    (gate.get("id").and_then(toml::Value::as_str) == Some(&generated_id)
                        && gate.get("kind").and_then(toml::Value::as_str) == Some("check")
                        && gate.get("tool").and_then(toml::Value::as_str) == Some(generated_tool))
                    .then(|| gate.get("required").and_then(toml::Value::as_bool))
                    .flatten()
                });
                if let Some(required) = required {
                    generated_table.insert("required".into(), toml::Value::Boolean(required));
                }
            }
        }

        seen_ids.insert(generated_id);
        reconciled.push(generated);
    }

    for existing_gate in existing_gates {
        let Some(table) = existing_gate.as_table() else {
            continue;
        };
        let Some(id) = table.get("id").and_then(toml::Value::as_str) else {
            continue;
        };
        if seen_ids.contains(id) || !schema_valid_work_entry("gates", &existing_gate) {
            continue;
        }
        let keep = match table.get("kind").and_then(toml::Value::as_str) {
            Some("check") => table
                .get("tool")
                .and_then(toml::Value::as_str)
                .and_then(|tool| staged_context.tool_spec(tool))
                .is_some_and(tool_defs::is_no_arg_execution_tool),
            Some("codex_review") => true,
            _ => false,
        };
        if keep {
            seen_ids.insert(id.to_string());
            reconciled.push(existing_gate);
        }
    }

    rendered.insert("gates".into(), toml::Value::Array(reconciled));
    Ok(())
}

fn work_gate_string<'a>(table: &'a toml::Table, key: &str, label: &str) -> Result<&'a str> {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("{label} work gate is missing string {key}"))
}

fn reconcile_work_refinements(existing: &toml::Table, rendered: &mut toml::Table) {
    let Some(existing_refinements) = existing.get("refinements").and_then(toml::Value::as_array)
    else {
        return;
    };
    let refinements = existing_refinements
        .iter()
        .find(|entry| schema_valid_work_entry("refinements", entry))
        .cloned()
        .into_iter()
        .collect();
    rendered.insert("refinements".into(), toml::Value::Array(refinements));
}

fn schema_valid_work_entry(field: &str, entry: &toml::Value) -> bool {
    let mut work = toml::Table::new();
    work.insert(field.into(), toml::Value::Array(vec![entry.clone()]));
    toml::Value::Table(work)
        .try_into::<crate::context::WorkConfig>()
        .is_ok_and(|config| config.validate().is_ok())
}
