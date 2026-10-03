//! Repo-scope vault namespace derivation and its cutover guards.

use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::command::VaultRepoScope;

use super::{VAULT_FILE_NAME, VAULT_HOME_ENV};

mod worktree;

#[cfg(test)]
mod tests;

/// Physical vault home selected for a repo scope.
pub(super) struct ScopedVaultHome {
    pub(super) home: PathBuf,
    /// Repository root inside the main checkout whose namespace a verified
    /// linked Git worktree shares; `None` for every other checkout.
    pub(super) main_checkout_root: Option<PathBuf>,
}

pub(super) fn scoped_vault_home(scope: &VaultRepoScope) -> Result<ScopedVaultHome> {
    if !crate::command::is_valid_vault_scope_id(&scope.scope_id) {
        bail!("invalid repo vault scope id '{}'", scope.scope_id);
    }
    let scopes_home = vault_base_home()?.join("scopes");
    let repo_root = std::fs::canonicalize(&scope.repo_root).with_context(|| {
        format!(
            "failed to canonicalize repo root for vault scope: {}",
            scope.repo_root.display()
        )
    })?;
    let checkout_home = scopes_home.join(repo_scope_dir(&repo_root, &scope.scope_id));
    let legacy_home = scopes_home.join(&scope.scope_id);
    let main_checkout_root =
        worktree::main_checkout_root(&repo_root)?.filter(|main_root| *main_root != repo_root);
    let Some(main_checkout_root) = main_checkout_root else {
        reject_legacy_repo_scope_cutover(scope, &checkout_home, &legacy_home)?;
        return Ok(ScopedVaultHome {
            home: checkout_home,
            main_checkout_root: None,
        });
    };

    // The digest recipe is unchanged, so a verified linked worktree selects
    // exactly the namespace its main checkout already uses.
    let shared_home = scopes_home.join(repo_scope_dir(&main_checkout_root, &scope.scope_id));
    reject_orphan_worktree_scope_cutover(scope, &main_checkout_root, &checkout_home, &shared_home)?;
    reject_legacy_repo_scope_cutover(scope, &shared_home, &legacy_home)?;
    Ok(ScopedVaultHome {
        home: shared_home,
        main_checkout_root: Some(main_checkout_root),
    })
}

fn reject_legacy_repo_scope_cutover(
    scope: &VaultRepoScope,
    trusted_home: &Path,
    legacy_home: &Path,
) -> Result<()> {
    if vault_file_exists(trusted_home)? || !vault_file_exists(legacy_home)? {
        return Ok(());
    }

    bail!(
        "legacy repo-scoped vault data exists at {}, but this Jig version now stores repo-scoped vaults in the trusted repo-local vault namespace at {} for '{}'. Refusing to treat the new namespace as empty. Move the legacy vault directory after confirming this checkout should own those secrets, or pass --home {} to inspect it explicitly",
        legacy_home.display(),
        trusted_home.display(),
        scope.repo_name,
        legacy_home.display()
    );
}

/// Refuses to shadow a vault that an earlier Jig version created in a linked
/// worktree's own checkout namespace. Nothing is created or moved here.
fn reject_orphan_worktree_scope_cutover(
    scope: &VaultRepoScope,
    main_checkout_root: &Path,
    checkout_home: &Path,
    shared_home: &Path,
) -> Result<()> {
    if !vault_file_exists(checkout_home)? {
        return Ok(());
    }

    // A relative JIG_VAULT_HOME resolves from this process' working
    // directory, so print absolute paths that stay valid from any directory.
    let checkout = absolute_display(checkout_home);
    let shared = absolute_display(shared_home);
    let recovery = if vault_file_exists(shared_home)? {
        format!(
            "Both vaults hold data: inspect the worktree vault with --home {checkout}, copy the fields you still need into the shared vault from the main checkout, then move {checkout} aside"
        )
    } else if path_exists(shared_home)? {
        format!(
            "The shared vault home {shared} exists but has no vault.json. After confirming it holds no vault data, remove that directory first, then rename {checkout} to exactly {shared}; or pass --home {checkout} to inspect the worktree vault explicitly"
        )
    } else {
        format!(
            "After confirming this repository should own those secrets, rename {checkout} to exactly {shared}, or pass --home {checkout} to inspect the worktree vault explicitly"
        )
    };
    bail!(
        "this linked Git worktree already has its own repo-scoped vault at {checkout}, created before Jig shared repo-scoped vaults with the main checkout, but '{}' now resolves to the vault shared with the main checkout {} at {shared}. Refusing to switch away from existing vault data. {recovery}",
        scope.repo_name,
        main_checkout_root.display(),
    );
}

fn absolute_display(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

fn path_exists(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("failed to inspect {}", path.display())),
    }
}

fn vault_file_exists(home: &Path) -> Result<bool> {
    let vault_file = home.join(VAULT_FILE_NAME);
    match std::fs::symlink_metadata(&vault_file) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error)
            .with_context(|| format!("failed to inspect vault file {}", vault_file.display())),
    }
}

/// Physical scope directory name for a repository root and `scope_id`. The
/// root must be absolute and is hashed byte for byte; changing this recipe
/// orphans every existing repo-scoped vault.
fn repo_scope_dir(repo_root: &Path, scope_id: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"jig-vault-repo-scope-v2\0");
    #[cfg(unix)]
    digest.update(repo_root.as_os_str().as_bytes());
    #[cfg(not(unix))]
    digest.update(repo_root.to_string_lossy().as_bytes());
    digest.update(b"\0");
    digest.update(scope_id.as_bytes());
    format!("repo-{}", lower_hex(&digest.finalize()))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

pub(super) fn vault_base_home() -> Result<PathBuf> {
    match std::env::var_os(VAULT_HOME_ENV) {
        Some(value) if value.is_empty() => bail!("{VAULT_HOME_ENV} must not be empty"),
        Some(value) => Ok(PathBuf::from(value)),
        None => Ok(dirs::home_dir()
            .context("could not resolve home directory for Jig vault")?
            .join(".jig/vault")),
    }
}
