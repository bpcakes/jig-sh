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

/// Precedes recovery that passes `--home` or moves vault directories. Agents
/// act on error text, and the generated Vault rules make both operator-only.
const VAULT_STORAGE_OPERATOR_STEP: &str = "Operator step (agents must stop and ask the operator instead of passing --home or moving, renaming, or removing vault directories themselves):";

/// Physical vault home selected for a repo scope.
pub(super) struct ScopedVaultHome {
    pub(super) home: PathBuf,
    /// Repository root inside the main checkout whose namespace a verified
    /// linked Git worktree shares; `None` for every other checkout.
    pub(super) main_checkout_root: Option<PathBuf>,
    /// Value-free operator guidance when a linked worktree keeps the vault an
    /// earlier Jig version created in its own namespace; `None` otherwise.
    pub(super) worktree_local_guidance: Option<String>,
}

pub(super) fn scoped_vault_home(scope: &VaultRepoScope) -> Result<ScopedVaultHome> {
    if !crate::context::is_valid_vault_scope_id(&scope.scope_id) {
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
    let linkage = worktree::main_checkout_root(&repo_root);

    // Existing data wins: a checkout whose own namespace already holds a vault
    // keeps it, as earlier Jig versions resolved it, so an upgrade never
    // strands persisted secrets. Only worktrees without one share. Guidance
    // is advisory and must never make the kept vault unreachable.
    if vault_file_exists(&checkout_home)? {
        let worktree_local_guidance = match linkage {
            Ok(Some(main_root)) if main_root != repo_root => {
                let shared_home = scopes_home.join(repo_scope_dir(&main_root, &scope.scope_id));
                Some(worktree_local_guidance(
                    &main_root,
                    &checkout_home,
                    &shared_home,
                ))
            }
            Err(error) => error
                .downcast_ref::<worktree::UnverifiedWorktree>()
                .map(|unverified| unverified_worktree_local_guidance(&checkout_home, unverified)),
            // Like earlier versions, a checkout whose Git metadata cannot be
            // inspected simply keeps its own vault.
            Ok(_) => None,
        };
        return Ok(ScopedVaultHome {
            home: checkout_home,
            main_checkout_root: None,
            worktree_local_guidance,
        });
    }

    let Some(main_checkout_root) = linkage?.filter(|main_root| *main_root != repo_root) else {
        reject_legacy_repo_scope_cutover(scope, &checkout_home, &legacy_home)?;
        return Ok(ScopedVaultHome {
            home: checkout_home,
            main_checkout_root: None,
            worktree_local_guidance: None,
        });
    };

    // The digest recipe is unchanged, so a verified linked worktree selects
    // exactly the namespace its main checkout already uses.
    let shared_home = scopes_home.join(repo_scope_dir(&main_checkout_root, &scope.scope_id));
    reject_legacy_repo_scope_cutover(scope, &shared_home, &legacy_home)?;
    Ok(ScopedVaultHome {
        home: shared_home,
        main_checkout_root: Some(main_checkout_root),
        worktree_local_guidance: None,
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
        "legacy repo-scoped vault data exists at {}, but this Jig version now stores repo-scoped vaults in the trusted repo-local vault namespace at {} for '{}'. Refusing to treat the new namespace as empty. {VAULT_STORAGE_OPERATOR_STEP} Move the legacy vault directory after confirming this checkout should own those secrets, or pass --home {} to inspect it explicitly",
        legacy_home.display(),
        trusted_home.display(),
        scope.repo_name,
        legacy_home.display()
    );
}

/// Explains how to move a verified linked worktree that keeps the vault an
/// earlier Jig version created in its own namespace onto the shared vault.
/// Nothing is created or moved here; migration stays an operator decision.
fn worktree_local_guidance(
    main_checkout_root: &Path,
    checkout_home: &Path,
    shared_home: &Path,
) -> String {
    // A relative JIG_VAULT_HOME resolves from this process' working
    // directory, so print absolute paths that stay valid from any directory.
    let checkout = absolute_display(checkout_home);
    let shared = absolute_display(shared_home);
    let migration = match (vault_file_exists(shared_home), path_exists(shared_home)) {
        (Ok(true), _) => format!(
            "Both vaults hold data: copy the fields this worktree still needs into the shared vault from the main checkout, then move {checkout} aside."
        ),
        (Ok(false), Ok(true)) => format!(
            "The shared vault home {shared} exists but has no vault.json. After confirming it holds no vault data, remove that directory, then rename {checkout} to exactly {shared}."
        ),
        (Ok(false), Ok(false)) => format!(
            "After confirming this repository should own those secrets, rename {checkout} to exactly {shared}."
        ),
        (Err(error), _) | (_, Err(error)) => format!(
            "Jig could not inspect the shared vault home {shared} ({error:#}). Confirm whether it holds vault data before renaming {checkout} to exactly {shared} or copying fields into it."
        ),
    };
    format!(
        "This linked Git worktree keeps its own repo-scoped vault at {checkout}, created by an earlier Jig version, instead of the vault shared with the main checkout {} at {shared}. It shares the main checkout's vault once {checkout} no longer holds vault.json. {VAULT_STORAGE_OPERATOR_STEP} {migration}",
        main_checkout_root.display()
    )
}

/// Explains why a checkout whose worktree link fails verification keeps the
/// vault already in its own namespace, as earlier Jig versions resolved it.
/// Migration steps follow once the link verifies.
fn unverified_worktree_local_guidance(
    checkout_home: &Path,
    unverified: &worktree::UnverifiedWorktree,
) -> String {
    format!(
        "Jig could not verify this checkout as a linked Git worktree ({}), so it keeps the repo-scoped vault already at {}. {} Jig then reports how to share the main checkout's vault.",
        unverified.reason,
        absolute_display(checkout_home),
        worktree::WORKTREE_REPAIR_STEP
    )
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
