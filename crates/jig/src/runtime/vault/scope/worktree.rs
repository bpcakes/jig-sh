//! Read-only proof that a checkout is a linked Git worktree, used to share the
//! main checkout's repo-scoped vault namespace.
//!
//! The proof reads Git's on-disk metadata directly and ignores `GIT_*`
//! environment variables, Git config includes, and `git` output, so the
//! environment cannot redirect a vault namespace. Nothing here creates files.

use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cap_std::{ambient_authority, fs::Dir};

use crate::runtime::git_path::{
    MAX_GIT_POINTER_BYTES, parse_git_path_line, parse_gitdir_pointer, read_nofollow_regular_file,
};

use super::VAULT_STORAGE_OPERATOR_STEP;

const MAX_GIT_CONFIG_BYTES: u64 = 1024 * 1024;

/// Repairs the Git link of a moved worktree so the proof can succeed again.
pub(super) const WORKTREE_REPAIR_STEP: &str = "If this worktree was moved, run `git worktree repair` inside it, or `git worktree repair <path>` from the main checkout.";

/// Returns the repository root inside the main checkout that corresponds to
/// the canonical `repo_root` when `repo_root` lies in a verified linked Git
/// worktree, keeping a nested `.jig.toml`'s relative location.
///
/// `Ok(None)` keeps today's checkout scope: no `.git` pointer file, a pointer
/// that Git itself could not use, a submodule, a separate-git-dir or bare
/// repository, or an independent repository nested in the main checkout. A
/// pointer whose target claims to be a linked worktree but fails the proof is
/// an error, because falling back would silently orphan the worktree's vault.
pub(super) fn main_checkout_root(repo_root: &Path) -> Result<Option<PathBuf>> {
    let Some(top) = worktree_top(repo_root)? else {
        return Ok(None);
    };
    let Some(admin) = worktree_admin_dir(&top)? else {
        return Ok(None);
    };
    // Git marks a linked worktree's administrative directory with `commondir`;
    // submodule and separate-git-dir directories have none.
    let commondir = admin.join("commondir");
    match fs::symlink_metadata(&commondir) {
        Ok(_) => {}
        Err(error) if is_absent(&error) => return Ok(None),
        Err(error) => {
            return Err(unverified(
                &top,
                &format!("cannot inspect {commondir:?}: {error}"),
            ));
        }
    }
    let common =
        verify_linked_worktree(&top, &admin).map_err(|reason| unverified(&top, &reason))?;
    // Bare and separate-git-dir repositories have no main checkout to share.
    if common.file_name() != Some(OsStr::new(".git")) || common_config_relocates_worktree(&common) {
        return Ok(None);
    }
    let (Some(main_top), Ok(relative)) = (common.parent(), repo_root.strip_prefix(&top)) else {
        return Ok(None);
    };
    if nested_repository_below(main_top, relative)? {
        return Ok(None);
    }
    // Hash the literal corresponding path and never canonicalize it: a symlink
    // in the main checkout must not select another repository's namespace.
    // Push components because joining an empty path appends a separator,
    // which would change the digest.
    let mut mapped = main_top.to_path_buf();
    mapped.extend(relative.components());
    Ok(Some(mapped))
}

/// Finds the nearest `.git` entry at or above the canonical `repo_root` and
/// returns its directory only when the entry is a regular pointer file that,
/// like the directory holding it, belongs to the current user.
fn worktree_top(repo_root: &Path) -> Result<Option<PathBuf>> {
    for directory in repo_root.ancestors() {
        let dot_git = directory.join(".git");
        let metadata = match fs::symlink_metadata(&dot_git) {
            Ok(metadata) => metadata,
            Err(error) if is_absent(&error) => continue,
            // An unreadable ancestor cannot carry a verifiable claim.
            Err(error) if error.kind() == ErrorKind::PermissionDenied => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to inspect Git metadata entry {}", dot_git.display())
                });
            }
        };
        // A `.git` directory is a main checkout; a symlink or special file is
        // never trusted as a worktree pointer.
        if !metadata.file_type().is_file() || !owned_by_current_user(&metadata) {
            return Ok(None);
        }
        let directory_metadata = fs::metadata(directory)
            .with_context(|| format!("failed to inspect worktree {}", directory.display()))?;
        return Ok(owned_by_current_user(&directory_metadata).then(|| directory.to_path_buf()));
    }
    Ok(None)
}

