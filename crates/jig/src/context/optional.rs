use anyhow::Result;

use super::{JIG_REPO_ROOT_ENV, RepoContext, find_optional_repo_root, repo_root_from_env};

pub(crate) const REPO_CONTEXT_NOT_FOUND: &str = "Could not find repo root containing .jig.toml";

impl RepoContext {
    pub(crate) fn repo_root_override_is_set() -> bool {
        std::env::var_os(JIG_REPO_ROOT_ENV).is_some_and(|root| !root.is_empty())
    }

    /// Load an explicitly configured or discovered repository without
    /// tolerating invalid context.
    pub(crate) fn load_optional_strict() -> Result<Option<Self>> {
        let root = match repo_root_from_env()? {
            Some(root) => Some(root),
            None => find_optional_repo_root()?,
        };
        root.map(Self::load_from_root).transpose()
    }

    /// Resolve optional repository context without emitting fallback warnings.
    pub(crate) fn load_optional_quiet() -> Result<Option<Self>> {
        Self::load_optional_with_warnings(false)
    }

    #[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
    pub(crate) fn load_optional() -> Result<Option<Self>> {
        Self::load_optional_with_warnings(true)
    }

    fn load_optional_with_warnings(warnings: bool) -> Result<Option<Self>> {
        // Contextless commands keep working when a shell inherited a stale
        // JIG_REPO_ROOT. Required commands use load(), where the override stays
        // strict. Inventory uses the same fallback path without stderr noise.
        match repo_root_from_env() {
            Ok(Some(root)) => {
                let display_root = root.display().to_string();
                match Self::load_from_root(root) {
                    Ok(ctx) => return Ok(Some(ctx)),
                    Err(error) if warnings => eprintln!(
                        "jig ignored invalid {JIG_REPO_ROOT_ENV}={display_root} for contextless command lookup: {error:#}"
                    ),
                    Err(_) => {}
                }
            }
            Ok(None) => {}
            Err(error) if warnings => eprintln!(
                "jig ignored invalid {JIG_REPO_ROOT_ENV} for contextless command lookup: {error:#}"
            ),
            Err(_) => {}
        }
        let Some(root) = find_optional_repo_root()? else {
            return Ok(None);
        };
        Self::load_from_root(root).map(Some)
    }
}

#[cfg(test)]
mod tests;
