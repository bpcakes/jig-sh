//! Release ownership of legacy MCP configuration without deleting user servers.

use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::progress::CliProgress;

pub(super) fn stage_retirement(
    seed: Option<&Path>,
    destination: &Path,
    retirement_paths: &mut BTreeSet<PathBuf>,
    progress: CliProgress,
) -> Result<()> {
    let relative = Path::new(".mcp.json");
    if !retirement_paths.contains(relative) {
        return Ok(());
    }
    let Some(seed) = seed else {
        return Ok(());
    };
    let path = seed.join(relative);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to stat {}", path.display()));
        }
    };
    if !metadata.is_file() {
        retirement_paths.remove(relative);
        return Ok(());
    }
    let contents = jig_repository::path::read_repository_regular_file_bytes(seed, relative)?;
    let mut value = match jig_context::strict_json::from_slice(&contents) {
        Ok(value) => value,
        Err(_) => {
            retirement_paths.remove(relative);
            return Ok(());
        }
    };
    let generated = json!({"command": "./scripts/jig", "args": ["mcp"]});
    let Some(servers) = value.get_mut("mcpServers").and_then(Value::as_object_mut) else {
        retirement_paths.remove(relative);
        return Ok(());
    };
    if servers.get("jig") != Some(&generated) {
        retirement_paths.remove(relative);
        progress.step("retire MCP registration", "preserve customized .mcp.json");
        return Ok(());
    }
    servers.remove("jig");
    if servers.is_empty() && value.as_object().is_some_and(|object| object.len() == 1) {
        return Ok(());
    }
    progress.step(
        "retire MCP registration",
        "preserve other .mcp.json settings",
    );
    let remaining = serde_json::to_string_pretty(&value)? + "\n";
    let staged_path = destination.join(relative);
    fs::write(&staged_path, remaining).context("Failed to stage retained .mcp.json settings")?;
    fs::set_permissions(&staged_path, metadata.permissions())
        .context("Failed to preserve retained .mcp.json permissions")
}