/// Reads `top/.git` and returns its canonical target directory, or `None`
/// when the pointer is malformed or dangling, where Git could not use it.
fn worktree_admin_dir(top: &Path) -> Result<Option<PathBuf>> {
    let directory = Dir::open_ambient_dir(top, ambient_authority())
        .with_context(|| format!("failed to open worktree {}", top.display()))?;
    let Some(pointer) = read_nofollow_regular_file(&directory, ".git", MAX_GIT_POINTER_BYTES)?
    else {
        return Ok(None);
    };
    let Some(admin) = parse_gitdir_pointer(&pointer, top) else {
        return Ok(None);
    };
    match fs::canonicalize(&admin) {
        Ok(admin) => Ok(admin.is_dir().then_some(admin)),
        Err(error) if is_absent(&error) => Ok(None),
        Err(error) => Err(error).with_context(|| {
            format!(
                "failed to resolve Git worktree pointer {admin:?} in {}",
                top.display()
            )
        }),
    }
}

/// Proves the claim `top/.git -> admin` and returns the canonical common Git
/// directory. The admin directory must sit directly in `<common>/worktrees`,
/// its `commondir` must resolve to that common directory, and its `gitdir`
/// back-link must name this checkout's literal `.git`. Only a writer of the
/// common directory can create such an admin directory.
fn verify_linked_worktree(top: &Path, admin: &Path) -> std::result::Result<PathBuf, String> {
    let admin_dir = Dir::open_ambient_dir(admin, ambient_authority())
        .map_err(|error| format!("cannot open {admin:?}: {error}"))?;
    let commondir = read_path_line(&admin_dir, admin, "commondir")?;
    let common = fs::canonicalize(&commondir)
        .map_err(|error| format!("cannot resolve commondir {commondir:?}: {error}"))?;
    if !common.is_dir() {
        return Err(format!("commondir {common:?} is not a directory"));
    }
    let worktrees = common.join("worktrees");
    if admin.parent() != Some(worktrees.as_path()) {
        return Err(format!("{admin:?} is not directly inside {worktrees:?}"));
    }
    let back_link = read_path_line(&admin_dir, admin, "gitdir")?;
    let expected = top.join(".git");
    match fs::canonicalize(&back_link) {
        Ok(target) if target == expected => Ok(common),
        Ok(target) => Err(format!(
            "its gitdir back-link names {target:?}, not {expected:?}"
        )),
        Err(error) => Err(format!(
            "cannot resolve gitdir back-link {back_link:?}: {error}"
        )),
    }
}

fn read_path_line(
    directory: &Dir,
    path: &Path,
    name: &str,
) -> std::result::Result<PathBuf, String> {
    match read_nofollow_regular_file(directory, name, MAX_GIT_POINTER_BYTES) {
        Ok(Some(bytes)) => parse_git_path_line(&bytes, path)
            .ok_or_else(|| format!("{:?} is not a single path line", path.join(name))),
        Ok(None) => Err(format!(
            "{:?} is missing, not a regular file, or too large",
            path.join(name)
        )),
        Err(error) => Err(format!("{error:#}")),
    }
}

/// Best-effort scan for `core.bare` or `core.worktree` in the common
/// directory's `config` and `config.worktree`. Config includes are not
/// followed. A missed setting can only share a namespace keyed by a path
/// inside the same repository's directory, never another repository's.
fn common_config_relocates_worktree(common: &Path) -> bool {
    let Ok(directory) = Dir::open_ambient_dir(common, ambient_authority()) else {
        return true;
    };
    ["config", "config.worktree"].into_iter().any(|name| {
        match fs::symlink_metadata(common.join(name)) {
            Err(error) if is_absent(&error) => return false,
            Err(_) => return true,
            Ok(_) => {}
        }
        match read_nofollow_regular_file(&directory, name, MAX_GIT_CONFIG_BYTES) {
            Ok(Some(bytes)) => config_sets_bare_or_worktree(&bytes),
            // An unreadable or oversized config keeps checkout scope.
            Ok(None) | Err(_) => true,
        }
    })
}

