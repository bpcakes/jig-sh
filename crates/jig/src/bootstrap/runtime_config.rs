use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::ANSWERS_FILE;
use super::clippy_policy::is_generated_rust_clippy_command;
use super::repository_model::{RUST_FILE_LOC_COMMAND_KEY, is_generated_rust_file_loc_command};
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
    (
        "application_contract_check_command",
        "scripts/check-webapps.sh application-contracts",
    ),
    (
        "public_artifacts_check_command",
        "scripts/check-webapps.sh public-artifacts",
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
    let prior_generated_per_app_commands = prior_generated_per_app_command_defaults(existing);

    for (key, value) in existing_commands {
        if preferred_rendered_commands.contains(key) && rendered_commands.contains_key(key) {
            continue;
        }
        if key == RUST_FILE_LOC_COMMAND_KEY
            && value
                .as_str()
                .is_some_and(is_generated_rust_file_loc_command)
            && rendered_commands
                .get(key)
                .and_then(toml::Value::as_str)
                .is_some_and(is_generated_rust_file_loc_command)
        {
            continue;
        }
        if value.as_str().is_some_and(is_generated_rust_clippy_command)
            && rendered_commands
                .get(key)
                .and_then(toml::Value::as_str)
                .is_some_and(is_generated_rust_clippy_command)
        {
            continue;
        }
        let is_retired_per_app_default = prior_generated_per_app_commands
            .get(key)
            .is_some_and(|generated_value| value.as_str() == Some(generated_value.as_str()));
        if is_retired_per_app_default
            && !staged_context
                .required_commands()
                .iter()
                .any(|required| required == key)
        {
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
                if value.as_str().is_some_and(is_generated_rust_clippy_command)
                    && rendered_commands
                        .get(replacement)
                        .and_then(toml::Value::as_str)
                        .is_some_and(is_generated_rust_clippy_command)
                {
                    continue;
                }
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

fn prior_generated_per_app_command_defaults(existing: &toml::Table) -> BTreeMap<String, String> {
    let Some(frontend_apps) = existing
        .get("frontend_apps")
        .and_then(toml::Value::as_array)
    else {
        return BTreeMap::new();
    };
    let mut commands = BTreeMap::new();
    for app in frontend_apps {
        let Some(app) = app.as_table() else {
            continue;
        };
        let Some(name) = app.get("name").and_then(toml::Value::as_str) else {
            continue;
        };
        let Some(dir) = app.get("dir").and_then(toml::Value::as_str) else {
            continue;
        };
        if name.is_empty()
            || !name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
            || dir.is_empty()
        {
            continue;
        }
        let key = super::answers::frontend_gate_key(name);
        for operation in ["lint", "typecheck", "build", "coverage"] {
            commands.insert(
                format!("typescript_{key}_{operation}_command"),
                format!("scripts/check-webapps.sh app-check {dir} {operation}"),
            );
        }
    }
    commands
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
    let mut consumed_existing_ids = BTreeSet::new();

    for mut generated in rendered_gates {
        let generated_gate =
            crate::context::parse_work_gate(&generated).context("Rendered work gate is invalid")?;
        let generated_table = generated
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("Rendered work gate is not a TOML table"))?;
        let generated_id = generated_gate.id().to_string();

        if let crate::context::WorkGate::Check(check) = &generated_gate {
            if let Some(collision) = existing_gates
                .iter()
                .filter_map(toml::Value::as_table)
                .find(|gate| {
                    gate.get("id").and_then(toml::Value::as_str) == Some(generated_id.as_str())
                })
            {
                let existing_kind = collision
                    .get("kind")
                    .and_then(toml::Value::as_str)
                    .unwrap_or("<missing>");
                let existing_tool = collision
                    .get("tool")
                    .and_then(toml::Value::as_str)
                    .unwrap_or("<missing>");
                if existing_kind != "check" || existing_tool != check.tool {
                    bail!(
                        "Cannot reconcile generated work gate '{generated_id}' ({}): the existing gate with that id is kind '{existing_kind}' and tool '{existing_tool}'. Rename the project-owned gate or restore the generated identity before readoption.",
                        check.tool
                    );
                }
            }
            let exact_match = existing_gates
                .iter()
                .filter_map(toml::Value::as_table)
                .find(|gate| generated_gate_is_exact(&generated_id, &check.tool, gate));
            let existing_match = exact_match.or_else(|| {
                existing_gates
                    .iter()
                    .filter_map(toml::Value::as_table)
                    .find(|gate| generated_gate_is_legacy_alias(&generated_id, &check.tool, gate))
            });
            if let Some(existing) = existing_match {
                if let Some(existing_id) = existing.get("id").and_then(toml::Value::as_str) {
                    consumed_existing_ids.insert(existing_id.to_string());
                }
                for field in ["required", "reuse"] {
                    if let Some(value) = existing.get(field) {
                        generated_table.insert(field.into(), value.clone());
                    }
                }
            }
        } else if let Some(existing) = existing_gates.iter().find(|value| {
            crate::context::parse_work_gate(value).is_ok_and(|gate| {
                gate.id() == generated_id && gate.same_definition(&generated_gate)
            })
        }) && let Some(required) = existing
            .as_table()
            .and_then(|table| table.get("required"))
            .and_then(toml::Value::as_bool)
        {
            generated_table.insert("required".into(), toml::Value::Boolean(required));
        }

        seen_ids.insert(generated_id);
        reconciled.push(generated);
    }

    for existing_gate in existing_gates {
        let Ok(gate) = crate::context::parse_work_gate(&existing_gate) else {
            continue;
        };
        let table = existing_gate
            .as_table()
            .expect("parsed work gates are TOML tables");
        if consumed_existing_ids.contains(gate.id())
            || seen_ids.contains(gate.id())
            || is_retired_generated_check_gate(table)
            || !schema_valid_work_entry("gates", &existing_gate)
        {
            continue;
        }
        let keep = match &gate {
            crate::context::WorkGate::Check(check) => staged_context
                .tool_spec(&check.tool)
                .is_some_and(tool_defs::is_no_arg_execution_tool),
            crate::context::WorkGate::Evidence(_) | crate::context::WorkGate::CodexReview(_) => {
                true
            }
            crate::context::WorkGate::Unsupported(_) => false,
        };
        if keep {
            seen_ids.insert(gate.id().to_string());
            reconciled.push(existing_gate);
        }
    }

    rendered.insert("gates".into(), toml::Value::Array(reconciled));
    Ok(())
}

fn generated_gate_is_exact(
    generated_id: &str,
    generated_tool: &str,
    existing: &toml::Table,
) -> bool {
    existing.get("kind").and_then(toml::Value::as_str) == Some("check")
        && existing.get("id").and_then(toml::Value::as_str) == Some(generated_id)
        && existing.get("tool").and_then(toml::Value::as_str) == Some(generated_tool)
}

fn generated_gate_is_legacy_alias(
    generated_id: &str,
    generated_tool: &str,
    existing: &toml::Table,
) -> bool {
    if existing.get("kind").and_then(toml::Value::as_str) != Some("check") {
        return false;
    }
    let existing_id = existing.get("id").and_then(toml::Value::as_str);
    let existing_tool = existing.get("tool").and_then(toml::Value::as_str);
    matches!(
        (generated_id, generated_tool, existing_id, existing_tool),
        (
            "jig-contract",
            "jig.contract_check",
            Some("contract"),
            Some("jig.contract_check")
        ) | ("rust-tests", "jig.test", Some("tests"), Some("jig.test"))
    )
}

fn is_retired_generated_check_gate(table: &toml::Table) -> bool {
    let id = table.get("id").and_then(toml::Value::as_str);
    let tool = table.get("tool").and_then(toml::Value::as_str);
    matches!(
        (id, tool),
        (Some("contract"), Some("jig.contract_check"))
            | (Some("tests"), Some("jig.test"))
            | (
                Some("application-contracts"),
                Some("jig.application_contract_check")
            )
            | (Some("public-artifacts"), Some("jig.public_artifacts_check"))
            | (Some("typescript-lint"), Some("jig.typescript_lint"))
            | (
                Some("typescript-typecheck"),
                Some("jig.typescript_typecheck")
            )
            | (Some("typescript-build"), Some("jig.typescript_build"))
            | (Some("typescript-coverage"), Some("jig.typescript_coverage"))
            | (Some("schema-dump"), Some("jig.schema_dump"))
    )
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
