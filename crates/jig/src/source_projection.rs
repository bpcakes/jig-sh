use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::repository_path::normalize_repo_relative_path;

pub(crate) const MAX_SUBMODULE_DEPTH: usize = 32;
pub(crate) const IGNORED_DOTENV_PATHSPECS: &[&str] =
    &[".env", ".env.*", ":(glob)**/.env", ":(glob)**/.env.*"];

pub(crate) fn initialized_submodule_paths(
    repository_root: &Path,
    output: &[u8],
) -> Result<Vec<PathBuf>> {
    let output = std::str::from_utf8(output).context("Git returned non-UTF-8 submodule config")?;
    output
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let (_, path) = entry
                .split_once('\n')
                .ok_or_else(|| anyhow::anyhow!("git returned a malformed submodule path record"))?;
            normalize_repo_relative_path(Path::new(path), "submodule path")
        })
        .filter_map(|relative| match relative {
            Ok(relative) if repository_root.join(&relative).join(".git").exists() => {
                Some(Ok(relative))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}