fn config_sets_bare_or_worktree(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    let mut in_core = false;
    for line in text.lines() {
        let mut line = line.trim();
        if let Some(header) = line.strip_prefix('[') {
            let Some((section, remainder)) = header.split_once(']') else {
                in_core = false;
                continue;
            };
            in_core = section.trim().eq_ignore_ascii_case("core");
            // Git accepts a variable on the same line as its section header.
            line = remainder.trim();
        }
        if !in_core || line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        let (key, value) = match line.split_once('=') {
            Some((key, value)) => (key.trim(), Some(strip_config_comment(value))),
            None => (strip_config_comment(line), None),
        };
        if key.eq_ignore_ascii_case("worktree")
            || (key.eq_ignore_ascii_case("bare") && value.is_none_or(config_value_is_true))
        {
            return true;
        }
    }
    false
}

fn strip_config_comment(value: &str) -> &str {
    value.split(['#', ';']).next().unwrap_or_default().trim()
}

fn config_value_is_true(value: &str) -> bool {
    let value = value.trim_matches('"');
    ["true", "yes", "on", "1"]
        .into_iter()
        .any(|truthy| value.eq_ignore_ascii_case(truthy))
}

/// Reports whether a directory below `main_top`, down to the mapped path
/// itself, holds a `.git` entry: an independent repository nested in the main
/// checkout owns its own namespace.
fn nested_repository_below(main_top: &Path, relative: &Path) -> Result<bool> {
    let mut directory = main_top.to_path_buf();
    for component in relative.components() {
        directory.push(component);
        let dot_git = directory.join(".git");
        match fs::symlink_metadata(&dot_git) {
            Ok(_) => return Ok(true),
            Err(error) if is_absent(&error) => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to inspect Git metadata entry {}", dot_git.display())
                });
            }
        }
    }
    Ok(false)
}

fn is_absent(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory)
}

#[cfg(unix)]
fn owned_by_current_user(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    // SAFETY: geteuid has no pointer or lifetime requirements.
    metadata.uid() == unsafe { libc::geteuid() }
}

#[cfg(not(unix))]
fn owned_by_current_user(_metadata: &fs::Metadata) -> bool {
    true
}

/// A `.git` pointer that claims a linked worktree but fails the proof.
#[derive(Debug)]
pub(super) struct UnverifiedWorktree {
    top: PathBuf,
    pub(super) reason: String,
}

impl fmt::Display for UnverifiedWorktree {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} has a Git worktree pointer, but Jig could not verify it as a linked worktree: {}. Refusing to derive a repo-scoped vault namespace from unverified Git metadata. {WORKTREE_REPAIR_STEP} {VAULT_STORAGE_OPERATOR_STEP} for diagnostics, pass an absolute --home <path> to select a vault explicitly",
            self.top.display(),
            self.reason
        )
    }
}

impl std::error::Error for UnverifiedWorktree {}

fn unverified(top: &Path, reason: &str) -> anyhow::Error {
    anyhow::Error::new(UnverifiedWorktree {
        top: top.to_path_buf(),
        reason: reason.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::config_sets_bare_or_worktree;

    #[test]
    fn config_scan_detects_bare_and_relocated_worktrees() {
        for config in [
            "[core]\n\tbare = true\n",
            "[Core]\n\tBare = Yes ; comment\n",
            "[core]\n\tbare\n",
            "[core] bare = on\n",
            "[core]\n\tworktree = ../elsewhere\n",
            "[user]\n\tname = Fixture\n[core]\n\tbare = 1\n",
        ] {
            assert!(config_sets_bare_or_worktree(config.as_bytes()), "{config}");
        }
        for config in [
            "[core]\n\tbare = false\n\trepositoryformatversion = 0\n",
            "[core]\n\tbare =\n",
            "[core]\n\t# bare = true\n",
            "[core \"sub\"]\n\tbare = true\n",
            "[extensions]\n\tworktree = true\n[core]\n\tbare = false\n",
        ] {
            assert!(!config_sets_bare_or_worktree(config.as_bytes()), "{config}");
        }
    }
}
