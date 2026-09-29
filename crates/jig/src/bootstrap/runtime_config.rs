use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::ANSWERS_FILE;
use super::clippy_policy::is_generated_rust_clippy_command;
use super::repository_model::{RUST_FILE_LOC_COMMAND_KEY, is_generated_rust_file_loc_command};
use crate::context::RepoContext;

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

mod retired_work;
pub(super) use retired_work::retired_work_notes;

/// Carries tracker ownership into the refreshed configuration. Refreshes
/// render contract 12 or later, which declare it as `[repository] tracker`.
pub(super) fn reconcile_tracker_ownership(
    seed_repo_path: Option<&Path>,
    destination: &Path,
) -> Result<()> {
    let Some(existing) = read_existing_config(seed_repo_path)? else {
        return Ok(());
    };
    retired_work::apply_retained_tracker(&existing, destination)
}

fn read_existing_config(seed_repo_path: Option<&Path>) -> Result<Option<toml::Table>> {
    let Some(seed_repo_path) = seed_repo_path else {
        return Ok(None);
    };
    let existing_path = seed_repo_path.join(ANSWERS_FILE);
    let existing_text = match fs::read_to_string(&existing_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to read {}", existing_path.display()));
        }
    };
    let existing = toml::from_str::<toml::Value>(&existing_text)
        .with_context(|| format!("Failed to parse {}", existing_path.display()))?;
    match existing {
        toml::Value::Table(table) => Ok(Some(table)),
        _ => bail!("{} is not a TOML table", existing_path.display()),
    }
}

fn rewrite_rendered_config(
    destination: &Path,
    edit: impl FnOnce(&mut toml::Table) -> Result<()>,
) -> Result<()> {
    let rendered_path = destination.join(ANSWERS_FILE);
    let rendered_text = fs::read_to_string(&rendered_path)
        .with_context(|| format!("Failed to read {}", rendered_path.display()))?;
    let mut rendered = toml::from_str::<toml::Value>(&rendered_text)
        .with_context(|| format!("Failed to parse {}", rendered_path.display()))?;
    let rendered = rendered
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("{} is not a TOML table", rendered_path.display()))?;
    edit(rendered)?;
    let serialized = toml::to_string_pretty(rendered)
        .with_context(|| format!("Failed to serialize {}", rendered_path.display()))?;
    fs::write(&rendered_path, serialized)
        .with_context(|| format!("Failed to write {}", rendered_path.display()))
}

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
        .filter(|runner| {
            matches!(
                runner.get("kind").and_then(toml::Value::as_str),
                Some("command" | "shell")
            )
        })
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

fn schema_valid_work_field(field: &str, value: toml::Value) -> bool {
    let mut work = toml::Table::new();
    work.insert(field.into(), value);
    toml::Value::Table(work)
        .try_into::<crate::context::WorkConfig>()
        .is_ok_and(|config| config.validate().is_ok())
}
