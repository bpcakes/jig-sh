use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

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
    let prior_generated_per_app_commands = prior_generated_per_app_command_defaults(existing);

    for (key, value) in existing_commands {
        let is_retired_fixed_default =
            GENERATED_FRONTEND_COMMAND_DEFAULTS
                .iter()
                .any(|(generated_key, generated_value)| {
                    key == generated_key && value.as_str() == Some(generated_value)
                });
        let is_retired_per_app_default = prior_generated_per_app_commands
            .get(key)
            .is_some_and(|generated_value| value.as_str() == Some(generated_value.as_str()));
        if (is_retired_fixed_default || is_retired_per_app_default)
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
        let generated_table = generated
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("Rendered work gate is not a TOML table"))?;
        let generated_id = work_gate_string(generated_table, "id", "rendered")?.to_string();
        let generated_kind = work_gate_string(generated_table, "kind", "rendered")?;
        let generated_tool = generated_table.get("tool").and_then(toml::Value::as_str);

        if generated_kind == "check"
            && let Some(generated_tool) = generated_tool
        {
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
                if existing_kind != "check" || existing_tool != generated_tool {
                    bail!(
                        "Cannot reconcile generated work gate '{generated_id}' ({generated_tool}): the existing gate with that id is kind '{existing_kind}' and tool '{existing_tool}'. Rename the project-owned gate or restore the generated identity before readoption."
                    );
                }
            }
            let exact_match = existing_gates
                .iter()
                .filter_map(toml::Value::as_table)
                .find(|gate| generated_gate_is_exact(&generated_id, generated_tool, gate));
            let existing_match = exact_match.or_else(|| {
                existing_gates
                    .iter()
                    .filter_map(toml::Value::as_table)
                    .find(|gate| {
                        generated_gate_is_legacy_alias(&generated_id, generated_tool, gate)
                    })
            });
            if let Some(existing) = existing_match {
                if let Some(existing_id) = existing.get("id").and_then(toml::Value::as_str) {
                    consumed_existing_ids.insert(existing_id.to_string());
                }
                // Generated applicability scopes are migration-owned contract
                // policy. Projects may retain whether a generated gate is
                // required and whether stable evidence can be reused; custom
                // scopes belong on a distinct project-owned gate id.
                for field in ["required", "reuse"] {
                    if let Some(value) = existing.get(field) {
                        generated_table.insert(field.into(), value.clone());
                    }
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
        if consumed_existing_ids.contains(id)
            || seen_ids.contains(id)
            || is_retired_generated_check_gate(table)
            || !schema_valid_work_entry("gates", &existing_gate)
        {
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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::reconcile_runtime_config;
    use crate::test_env::TestRepoBuilder;

    #[test]
    fn removed_frontend_app_retires_only_unchanged_generated_commands() {
        let temp = tempdir().unwrap();
        let seed = temp.path().join("seed");
        let destination = temp.path().join("destination");
        fs::create_dir_all(&seed).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(
            seed.join(".jig.toml"),
            r#"
[[frontend_apps]]
name = "ExampleApp"
dir = "web"

[commands]
typescript_exampleapp_lint_command = "scripts/check-webapps.sh app-check web lint"
typescript_exampleapp_typecheck_command = "scripts/check-webapps.sh app-check web project-typecheck"
project_release_command = "just release"
"#,
        )
        .unwrap();
        TestRepoBuilder::new(&destination).write();

        reconcile_runtime_config(Some(&seed), &destination).unwrap();

        let reconciled = fs::read_to_string(destination.join(".jig.toml")).unwrap();
        assert!(!reconciled.contains("typescript_exampleapp_lint_command"));
        assert!(reconciled.contains("typescript_exampleapp_typecheck_command"));
        assert!(reconciled.contains("project_release_command"));
    }
}
