use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::super::scan::{RepoScan, push_scan_warning, read_limited_text, relative_path_string};
use super::tool::{DetectedTool, tools_from_map};

pub(super) fn infer_sqlx_command_tools(
    root: &Path,
    scan: &RepoScan,
    warnings: &mut Vec<String>,
) -> Vec<DetectedTool> {
    let mut tools = BTreeMap::<String, BTreeSet<String>>::new();
    for path in scan.files_with_extensions(&["sh", "yml", "yaml", "env", "toml"]) {
        let relative = relative_path_string(path.strip_prefix(root).unwrap_or(path));
        let text = match read_limited_text(path) {
            Ok(text) => text,
            Err(error) => {
                push_scan_warning(
                    warnings,
                    path,
                    &format!("could not read text for command inference: {error:#}"),
                );
                continue;
            }
        };
        if text.contains("SQLX_OFFLINE=true") || text.contains("SQLX_OFFLINE=1") {
            tools
                .entry("sqlx-offline".into())
                .or_default()
                .insert(relative.clone());
        }
        if text.contains("DATABASE_URL") {
            tools
                .entry("database-url-convention".into())
                .or_default()
                .insert(relative.clone());
        }
        if text.contains("cargo sqlx migrate add") || text.contains("sqlx migrate add") {
            tools
                .entry("sqlx-migration-wrapper".into())
                .or_default()
                .insert(relative);
        }
    }
    tools_from_map(tools)
}
