use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct VaultAnswers {
    scope: String,
    scope_id: String,
    allow_global: bool,
}

pub(super) fn default_answers() -> VaultAnswers {
    VaultAnswers {
        scope: "repo".into(),
        scope_id: ulid::Ulid::new().to_string(),
        allow_global: false,
    }
}

pub(super) fn validate_answers(vault: &VaultAnswers) -> Result<()> {
    if vault.scope != "repo" {
        bail!(
            "Unsupported vault.scope '{}'. Expected 'repo'.",
            vault.scope
        );
    }
    if !jig_context::is_valid_vault_scope_id(&vault.scope_id) {
        bail!(
            "vault.scope_id must be 1 to 128 bytes and may only contain letters, digits, '_', or '-'"
        );
    }
    Ok(())
}

pub(super) fn apply_existing_default(
    target: &mut Option<VaultAnswers>,
    destination: &Path,
) -> Result<Option<String>> {
    if target.is_some() {
        return Ok(None);
    }
    let path = destination.join(super::super::ANSWERS_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to read {}", path.display()));
        }
    };
    let value = toml::from_str::<toml::Value>(&text)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    let Some(vault) = value.get("vault") else {
        return Ok(Some(
            "Existing .jig.toml had no [vault] block, so Jig added a new repo-scoped vault scope. Existing legacy global vault secrets are not migrated automatically."
                .into(),
        ));
    };
    let Some(table) = vault.as_table() else {
        bail!("[vault] in {} must be a TOML table", path.display());
    };
    for key in table.keys() {
        if !matches!(key.as_str(), "scope" | "scope_id" | "allow_global") {
            bail!("Unknown [vault].{key} in {}", path.display());
        }
    }
    let scope = match table.get("scope") {
        Some(value) => Some(value.as_str().ok_or_else(|| {
            anyhow::anyhow!("[vault].scope in {} must be a string", path.display())
        })?),
        None => None,
    };
    let allow_global = match table.get("allow_global") {
        Some(value) => value.as_bool().ok_or_else(|| {
            anyhow::anyhow!(
                "[vault].allow_global in {} must be a boolean",
                path.display()
            )
        })?,
        None => false,
    };
    if matches!(scope, None | Some("legacy")) {
        if table.get("scope_id").is_some() {
            bail!(
                "[vault].scope_id in {} requires [vault].scope = \"repo\"",
                path.display()
            );
        }
        return Ok(Some(
            "Existing .jig.toml had no repo [vault] scope, so Jig added a new repo-scoped vault scope. Existing legacy global vault secrets are not migrated automatically."
                .into(),
        ));
    }
    if scope != Some("repo") {
        bail!(
            "Unsupported [vault].scope '{}' in {}. Expected 'repo' or 'legacy'.",
            scope.unwrap_or_default(),
            path.display()
        );
    }
    let Some(scope_id) = table.get("scope_id").and_then(toml::Value::as_str) else {
        bail!(
            "[vault].scope_id is required in {} when [vault].scope = \"repo\"",
            path.display()
        );
    };
    if !jig_context::is_valid_vault_scope_id(scope_id) {
        bail!(
            "Invalid [vault].scope_id in {}: must be 1 to 128 bytes and may only contain letters, digits, '_', or '-'",
            path.display()
        );
    }
    *target = Some(VaultAnswers {
        scope: "repo".into(),
        scope_id: scope_id.into(),
        allow_global,
    });
    Ok(None)
}
