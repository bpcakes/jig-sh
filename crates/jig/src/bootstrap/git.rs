use std::cell::Cell;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, bail};
use tempfile::Builder as TempDirBuilder;

#[cfg(test)]
use crate::process::run_checked_output;
use crate::process::{require_success, run_checked_stdout_trimmed};

use super::{GIT_BIN_ENV, external_program};

pub(super) fn is_git_work_tree(path: &Path) -> bool {
    git_command(path, ["rev-parse", "--is-inside-work-tree"])
        .output()
        .is_ok_and(|output| output.status.success())
}

pub(super) fn ensure_clean_git_work_tree(path: &Path) -> Result<()> {
    let status = git_stdout(path, ["-c", "core.fsmonitor=false", "status", "--short"])?;
    if !status.is_empty() {
        bail!(
            "Local committed template mode requires a clean git working tree: {}\n\
             Commit or stash template changes before using this template source.",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn git(path: &Path, args: impl IntoIterator<Item = impl AsRef<str>>) -> Result<()> {
    let mut command = git_command(path, args);
    run_checked_output(&mut command, |output| {
        git_command_failed_message(path, output)
    })?;
    Ok(())
}

pub(super) fn git_stdout(
    path: &Path,
    args: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<String> {
    let mut command = git_command(path, args);
    run_checked_stdout_trimmed(&mut command, |output| {
        git_command_failed_message(path, output)
    })
}

pub(super) fn git_command(path: &Path, args: impl IntoIterator<Item = impl AsRef<str>>) -> Command {
    let git_program = external_program(GIT_BIN_ENV, "git");
    let mut command = Command::new(git_program);
    command.current_dir(path).arg("--no-replace-objects");
    scrub_known_repository_git_environment(&mut command);
    for arg in args {
        command.arg(arg.as_ref());
    }
    command
}

#[cfg(test)]
pub(super) fn init_git_repo(destination: &Path, default_branch: &str) -> Result<bool> {
    init_git_repo_with_validation(destination, default_branch, || Ok(()))
}

pub(super) fn init_git_repo_with_validation(
    destination: &Path,
    default_branch: &str,
    mut validate_destination: impl FnMut() -> Result<()>,
) -> Result<bool> {
    let destination_git = destination.join(".git");
    match fs::symlink_metadata(&destination_git) {
        Ok(_) => {
            validate_existing_git_work_tree_at_boundary(
                destination,
                &mut validate_destination,
                "before accepting existing Git metadata",
            )?;
            return Ok(false);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Failed to inspect git metadata {}",
                    destination_git.display()
                )
            });
        }
    }

    // Git may leave a partial .git directory after any failed init/fallback
    // step. Build it in a private sibling working tree, then publish only the
    // completed metadata directory with a no-replace rename. A concurrently
    // created destination .git wins without being modified.
    validate_destination().context("Git init destination validation failed before staging")?;
    let staged = private_tempdir_in(destination, ".jig-git-init-").with_context(|| {
        format!(
            "Failed to create private git init staging directory in {}",
            destination.display()
        )
    })?;
    let staged_destination = staged.path().to_path_buf();
    let staged_git = staged_destination.join(".git");

    let git_program = external_program(GIT_BIN_ENV, "git");
    let initialization = (|| {
        staged.require_identity("before preparing the private Git template")?;
        let template_dir =
            prepare_private_git_template(&git_program, &staged_destination, &staged_git)?;
        staged.require_identity("after preparing the private Git template")?;
        let mut with_branch_command =
            staged_repository_command(&git_program, &staged_destination, &staged_git);
        apply_private_git_template(&mut with_branch_command, template_dir.as_deref());
        staged.require_identity("before running git init")?;
        let with_branch = with_branch_command
            .args(["init", "-b", default_branch])
            .output()
            .with_context(|| format!("Failed to start {}", git_program))?;
        staged.require_identity("after running git init")?;
        if !with_branch.status.success() {
            if !git_init_branch_flag_unsupported(&with_branch) {
                bail!(
                    "git init -b {default_branch} failed.\nstdout:\n{}\nstderr:\n{}",
                    String::from_utf8_lossy(&with_branch.stdout),
                    String::from_utf8_lossy(&with_branch.stderr)
                );
            }

            let mut fallback_command =
                staged_repository_command(&git_program, &staged_destination, &staged_git);
            apply_private_git_template(&mut fallback_command, template_dir.as_deref());
            staged.require_identity("before running fallback git init")?;
            let fallback = fallback_command
                .arg("init")
                .output()
                .with_context(|| format!("Failed to start {}", git_program))?;
            staged.require_identity("after running fallback git init")?;
            require_success(&fallback, |output| {
                format!(
                    "git init failed.\nstdout:\n{}\nstderr:\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                )
            })?;
            set_git_head_branch(
                &staged_destination,
                &staged_git,
                &git_program,
                default_branch,
            )?;
            staged.require_identity("after setting the fallback Git branch")?;
        }

        staged.require_identity("before validating initialized Git metadata")?;
        validate_staged_git_repository(
            &staged_destination,
            &staged_git,
            &git_program,
            default_branch,
        )?;
        staged.require_identity("after validating initialized Git metadata")?;
        let git_directory = super::path::repository_directory_commit_at(&staged_git)
            .context("Failed to retain initialized Git metadata directory identity")?;
        require_staged_git_directory_identity(
            &staged,
            &git_directory,
            &staged_git,
            "after retaining initialized Git metadata",
        )?;
        Ok(git_directory)
    })();
    let (staged, staged_git_commit) =
        retain_staging_directory_on_success(staged, &staged_destination, initialization)?;

    if let Err(error) = staged
        .require_identity("before destination validation for metadata staging")
        .and_then(|()| {
            require_staged_git_directory_identity(
                &staged,
                &staged_git_commit,
                &staged_git,
                "before destination validation for metadata staging",
            )
        })
        .and_then(|()| {
            validate_destination()
                .context("Git init destination validation failed before metadata staging")
        })
        .and_then(|()| staged.require_identity("after destination validation for metadata staging"))
        .and_then(|()| {
            require_staged_git_directory_identity(
                &staged,
                &staged_git_commit,
                &staged_git,
                "after destination validation for metadata staging",
            )
        })
    {
        return close_staging_directory(staged, &staged_destination, Err(error));
    }
    let metadata_stage =
        match private_tempdir_in(destination, ".jig-git-metadata-").with_context(|| {
            format!(
                "Failed to create private git metadata staging directory in {}",
                destination.display()
            )
        }) {
            Ok(metadata_stage) => metadata_stage,
            Err(error) => {
                return close_staging_directory(staged, &staged_destination, Err(error));
            }
        };
    let metadata_stage_path = metadata_stage.path().to_path_buf();

    let transfer = (|| {
        staged.require_identity("before transferring initialized Git metadata")?;
        metadata_stage.require_identity("before receiving initialized Git metadata")?;
        let permissions = fs::symlink_metadata(&staged_git)
            .with_context(|| {
                format!(
                    "Failed to inspect staged git metadata permissions {}",
                    staged_git.display()
                )
            })?
            .permissions();
        move_directory_contents(
            &staged_git,
            &metadata_stage_path,
            &staged,
            &staged_git_commit,
            &metadata_stage,
        )?;
        staged.require_identity("after transferring initialized Git metadata")?;
        metadata_stage.require_identity("after receiving initialized Git metadata")?;
        validate_staged_git_repository(
            &staged_destination,
            &metadata_stage_path,
            &git_program,
            default_branch,
        )?;
        staged.require_identity("after validating the disposable Git worktree")?;
        metadata_stage.require_identity("after validating staged Git metadata")?;
        Ok(permissions)
    })();
    let final_git_permissions = match transfer {
        Ok(permissions) => permissions,
        Err(error) => {
            let error =
                close_staging_directory::<()>(metadata_stage, &metadata_stage_path, Err(error))
                    .expect_err("an error remains an error after metadata staging cleanup");
            return close_staging_directory(staged, &staged_destination, Err(error));
        }
    };

    if let Err(error) = close_worktree_staging_before_publication(staged, &staged_destination) {
        return close_staging_directory(metadata_stage, &metadata_stage_path, Err(error));
    }

    // All disposable worktree cleanup has succeeded. Publication keeps this
    // guard through the no-replace rename, disarming it only after success and
    // explicitly closing it on every failure or contention path.
    if let Err(error) = metadata_stage
        .require_identity("before destination validation for Git metadata publication")
        .and_then(|()| {
            validate_destination()
                .context("Git init destination validation failed before .git publication")
        })
        .and_then(|()| {
            metadata_stage
                .require_identity("after destination validation for Git metadata publication")
        })
    {
        return close_staging_directory(metadata_stage, &metadata_stage_path, Err(error));
    }
    publish_staged_git_directory(
        metadata_stage,
        &destination_git,
        final_git_permissions,
        &mut validate_destination,
    )
}

enum ExistingGitMetadataCommit {
    Directory(super::path::RepositoryDirectoryCommit),
    GitFile(super::path::RepositoryFileCommit),
}

impl ExistingGitMetadataCommit {
    fn retain(path: &Path) -> Result<Self> {
        let metadata = fs::symlink_metadata(path).with_context(|| {
            format!("Failed to inspect existing Git metadata {}", path.display())
        })?;
        if metadata.file_type().is_dir() {
            return super::path::repository_directory_commit_at(path)
                .map(Self::Directory)
                .with_context(|| {
                    format!(
                        "Existing Git metadata must be an identity-retained real directory: {}",
                        path.display()
                    )
                });
        }
        if super::path::repository_metadata_is_real_regular_file(&metadata) {
            return super::path::repository_file_fingerprint_at(path)
                .map(Self::GitFile)
                .with_context(|| {
                    format!(
                        "Existing Git metadata file could not be retained safely: {}",
                        path.display()
                    )
                });
        }
        bail!(
            "Existing Git metadata must be a real directory or regular gitfile, not a symlink, reparse point, or special file: {}",
            path.display()
        )
    }

    fn require_matches_path(&self, path: &Path, action: &str) -> Result<()> {
        let matches = match self {
            Self::Directory(commit) => {
                super::path::repository_directory_commit_matches_path(commit, path)?
            }
            Self::GitFile(commit) => {
                super::path::repository_file_commit_matches_path(commit, path)?
            }
        };
        if !matches {
            bail!(
                "Existing Git metadata {} changed concurrently {action}; refusing to accept an unverified repository",
                path.display()
            );
        }
        Ok(())
    }
}

fn validate_existing_git_work_tree_at_boundary(
    destination: &Path,
    validate_destination: &mut impl FnMut() -> Result<()>,
    boundary: &str,
) -> Result<()> {
    validate_destination()
        .with_context(|| format!("Git init destination validation failed {boundary}"))?;
    let metadata = validate_existing_git_work_tree(destination)?;
    validate_destination()
        .with_context(|| format!("Git init destination validation failed after {boundary}"))?;
    metadata.require_matches_path(
        &destination.join(".git"),
        &format!("after destination validation {boundary}"),
    )
}

fn validate_existing_git_work_tree(destination: &Path) -> Result<ExistingGitMetadataCommit> {
    let destination_git = destination.join(".git");
    let metadata = ExistingGitMetadataCommit::retain(&destination_git)?;
    metadata.require_matches_path(&destination_git, "before repository validation")?;

    let inside = existing_git_stdout(destination, ["rev-parse", "--is-inside-work-tree"])
        .context("Existing Git metadata does not identify a usable work tree")?;
    metadata.require_matches_path(&destination_git, "after work-tree validation")?;
    if inside != "true" {
        bail!(
            "Existing Git metadata at {} does not identify a work tree (Git reported {inside:?})",
            destination_git.display()
        );
    }

    let bare = existing_git_stdout(destination, ["rev-parse", "--is-bare-repository"])
        .context("Failed to validate whether existing Git metadata is bare")?;
    metadata.require_matches_path(&destination_git, "after bare-repository validation")?;
    if bare != "false" {
        bail!(
            "Existing Git metadata at {} identifies a bare repository (Git reported {bare:?})",
            destination_git.display()
        );
    }

    let reported_root = existing_git_work_tree_root(destination)
        .context("Failed to resolve the existing Git work-tree root")?;
    metadata.require_matches_path(&destination_git, "after work-tree root resolution")?;
    let expected_root = fs::canonicalize(destination).with_context(|| {
        format!(
            "Failed to resolve expected Git work-tree root {}",
            destination.display()
        )
    })?;
    if reported_root != expected_root {
        bail!(
            "Existing Git metadata at {} resolves a different work-tree root: expected {}, reported {}",
            destination_git.display(),
            expected_root.display(),
            reported_root.display()
        );
    }

    let prefix = existing_git_stdout(destination, ["rev-parse", "--show-prefix"])
        .context("Failed to validate the existing Git work-tree root")?;
    metadata.require_matches_path(&destination_git, "after work-tree root validation")?;
    if !prefix.is_empty() {
        bail!(
            "Existing Git metadata at {} resolves the destination as nested path {prefix:?}, not as the repository root",
            destination_git.display()
        );
    }

    let mut status = existing_git_command(
        destination,
        [
            "-c",
            "core.fsmonitor=false",
            "status",
            "--porcelain=v1",
            "--untracked-files=no",
        ],
    );
    status.env("GIT_OPTIONAL_LOCKS", "0");
    let status = status
        .output()
        .with_context(|| format!("Failed to start Git in {}", destination.display()))?;
    require_success(&status, |output| {
        git_command_failed_message(destination, output)
    })
    .context("Existing Git metadata does not identify a usable repository")?;
    metadata.require_matches_path(&destination_git, "after repository status validation")?;
    Ok(metadata)
}

fn existing_git_work_tree_root(destination: &Path) -> Result<PathBuf> {
    let mut command = existing_git_command(destination, ["rev-parse", "--show-toplevel"]);
    let mut output = command
        .output()
        .with_context(|| format!("Failed to start Git in {}", destination.display()))?;
    require_success(&output, |output| {
        git_command_failed_message(destination, output)
    })?;
    strip_git_path_line_ending(&mut output.stdout)?;
    let reported = PathBuf::from(git_path_from_bytes(output.stdout)?);
    let reported = if reported.is_absolute() {
        reported
    } else {
        destination.join(reported)
    };
    fs::canonicalize(&reported).with_context(|| {
        format!(
            "Git reported an unusable work-tree root {}",
            reported.display()
        )
    })
}

fn existing_git_stdout(
    destination: &Path,
    args: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<String> {
    let mut command = existing_git_command(destination, args);
    run_checked_stdout_trimmed(&mut command, |output| {
        git_command_failed_message(destination, output)
    })
}

fn existing_git_command(
    destination: &Path,
    args: impl IntoIterator<Item = impl AsRef<str>>,
) -> Command {
    let git_program = external_program(GIT_BIN_ENV, "git");
    let mut command = Command::new(git_program);
    command.current_dir(destination).arg("--no-replace-objects");
    scrub_git_repository_environment(&mut command);
    command
        .env("GIT_CONFIG_GLOBAL", null_git_config_path())
        .env("GIT_CONFIG_SYSTEM", null_git_config_path())
        .env("GIT_CONFIG_NOSYSTEM", "1");
    for arg in args {
        command.arg(arg.as_ref());
    }
    command
}

struct StagingDirectory {
    path: PathBuf,
    commit: super::path::RepositoryDirectoryCommit,
    preserve_for_recovery: Cell<bool>,
}

impl StagingDirectory {
    fn path(&self) -> &Path {
        &self.path
    }

    fn require_identity(&self, action: &str) -> Result<()> {
        self.require_identity_at(&self.path, action)
    }

    fn require_identity_at(&self, path: &Path, action: &str) -> Result<()> {
        let metadata = fs::symlink_metadata(path).with_context(|| {
            format!(
                "Git staging directory {} disappeared {action}",
                path.display()
            )
        })?;
        if !metadata.file_type().is_dir() {
            bail!(
                "Git staging path {} was replaced by a non-directory {action}; preserving the foreign replacement",
                path.display()
            );
        }
        if !super::path::repository_directory_commit_matches_path(&self.commit, path)? {
            bail!(
                "Git staging directory {} was replaced concurrently {action}; preserving the foreign replacement",
                path.display()
            );
        }
        Ok(())
    }

    fn preserve_for_recovery(&self) {
        self.preserve_for_recovery.set(true);
    }
}

fn require_new_staging_directory(path: &Path) -> Result<super::path::RepositoryDirectoryCommit> {
    let metadata = fs::symlink_metadata(path).with_context(|| {
        format!(
            "Failed to inspect newly created Git staging directory {}; preserving it for manual recovery",
            path.display()
        )
    })?;
    if !metadata.file_type().is_dir() {
        bail!(
            "Newly created Git staging path {} is not a real directory; preserving it for manual recovery",
            path.display()
        );
    }
    super::path::repository_directory_commit_at(path).with_context(|| {
        format!(
            "Failed to retain newly created Git staging directory {}; preserving it for manual recovery",
            path.display()
        )
    })
}

fn private_tempdir_in(parent: &Path, prefix: &str) -> Result<StagingDirectory> {
    let mut builder = TempDirBuilder::new();
    builder.prefix(prefix);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    let temporary = builder.tempdir_in(parent)?;
    // `TempDir` recursively removes whatever currently occupies its path on
    // drop. Disarm that path-based cleanup immediately: all cleanup below is
    // identity-checked and quarantined first, so a concurrent replacement is
    // preserved rather than recursively deleted.
    let path = temporary.keep();
    let commit = require_new_staging_directory(&path)?;
    Ok(StagingDirectory {
        path,
        commit,
        preserve_for_recovery: Cell::new(false),
    })
}

fn move_directory_contents(
    source: &Path,
    destination: &Path,
    source_root: &StagingDirectory,
    source_commit: &super::path::RepositoryDirectoryCommit,
    destination_root: &StagingDirectory,
) -> Result<()> {
    move_directory_contents_with(
        source,
        destination,
        source_root,
        source_commit,
        destination_root,
        || Ok(()),
    )
}

fn move_directory_contents_with(
    source: &Path,
    destination: &Path,
    source_root: &StagingDirectory,
    source_commit: &super::path::RepositoryDirectoryCommit,
    destination_root: &StagingDirectory,
    after_snapshot: impl FnOnce() -> Result<()>,
) -> Result<()> {
    source_root.require_identity("before reading initialized Git metadata")?;
    require_staged_git_directory_identity(
        source_root,
        source_commit,
        source,
        "before reading initialized Git metadata",
    )?;
    destination_root.require_identity("before receiving initialized Git metadata")?;
    let mut entry_names = Vec::<OsString>::new();
    let entries = fs::read_dir(source)
        .with_context(|| format!("Failed to read staged git metadata {}", source.display()));
    let entries = match entries {
        Ok(entries) => entries,
        Err(error) => {
            source_root.preserve_for_recovery();
            return Err(error).context(
                "Could not snapshot initialized Git metadata; preserving the staging tree for recovery",
            );
        }
    };
    for entry in entries {
        match entry {
            Ok(entry) => entry_names.push(entry.file_name()),
            Err(error) => {
                source_root.preserve_for_recovery();
                return Err(error)
                    .with_context(|| {
                        format!("Failed to inspect staged git metadata {}", source.display())
                    })
                    .context(
                        "Could not complete the initialized Git metadata snapshot; preserving the staging tree for recovery",
                    );
            }
        }
    }
    entry_names.sort();
    if let Err(error) = after_snapshot() {
        source_root.preserve_for_recovery();
        return Err(error).context(
            "Initialized Git metadata snapshot hook failed; preserving the source staging tree for recovery",
        );
    }

    source_root.require_identity("after snapshotting initialized Git metadata")?;
    require_staged_git_directory_identity(
        source_root,
        source_commit,
        source,
        "after snapshotting initialized Git metadata",
    )?;
    destination_root.require_identity("after snapshotting initialized Git metadata")?;

    for entry_name in entry_names {
        let source_entry = source.join(&entry_name);
        let destination_entry = destination.join(&entry_name);
        source_root.require_identity("before moving an initialized Git metadata entry")?;
        require_staged_git_directory_identity(
            source_root,
            source_commit,
            source,
            "before moving an initialized Git metadata entry",
        )?;
        destination_root.require_identity("before receiving an initialized Git metadata entry")?;
        if let Err(error) = super::path::rename_entry_noreplace(&source_entry, &destination_entry)
            .with_context(|| {
                format!(
                    "Failed to transfer staged git metadata {} to {}",
                    source_entry.display(),
                    destination_entry.display()
                )
            })
        {
            source_root.preserve_for_recovery();
            return Err(error).context(
                "Initialized Git metadata changed during transfer; preserving the source staging tree for recovery",
            );
        }
        source_root.require_identity("after moving an initialized Git metadata entry")?;
        require_staged_git_directory_identity(
            source_root,
            source_commit,
            source,
            "after moving an initialized Git metadata entry",
        )?;
        destination_root.require_identity("after receiving an initialized Git metadata entry")?;
    }
    require_staged_git_directory_identity(
        source_root,
        source_commit,
        source,
        "after transferring initialized Git metadata",
    )?;
    source_root.require_identity("after transferring initialized Git metadata")?;
    destination_root.require_identity("after transferring initialized Git metadata")?;

    let mut remaining = match fs::read_dir(source).with_context(|| {
        format!(
            "Failed to prove staged git metadata {} is empty after transfer",
            source.display()
        )
    }) {
        Ok(entries) => entries,
        Err(error) => {
            source_root.preserve_for_recovery();
            return Err(error).context(
                "Could not inspect the initialized Git metadata source; preserving the source staging tree for recovery",
            );
        }
    };
    match remaining.next() {
        Some(Ok(entry)) => {
            source_root.preserve_for_recovery();
            bail!(
                "Initialized Git metadata entry {:?} appeared after the transfer snapshot; preserving the source staging tree for recovery",
                entry.file_name()
            );
        }
        Some(Err(error)) => {
            source_root.preserve_for_recovery();
            return Err(error).context(
                "Could not prove the initialized Git metadata source is empty; preserving the source staging tree for recovery",
            );
        }
        None => {}
    }
    if let Err(error) = fs::remove_dir(source).with_context(|| {
        format!(
            "Failed to remove empty staged git metadata source {}",
            source.display()
        )
    }) {
        source_root.preserve_for_recovery();
        return Err(error).context(
            "Could not atomically prove the initialized Git metadata source remained empty; preserving the source staging tree for recovery",
        );
    }
    source_root.require_identity("after removing the empty Git metadata source")?;
    destination_root.require_identity("after removing the empty Git metadata source")?;
    Ok(())
}

fn require_staged_git_directory_identity(
    staging_root: &StagingDirectory,
    expected: &super::path::RepositoryDirectoryCommit,
    path: &Path,
    action: &str,
) -> Result<()> {
    match super::path::repository_directory_commit_matches_path(expected, path) {
        Ok(true) => Ok(()),
        Ok(false) => {
            staging_root.preserve_for_recovery();
            bail!(
                "Initialized Git metadata directory {} was replaced concurrently {action}; preserving the staging tree for recovery",
                path.display()
            )
        }
        Err(error) => {
            staging_root.preserve_for_recovery();
            Err(error).with_context(|| {
                format!(
                    "Failed to verify initialized Git metadata directory {} {action}; preserving the staging tree for recovery",
                    path.display()
                )
            })
        }
    }
}

fn staged_repository_command(git_program: &str, work_tree: &Path, git_dir: &Path) -> Command {
    let mut command = repository_command(git_program, work_tree, git_dir);
    scrub_git_repository_environment(&mut command);
    command
        .env("GIT_CONFIG_GLOBAL", null_git_config_path())
        .env("GIT_CONFIG_SYSTEM", null_git_config_path())
        .env("GIT_CONFIG_NOSYSTEM", "1");
    command
}

fn ambient_config_repository_command(
    git_program: &str,
    work_tree: &Path,
    git_dir: &Path,
) -> Command {
    let mut command = repository_command(git_program, work_tree, git_dir);
    scrub_git_repository_environment_for_ambient_config(&mut command);
    command
}

fn repository_command(git_program: &str, work_tree: &Path, git_dir: &Path) -> Command {
    let mut command = Command::new(git_program);
    command
        .current_dir(work_tree)
        .arg("--no-replace-objects")
        .arg("--git-dir")
        .arg(git_dir)
        .arg("--work-tree")
        .arg(work_tree);
    command
}

fn scrub_git_repository_environment(command: &mut Command) {
    scrub_git_repository_environment_except(command, &[]);
}

fn scrub_git_repository_environment_for_ambient_config(command: &mut Command) {
    // A caller's ordinary execution environment (PATH, HOME, temporary
    // directories, locale, and so on) remains intact. Git-specific inputs are
    // deny-by-default because several undocumented/internal variables also
    // redirect repository writes. These variables are retained only while
    // resolving the effective ambient template; all mutating and validation
    // commands use deterministic empty global/system config instead.
    const ALLOWED_GIT_ENVIRONMENT: &[&str] = &[
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_CONFIG_SYSTEM",
    ];
    scrub_git_repository_environment_except(command, ALLOWED_GIT_ENVIRONMENT);
}

pub(super) fn scrub_known_repository_git_environment(command: &mut Command) {
    // Keep the user's ordinary environment, read-only config sources, and the
    // authentication knobs needed by remote template fetches. Repository
    // discovery/redirection, alternate object/index paths, quarantine state,
    // replacement refs, namespaces, and command-scoped config are stripped so
    // a command aimed at a known repository cannot escape to ambient metadata.
    const ALLOWED_GIT_ENVIRONMENT: &[&str] = &[
        "GIT_ASKPASS",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_CONFIG_SYSTEM",
        "GIT_SSH",
        "GIT_SSH_COMMAND",
        "GIT_SSH_VARIANT",
        "GIT_TERMINAL_PROMPT",
    ];
    scrub_git_repository_environment_except(command, ALLOWED_GIT_ENVIRONMENT);
}

pub(super) fn scrub_remote_template_git_environment(command: &mut Command) {
    // Remote template resolution must retain the caller's explicit transport
    // and authentication policy. Keep this allowlist separate from repository
    // mutation: none of these values may redirect Git metadata, objects, the
    // index, refs, or the work tree. Trace/debug variables stay scrubbed because
    // they can expose credentials in captured clone diagnostics.
    const ALLOWED_GIT_ENVIRONMENT: &[&str] = &[
        "GIT_ASKPASS",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_CONFIG_SYSTEM",
        "GIT_HTTP_LOW_SPEED_LIMIT",
        "GIT_HTTP_LOW_SPEED_TIME",
        "GIT_HTTP_MAX_REQUESTS",
        "GIT_HTTP_PROXY_AUTHMETHOD",
        "GIT_HTTP_USER_AGENT",
        "GIT_PROXY_COMMAND",
        "GIT_SSH",
        "GIT_SSH_COMMAND",
        "GIT_SSH_VARIANT",
        "GIT_SSL_CAINFO",
        "GIT_SSL_CAPATH",
        "GIT_SSL_CIPHER_LIST",
        "GIT_SSL_NO_VERIFY",
        "GIT_SSL_VERSION",
        "GIT_TERMINAL_PROMPT",
    ];
    scrub_git_repository_environment_except(command, ALLOWED_GIT_ENVIRONMENT);
}

pub(super) fn disable_git_worktree_integrations(command: &mut Command) {
    // These values are installed only after inherited command-scoped config is
    // removed. Clone and checkout must not execute an ambient hook or fsmonitor
    // process while preparing a supposedly private template checkout.
    command
        .env("GIT_CONFIG_COUNT", "2")
        .env("GIT_CONFIG_KEY_0", "core.hooksPath")
        .env("GIT_CONFIG_VALUE_0", null_git_config_path())
        .env("GIT_CONFIG_KEY_1", "core.fsmonitor")
        .env("GIT_CONFIG_VALUE_1", "false");
}

fn scrub_git_repository_environment_except(command: &mut Command, allowed: &[&str]) {
    for (name, _) in env::vars_os() {
        let normalized = name.to_string_lossy().to_ascii_uppercase();
        if normalized.starts_with("GIT_") && !allowed.contains(&normalized.as_str()) {
            command.env_remove(name);
        }
    }
}

#[cfg(unix)]
fn null_git_config_path() -> &'static OsStr {
    OsStr::new("/dev/null")
}

#[cfg(windows)]
fn null_git_config_path() -> &'static OsStr {
    OsStr::new("NUL")
}

#[cfg(not(any(unix, windows)))]
fn null_git_config_path() -> &'static OsStr {
    OsStr::new("")
}

fn prepare_private_git_template(
    git_program: &str,
    work_tree: &Path,
    git_dir: &Path,
) -> Result<Option<PathBuf>> {
    let source = match inherited_git_template_dir() {
        Some(source) => Some(source),
        None => match configured_git_template_dir(git_program, work_tree, git_dir)? {
            Some(source) => Some(source),
            None => default_git_template_dir(git_program, work_tree)?,
        },
    };
    let private = work_tree.join(".jig-git-template");
    if let Some(source) = source.filter(|source| !source.is_empty()) {
        let source = PathBuf::from(source);
        let source = if source.is_absolute() {
            source
        } else {
            work_tree.join(source)
        };
        copy_git_template_directory(&source, &private)?;
    } else {
        create_empty_private_git_template(&private)?;
    }
    for relative in ["commondir", "gitdir", "config.worktree"] {
        require_absent_git_template_redirect(&private.join(relative))?;
    }
    require_empty_or_absent_alternates(&private.join("objects/info/alternates"))?;
    Ok(Some(private))
}

fn inherited_git_template_dir() -> Option<std::ffi::OsString> {
    env::vars_os().find_map(|(name, value)| {
        let matches = if cfg!(windows) {
            name.to_string_lossy()
                .eq_ignore_ascii_case("GIT_TEMPLATE_DIR")
        } else {
            name == "GIT_TEMPLATE_DIR"
        };
        matches.then_some(value)
    })
}

fn configured_git_template_dir(
    git_program: &str,
    work_tree: &Path,
    git_dir: &Path,
) -> Result<Option<std::ffi::OsString>> {
    let mut output = ambient_config_repository_command(git_program, work_tree, git_dir)
        .args(["config", "--path", "--get", "init.templateDir"])
        .output()
        .with_context(|| format!("Failed to start {git_program}"))?;
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    require_success(&output, |output| {
        staged_git_command_failed_message(work_tree, output)
    })?;
    strip_git_path_line_ending(&mut output.stdout)?;
    git_path_from_bytes(output.stdout).map(Some)
}

fn default_git_template_dir(
    git_program: &str,
    work_tree: &Path,
) -> Result<Option<std::ffi::OsString>> {
    let mut command = Command::new(git_program);
    command.current_dir(work_tree).arg("--exec-path");
    scrub_git_repository_environment(&mut command);
    let mut output = command
        .output()
        .with_context(|| format!("Failed to start {git_program}"))?;
    require_success(&output, |output| {
        staged_git_command_failed_message(work_tree, output)
    })?;
    strip_git_path_line_ending(&mut output.stdout)?;
    let exec_path = PathBuf::from(git_path_from_bytes(output.stdout)?);
    let exec_path = if exec_path.is_absolute() {
        exec_path
    } else {
        work_tree.join(exec_path)
    };
    let Some(prefix) = exec_path.parent().and_then(Path::parent) else {
        return Ok(None);
    };
    let candidate = prefix.join("share/git-core/templates");
    match fs::symlink_metadata(&candidate) {
        Ok(_) => Ok(Some(candidate.into_os_string())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| {
            format!(
                "Failed to inspect Git's default template directory {}",
                candidate.display()
            )
        }),
    }
}

fn strip_git_path_line_ending(bytes: &mut Vec<u8>) -> Result<()> {
    while bytes
        .last()
        .is_some_and(|byte| matches!(byte, b'\n' | b'\r'))
    {
        // Remove only Git's line ending; spaces are meaningful in paths.
        let _ = bytes.pop();
    }
    if bytes.contains(&b'\0') || bytes.contains(&b'\n') || bytes.contains(&b'\r') {
        bail!("Configured Git path contains an unsupported NUL or line break");
    }
    Ok(())
}

fn create_empty_private_git_template(path: &Path) -> Result<()> {
    let builder = &mut fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;

        builder.mode(0o700);
    }
    builder.create(path).with_context(|| {
        format!(
            "Failed to create private git template directory {}",
            path.display()
        )
    })
}

#[cfg(unix)]
fn git_path_from_bytes(bytes: Vec<u8>) -> Result<std::ffi::OsString> {
    use std::os::unix::ffi::OsStringExt;

    Ok(std::ffi::OsString::from_vec(bytes))
}

#[cfg(not(unix))]
fn git_path_from_bytes(bytes: Vec<u8>) -> Result<std::ffi::OsString> {
    let value = String::from_utf8(bytes)
        .context("Git-reported path is not valid UTF-8 on this platform")?;
    Ok(value.into())
}

fn copy_git_template_directory(source: &Path, destination: &Path) -> Result<()> {
    let before = snapshot_git_template_tree(source)?;
    let root = before
        .iter()
        .find(|entry| entry.relative.as_os_str().is_empty())
        .context("Git template snapshot did not contain its root directory")?;
    if root.kind != GitTemplateEntryKind::Directory {
        bail!(
            "Git template source is not a real directory: {}",
            source.display()
        );
    }
    fs::create_dir(destination).with_context(|| {
        format!(
            "Failed to create private git template directory {}",
            destination.display()
        )
    })?;
    copy_git_template_entries(source, destination, &before)?;
    let after = snapshot_git_template_tree(source)?;
    if before != after {
        bail!(
            "Git template source changed while it was copied: {}",
            source.display()
        );
    }
    fs::set_permissions(destination, root.permissions.clone()).with_context(|| {
        format!(
            "Failed to preserve git template directory permissions on {}",
            destination.display()
        )
    })
}

fn copy_git_template_entries(
    source: &Path,
    destination: &Path,
    snapshot: &[GitTemplateEntrySnapshot],
) -> Result<()> {
    for entry in snapshot
        .iter()
        .filter(|entry| !entry.relative.as_os_str().is_empty())
    {
        let source_entry = source.join(&entry.relative);
        let destination_entry = destination.join(&entry.relative);
        match entry.kind {
            GitTemplateEntryKind::Directory => {
                fs::create_dir(&destination_entry).with_context(|| {
                    format!(
                        "Failed to create private git template directory {}",
                        destination_entry.display()
                    )
                })?;
            }
            GitTemplateEntryKind::File => copy_verified_git_template_file(
                &source_entry,
                &destination_entry,
                &entry.identity,
                &entry.permissions,
            )?,
        }
    }
    for entry in snapshot.iter().rev().filter(|entry| {
        !entry.relative.as_os_str().is_empty() && entry.kind == GitTemplateEntryKind::Directory
    }) {
        let destination_entry = destination.join(&entry.relative);
        fs::set_permissions(&destination_entry, entry.permissions.clone()).with_context(|| {
            format!(
                "Failed to preserve git template directory permissions on {}",
                destination_entry.display()
            )
        })?;
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct GitTemplateEntrySnapshot {
    relative: PathBuf,
    kind: GitTemplateEntryKind,
    identity: super::path::RepositoryEntryIdentity,
    length: u64,
    modified: Option<std::time::SystemTime>,
    permission_identity: u32,
    permissions: fs::Permissions,
}

impl PartialEq for GitTemplateEntrySnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.relative == other.relative
            && self.kind == other.kind
            && self.identity == other.identity
            && self.length == other.length
            && self.modified == other.modified
            && self.permission_identity == other.permission_identity
    }
}

impl Eq for GitTemplateEntrySnapshot {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GitTemplateEntryKind {
    Directory,
    File,
}

fn snapshot_git_template_tree(root: &Path) -> Result<Vec<GitTemplateEntrySnapshot>> {
    let mut snapshot = Vec::new();
    snapshot_git_template_entry(root, Path::new(""), &mut snapshot)?;
    snapshot.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(snapshot)
}

fn snapshot_git_template_entry(
    root: &Path,
    relative: &Path,
    snapshot: &mut Vec<GitTemplateEntrySnapshot>,
) -> Result<()> {
    let path = root.join(relative);
    let metadata = fs::symlink_metadata(&path)
        .with_context(|| format!("Failed to inspect git template entry {}", path.display()))?;
    let kind = if metadata.file_type().is_dir() {
        GitTemplateEntryKind::Directory
    } else if metadata.file_type().is_file() {
        GitTemplateEntryKind::File
    } else {
        bail!(
            "Git template entry must be a regular file or real directory, not a symbolic link or special file: {}",
            path.display()
        );
    };
    snapshot.push(GitTemplateEntrySnapshot {
        relative: relative.to_path_buf(),
        kind,
        identity: super::path::repository_path_identity(&path)?,
        length: metadata.len(),
        modified: metadata.modified().ok(),
        permission_identity: git_template_permission_identity(&metadata),
        permissions: metadata.permissions(),
    });
    if kind == GitTemplateEntryKind::Directory {
        let mut children = fs::read_dir(&path)
            .with_context(|| format!("Failed to read git template directory {}", path.display()))?
            .map(|entry| {
                entry
                    .map(|entry| entry.file_name())
                    .with_context(|| format!("Failed to inspect entry in {}", path.display()))
            })
            .collect::<Result<Vec<_>>>()?;
        children.sort();
        for child in children {
            snapshot_git_template_entry(root, &relative.join(child), snapshot)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn git_template_permission_identity(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;

    metadata.mode()
}

#[cfg(windows)]
fn git_template_permission_identity(metadata: &fs::Metadata) -> u32 {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes()
}

#[cfg(not(any(unix, windows)))]
fn git_template_permission_identity(metadata: &fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

fn copy_verified_git_template_file(
    source: &Path,
    destination: &Path,
    expected_identity: &super::path::RepositoryEntryIdentity,
    permissions: &fs::Permissions,
) -> Result<()> {
    let mut source_file = open_verified_git_template_file(source, expected_identity)?;
    let mut destination_options = fs::OpenOptions::new();
    destination_options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        destination_options.mode(0o600);
    }
    let mut destination_file = destination_options.open(destination).with_context(|| {
        format!(
            "Failed to create private git template file {} without replacing an entry",
            destination.display()
        )
    })?;
    io::copy(&mut source_file, &mut destination_file).with_context(|| {
        format!(
            "Failed to copy git template file {} into private staging",
            source.display()
        )
    })?;
    destination_file.flush().with_context(|| {
        format!(
            "Failed to flush private git template file {}",
            destination.display()
        )
    })?;
    drop(destination_file);
    fs::set_permissions(destination, permissions.clone()).with_context(|| {
        format!(
            "Failed to preserve git template file permissions on {}",
            destination.display()
        )
    })?;

    let mut verification = open_verified_git_template_file(source, expected_identity)?;
    let mut copied = File::open(destination).with_context(|| {
        format!(
            "Failed to reopen private git template file {}",
            destination.display()
        )
    })?;
    if !readers_have_equal_contents(&mut verification, &mut copied)? {
        bail!(
            "Git template file changed while it was copied: {}",
            source.display()
        );
    }
    Ok(())
}

fn open_verified_git_template_file(
    path: &Path,
    expected_identity: &super::path::RepositoryEntryIdentity,
) -> Result<File> {
    if super::path::repository_path_identity(path)? != *expected_identity {
        bail!(
            "Git template entry changed before it could be opened: {}",
            path.display()
        );
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path).with_context(|| {
        format!(
            "Failed to open git template file without following links: {}",
            path.display()
        )
    })?;
    if !file
        .metadata()
        .with_context(|| {
            format!(
                "Failed to inspect opened git template file {}",
                path.display()
            )
        })?
        .is_file()
        || super::path::repository_file_identity(&file)? != *expected_identity
        || super::path::repository_path_identity(path)? != *expected_identity
    {
        bail!(
            "Git template entry changed while it was being opened: {}",
            path.display()
        );
    }
    Ok(file)
}

fn readers_have_equal_contents(left: &mut File, right: &mut File) -> Result<bool> {
    let mut left_buffer = [0_u8; 16 * 1024];
    let mut right_buffer = [0_u8; 16 * 1024];
    loop {
        let left_read = left.read(&mut left_buffer)?;
        let right_read = right.read(&mut right_buffer)?;
        if left_read != right_read || left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

fn require_absent_git_template_redirect(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => bail!(
            "Git template contains unsafe repository redirection metadata: {}",
            path.display()
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("Failed to inspect git template metadata {}", path.display())),
    }
}

fn apply_private_git_template(command: &mut Command, template_dir: Option<&Path>) {
    if let Some(template_dir) = template_dir {
        command.env("GIT_TEMPLATE_DIR", template_dir);
    }
}

fn validate_staged_git_repository(
    work_tree: &Path,
    git_dir: &Path,
    git_program: &str,
    default_branch: &str,
) -> Result<()> {
    let canonical_work_tree = fs::canonicalize(work_tree).with_context(|| {
        format!(
            "Failed to resolve staged git working tree {}",
            work_tree.display()
        )
    })?;
    let canonical_git_dir = require_real_root_directory(git_dir)?;

    for relative in [
        "objects",
        "objects/info",
        "objects/pack",
        "refs",
        "refs/heads",
        "refs/tags",
    ] {
        require_real_directory(&git_dir.join(relative), &canonical_git_dir)?;
    }
    for relative in ["HEAD", "config"] {
        require_real_file(&git_dir.join(relative), &canonical_git_dir)?;
    }
    for relative in ["commondir", "gitdir", "config.worktree"] {
        require_absent_linked_worktree_marker(&git_dir.join(relative))?;
    }
    require_empty_or_absent_alternates(&git_dir.join("objects/info/alternates"))?;

    require_git_path(
        git_program,
        work_tree,
        git_dir,
        ["rev-parse", "--git-dir"],
        &canonical_git_dir,
        "git directory",
    )?;
    require_git_path(
        git_program,
        work_tree,
        git_dir,
        ["rev-parse", "--git-common-dir"],
        &canonical_git_dir,
        "git common directory",
    )?;
    require_git_path(
        git_program,
        work_tree,
        git_dir,
        ["rev-parse", "--git-path", "objects"],
        &canonical_git_dir.join("objects"),
        "git object directory",
    )?;
    require_git_path(
        git_program,
        work_tree,
        git_dir,
        ["rev-parse", "--git-path", "refs"],
        &canonical_git_dir.join("refs"),
        "git refs directory",
    )?;
    require_git_path(
        git_program,
        work_tree,
        git_dir,
        ["rev-parse", "--show-toplevel"],
        &canonical_work_tree,
        "git working tree",
    )?;

    let inside = staged_git_stdout(
        git_program,
        work_tree,
        git_dir,
        ["rev-parse", "--is-inside-work-tree"],
    )?;
    if inside != "true" {
        bail!(
            "Staged git repository at {} did not resolve as a working tree (git reported {inside:?})",
            git_dir.display()
        );
    }

    let bare = staged_git_stdout(
        git_program,
        work_tree,
        git_dir,
        ["rev-parse", "--is-bare-repository"],
    )?;
    if bare != "false" {
        bail!(
            "Staged git repository at {} resolved as bare (git reported {bare:?})",
            git_dir.display()
        );
    }

    let branch = staged_git_stdout(git_program, work_tree, git_dir, ["symbolic-ref", "HEAD"])?;
    let expected_branch = format!("refs/heads/{default_branch}");
    if branch != expected_branch {
        bail!(
            "Staged git repository at {} has HEAD {branch:?}, expected {expected_branch:?}",
            git_dir.display()
        );
    }

    require_local_config_value(git_program, work_tree, git_dir, "core.bare", "false")?;
    require_missing_local_config_value(git_program, work_tree, git_dir, "core.worktree")?;
    require_safe_local_path_config(git_program, work_tree, git_dir, "core.hookspath")?;
    require_disabled_local_bool_config(git_program, work_tree, git_dir, "core.fsmonitor")?;

    let config = staged_git_output(
        git_program,
        work_tree,
        git_dir,
        ["config", "--local", "--name-only", "--null", "--list"],
    )?;
    require_success(&config, |output| {
        staged_git_command_failed_message(work_tree, output)
    })?;
    require_no_local_config_includes(git_dir, &config.stdout)?;

    let status = staged_git_output(
        git_program,
        work_tree,
        git_dir,
        [
            "-c",
            "core.fsmonitor=false",
            "status",
            "--porcelain=v1",
            "--untracked-files=no",
        ],
    )?;
    require_success(&status, |output| {
        staged_git_command_failed_message(work_tree, output)
    })?;
    Ok(())
}

fn require_real_directory(path: &Path, canonical_git_dir: &Path) -> Result<PathBuf> {
    let metadata = fs::symlink_metadata(path).with_context(|| {
        format!(
            "Required git metadata directory is missing: {}",
            path.display()
        )
    })?;
    if !metadata.file_type().is_dir() {
        bail!(
            "Required git metadata path is not a real directory: {}",
            path.display()
        );
    }
    require_canonical_git_path(path, canonical_git_dir)
}

fn require_real_root_directory(path: &Path) -> Result<PathBuf> {
    let metadata = fs::symlink_metadata(path).with_context(|| {
        format!(
            "Required git metadata directory is missing: {}",
            path.display()
        )
    })?;
    if !metadata.file_type().is_dir() {
        bail!(
            "Required git metadata path is not a real directory: {}",
            path.display()
        );
    }
    fs::canonicalize(path)
        .with_context(|| format!("Failed to resolve git metadata path {}", path.display()))
}

fn require_real_file(path: &Path, canonical_git_dir: &Path) -> Result<PathBuf> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Required git metadata file is missing: {}", path.display()))?;
    if !metadata.file_type().is_file() {
        bail!(
            "Required git metadata path is not a real file: {}",
            path.display()
        );
    }
    require_canonical_git_path(path, canonical_git_dir)
}

fn require_canonical_git_path(path: &Path, canonical_git_dir: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("Failed to resolve git metadata path {}", path.display()))?;
    if !canonical.starts_with(canonical_git_dir) {
        bail!(
            "Git metadata path {} resolves outside {} to {}",
            path.display(),
            canonical_git_dir.display(),
            canonical.display()
        );
    }
    Ok(canonical)
}

fn require_absent_linked_worktree_marker(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => bail!(
            "Staged git repository contains unsupported linked-worktree metadata: {}",
            path.display()
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("Failed to inspect git metadata {}", path.display()))
        }
    }
}

fn require_empty_or_absent_alternates(path: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| {
                format!("Failed to inspect git alternates file {}", path.display())
            });
        }
    };
    if !metadata.file_type().is_file() {
        bail!(
            "Git alternates metadata is not a real file: {}",
            path.display()
        );
    }
    let contents = fs::read(path)
        .with_context(|| format!("Failed to read git alternates file {}", path.display()))?;
    if contents.iter().any(|byte| !byte.is_ascii_whitespace()) {
        bail!(
            "Staged git repository redirects object reads through non-empty alternates file {}",
            path.display()
        );
    }
    Ok(())
}

fn require_git_path<const N: usize>(
    git_program: &str,
    work_tree: &Path,
    git_dir: &Path,
    args: [&str; N],
    expected: &Path,
    label: &str,
) -> Result<()> {
    let reported = staged_git_stdout(git_program, work_tree, git_dir, args)?;
    let reported = PathBuf::from(reported);
    let reported = if reported.is_absolute() {
        reported
    } else {
        work_tree.join(reported)
    };
    let reported = fs::canonicalize(&reported).with_context(|| {
        format!(
            "Git reported an unusable {label} path {}",
            reported.display()
        )
    })?;
    let expected = fs::canonicalize(expected).with_context(|| {
        format!(
            "Failed to resolve expected {label} path {}",
            expected.display()
        )
    })?;
    if reported != expected {
        bail!(
            "Git resolved {label} outside the staged repository: expected {}, reported {}",
            expected.display(),
            reported.display()
        );
    }
    Ok(())
}

fn require_local_config_value(
    git_program: &str,
    work_tree: &Path,
    git_dir: &Path,
    key: &str,
    expected: &str,
) -> Result<()> {
    let actual = staged_git_stdout(
        git_program,
        work_tree,
        git_dir,
        ["config", "--local", "--get", key],
    )?;
    if actual != expected {
        bail!(
            "Staged git repository has unsafe local {key} value {actual:?}; expected {expected:?}"
        );
    }
    Ok(())
}

fn require_missing_local_config_value(
    git_program: &str,
    work_tree: &Path,
    git_dir: &Path,
    key: &str,
) -> Result<()> {
    let output = staged_git_output(
        git_program,
        work_tree,
        git_dir,
        ["config", "--local", "--get-all", key],
    )?;
    if output.status.success() {
        bail!(
            "Staged git repository contains unsafe local {key} redirection: {:?}",
            String::from_utf8_lossy(&output.stdout).trim()
        );
    }
    if output.status.code() == Some(1) {
        return Ok(());
    }
    bail!("{}", staged_git_command_failed_message(work_tree, &output))
}

fn require_safe_local_path_config(
    git_program: &str,
    work_tree: &Path,
    git_dir: &Path,
    key: &str,
) -> Result<()> {
    let output = staged_git_output(
        git_program,
        work_tree,
        git_dir,
        ["config", "--local", "--null", "--get-all", key],
    )?;
    if output.status.code() == Some(1) {
        return Ok(());
    }
    require_success(&output, |output| {
        staged_git_command_failed_message(work_tree, output)
    })?;
    for value in output.stdout.split(|byte| *byte == 0) {
        if value.is_empty()
            || value.eq_ignore_ascii_case(b"true")
            || value.eq_ignore_ascii_case(b"false")
        {
            continue;
        }
        let value = std::str::from_utf8(value).with_context(|| {
            format!("Staged git repository has non-UTF-8 local {key} path redirection")
        })?;
        let path = Path::new(value);
        if value.starts_with('~')
            || value.starts_with("%(")
            || path.components().any(|component| {
                !matches!(
                    component,
                    std::path::Component::CurDir | std::path::Component::Normal(_)
                )
            })
        {
            bail!("Staged git repository has unsafe local {key} path redirection {value:?}");
        }
    }
    Ok(())
}

fn require_disabled_local_bool_config(
    git_program: &str,
    work_tree: &Path,
    git_dir: &Path,
    key: &str,
) -> Result<()> {
    let output = staged_git_output(
        git_program,
        work_tree,
        git_dir,
        [
            "config",
            "--local",
            "--type=bool",
            "--null",
            "--get-all",
            key,
        ],
    )?;
    if output.status.code() == Some(1) {
        return Ok(());
    }
    if !output.status.success() {
        bail!(
            "Staged git repository has an unsafe non-boolean local {key} value.\n{}",
            staged_git_command_failed_message(work_tree, &output)
        );
    }
    if output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
        .all(|value| value == b"false")
    {
        return Ok(());
    }
    bail!("Staged git repository enables unsafe local {key} integration")
}

fn require_no_local_config_includes(git_dir: &Path, names: &[u8]) -> Result<()> {
    for name in names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let normalized = name.iter().map(u8::to_ascii_lowercase).collect::<Vec<_>>();
        if normalized == b"include.path"
            || (normalized.starts_with(b"includeif.") && normalized.ends_with(b".path"))
        {
            bail!(
                "Staged git repository at {} contains unsafe repository-local config include {:?}",
                git_dir.display(),
                String::from_utf8_lossy(name)
            );
        }
    }
    Ok(())
}

fn staged_git_stdout(
    git_program: &str,
    work_tree: &Path,
    git_dir: &Path,
    args: impl IntoIterator<Item = impl AsRef<OsStr>>,
) -> Result<String> {
    let output = staged_git_output(git_program, work_tree, git_dir, args)?;
    require_success(&output, |output| {
        staged_git_command_failed_message(work_tree, output)
    })?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn staged_git_output(
    git_program: &str,
    work_tree: &Path,
    git_dir: &Path,
    args: impl IntoIterator<Item = impl AsRef<OsStr>>,
) -> Result<Output> {
    staged_repository_command(git_program, work_tree, git_dir)
        .args(args)
        .output()
        .with_context(|| format!("Failed to start {git_program}"))
}

fn staged_git_command_failed_message(path: &Path, output: &Output) -> String {
    format!(
        "git validation command failed in {}\nstdout:\n{}\nstderr:\n{}",
        path.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn close_staging_directory<T>(
    staged: StagingDirectory,
    path: &Path,
    result: Result<T>,
) -> Result<T> {
    let cleanup = cleanup_staging_directory(&staged).with_context(|| {
        format!(
            "Failed to remove git init staging directory {}",
            path.display()
        )
    });
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Err(primary), Err(cleanup)) => Err(primary.context(format!(
            "Additionally, git init staging cleanup failed: {cleanup:#}"
        ))),
    }
}

fn close_worktree_staging_before_publication(staged: StagingDirectory, path: &Path) -> Result<()> {
    cleanup_staging_directory(&staged).with_context(|| {
        format!(
            "Failed to remove git init staging directory {} before publication",
            path.display()
        )
    })?;
    #[cfg(test)]
    if env::var_os("JIG_TEST_GIT_STAGING_CLOSE_FAILURE").is_some() {
        bail!("injected git init staging cleanup failure before publication");
    }
    Ok(())
}

fn retain_staging_directory_on_success<T>(
    staged: StagingDirectory,
    path: &Path,
    result: Result<T>,
) -> Result<(StagingDirectory, T)> {
    match result {
        Ok(value) => Ok((staged, value)),
        Err(error) => {
            let error = close_staging_directory::<()>(staged, path, Err(error))
                .expect_err("an initialization error remains after staging cleanup");
            Err(error)
        }
    }
}

fn publish_staged_git_directory(
    staged: StagingDirectory,
    destination: &Path,
    final_permissions: fs::Permissions,
    validate_destination: &mut impl FnMut() -> Result<()>,
) -> Result<bool> {
    let staged_path = staged.path().to_path_buf();
    let mut publication_committed = false;
    let publication = (|| {
        staged.require_identity("before restoring final Git metadata permissions")?;
        // Keep the complete candidate private through validation and the last
        // retained-root hook. Restore the permissions Git selected for `.git`
        // only at the final publication boundary.
        fs::set_permissions(&staged_path, final_permissions).with_context(|| {
            format!(
                "Failed to restore initialized git metadata permissions on {}",
                staged_path.display()
            )
        })?;
        staged.require_identity("after restoring final Git metadata permissions")?;
        staged.require_identity("immediately before Git metadata publication")?;
        match super::path::rename_entry_noreplace(&staged_path, destination) {
            Ok(()) => {
                publication_committed = true;
                validate_published_staging_directory(destination, &staged)?;
                Ok(true)
            }
            Err(error) => match fs::symlink_metadata(destination) {
                Ok(_) => {
                    let work_tree = destination.parent().with_context(|| {
                        format!(
                            "Contended Git metadata path has no work-tree parent: {}",
                            destination.display()
                        )
                    })?;
                    validate_existing_git_work_tree_at_boundary(
                        work_tree,
                        validate_destination,
                        "after concurrent Git metadata publication",
                    )?;
                    Ok(false)
                }
                Err(inspect_error) if inspect_error.kind() == io::ErrorKind::NotFound => Err(error)
                    .with_context(|| {
                        format!(
                            "Failed to publish initialized git metadata {}",
                            destination.display()
                        )
                    }),
                Err(inspect_error) => Err(inspect_error).with_context(|| {
                    format!(
                        "Failed to inspect contended git metadata {} after publish failed: {error}",
                        destination.display()
                    )
                }),
            },
        }
    })();

    if publication_committed {
        // The source name no longer identifies any owned cleanup target. A
        // post-rename identity mismatch is quarantined by
        // `validate_published_staging_directory`; never recurse through the
        // stale staging path after the publication syscall committed.
        return publication;
    }
    close_staging_directory(staged, &staged_path, publication)
}

fn validate_published_staging_directory(
    destination: &Path,
    staged: &StagingDirectory,
) -> Result<()> {
    let metadata = fs::symlink_metadata(destination).with_context(|| {
        format!(
            "Published Git metadata {} disappeared before its identity could be verified",
            destination.display()
        )
    })?;
    if metadata.file_type().is_dir()
        && super::path::repository_directory_commit_matches_path(&staged.commit, destination)?
    {
        return Ok(());
    }

    let recovery = rename_to_unique_sibling(destination, ".jig-git-foreign-publication-")
        .with_context(|| {
            format!(
                "Published Git metadata {} had an unexpected identity and could not be quarantined",
                destination.display()
            )
        })?;
    bail!(
        "Published Git metadata {} had an unexpected identity; the foreign replacement was preserved at {}",
        destination.display(),
        recovery.display()
    )
}

fn cleanup_staging_directory(staged: &StagingDirectory) -> Result<()> {
    if staged.preserve_for_recovery.get() {
        bail!(
            "Git staging directory {} contains an unowned or unverifiable replacement; preserving the complete tree for manual recovery",
            staged.path.display()
        );
    }
    staged.require_identity("before cleanup quarantine")?;
    let quarantine = rename_to_unique_sibling(&staged.path, ".jig-git-cleanup-")?;
    staged.require_identity_at(&quarantine, "after cleanup quarantine")?;
    // `remove_dir_all` is used only after the owned directory has been moved
    // away from its published staging name and its identity has been checked
    // again. If a watcher substituted the source, the mismatch above leaves
    // that foreign tree intact at the quarantine path for manual recovery.
    staged.require_identity_at(&quarantine, "immediately before quarantined cleanup")?;
    fs::remove_dir_all(&quarantine).with_context(|| {
        format!(
            "Failed to remove quarantined Git staging directory {}",
            quarantine.display()
        )
    })
}

fn rename_to_unique_sibling(source: &Path, prefix: &str) -> Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_QUARANTINE: AtomicU64 = AtomicU64::new(0);

    let parent = source.parent().with_context(|| {
        format!(
            "Git staging path has no parent for quarantine: {}",
            source.display()
        )
    })?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for attempt in 0_u64..128 {
        let sequence = NEXT_QUARANTINE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            "{prefix}{:x}-{timestamp:x}-{sequence:x}-{attempt:x}",
            std::process::id()
        ));
        match super::path::rename_entry_noreplace(source, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) => match fs::symlink_metadata(&candidate) {
                Ok(_) => continue,
                Err(inspect_error) if inspect_error.kind() == io::ErrorKind::NotFound => {
                    return Err(error).with_context(|| {
                        format!(
                            "Failed to quarantine Git staging directory {}",
                            source.display()
                        )
                    });
                }
                Err(inspect_error) => {
                    return Err(inspect_error).with_context(|| {
                        format!(
                            "Failed to inspect Git staging quarantine candidate {} after rename failed: {error}",
                            candidate.display()
                        )
                    });
                }
            },
        }
    }
    bail!(
        "Failed to allocate an uncontended quarantine name for Git staging directory {}",
        source.display()
    )
}

fn git_init_branch_flag_unsupported(output: &std::process::Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    stderr.contains("unknown switch `b")
        || stderr.contains("unknown option `b")
        || stderr.contains("unknown option `initial-branch")
        || stderr.contains("unknown option `initial branch")
}

fn git_command_failed_message(path: &Path, output: &std::process::Output) -> String {
    format!(
        "git command failed in {}\nstdout:\n{}\nstderr:\n{}",
        path.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn set_git_head_branch(
    work_tree: &Path,
    git_dir: &Path,
    git_program: &str,
    default_branch: &str,
) -> Result<()> {
    let output = staged_repository_command(git_program, work_tree, git_dir)
        .args([
            "symbolic-ref",
            "HEAD",
            &format!("refs/heads/{default_branch}"),
        ])
        .output()
        .with_context(|| format!("Failed to start {}", git_program))?;
    require_success(&output, |output| {
        format!(
            "git symbolic-ref HEAD refs/heads/{default_branch} failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use tempfile::tempdir;

    use super::{
        GIT_BIN_ENV, init_git_repo, init_git_repo_with_validation, validate_staged_git_repository,
    };

    const HELPER_DESTINATION: &str = "JIG_GIT_INIT_AMBIENT_HELPER_DESTINATION";
    const HELPER_EXPECT_EXISTING_GIT_PRESERVED: &str =
        "JIG_GIT_INIT_AMBIENT_HELPER_EXPECT_EXISTING_GIT_PRESERVED";
    const HELPER_EXPECT_EXISTING_GIT_ACCEPTED: &str =
        "JIG_GIT_INIT_AMBIENT_HELPER_EXPECT_EXISTING_GIT_ACCEPTED";
    const HELPER_EXPECT_FAILURE: &str = "JIG_GIT_INIT_AMBIENT_HELPER_EXPECT_FAILURE";
    const HELPER_TEST: &str = "bootstrap::git::tests::git_init_ambient_environment_helper";

    #[test]
    fn git_init_ambient_environment_helper() {
        let Some(destination) = std::env::var_os(HELPER_DESTINATION) else {
            return;
        };
        let destination = PathBuf::from(destination);
        if std::env::var_os(HELPER_EXPECT_EXISTING_GIT_ACCEPTED).is_some() {
            assert!(!init_git_repo(&destination, "main").unwrap());
            assert!(destination.join(".git").is_dir());
            return;
        }
        if let Some(expected) = std::env::var_os(HELPER_EXPECT_EXISTING_GIT_PRESERVED) {
            let error = init_git_repo(&destination, "main").unwrap_err();
            let expected = expected.to_string_lossy();
            assert!(
                format!("{error:#}").contains(expected.as_ref()),
                "unexpected existing-metadata failure: {error:#}"
            );
            assert!(fs::symlink_metadata(destination.join(".git")).is_ok());
            return;
        }
        if let Some(expected) = std::env::var_os(HELPER_EXPECT_FAILURE) {
            let error = init_git_repo(&destination, "main").unwrap_err();
            let expected = expected.to_string_lossy();
            assert!(
                format!("{error:#}").contains(expected.as_ref()),
                "unexpected injected failure: {error:#}"
            );
            assert!(!destination.join(".git").exists());
            return;
        }
        assert!(init_git_repo(&destination, "main").unwrap());
        assert!(!init_git_repo(&destination, "main").unwrap());
        validate_staged_git_repository(&destination, &destination.join(".git"), "git", "main")
            .unwrap();
    }

    #[test]
    fn git_init_accepts_a_valid_existing_git_directory() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        fs::create_dir(&destination).unwrap();
        create_existing_git_repository(&destination);

        assert!(!init_git_repo(&destination, "main").unwrap());
        assert!(destination.join(".git").is_dir());
        assert_no_private_git_staging(&destination);
    }

    #[test]
    fn git_init_accepts_a_valid_existing_regular_gitfile() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        let metadata = temp.path().join("separate-metadata");
        fs::create_dir(&destination).unwrap();
        create_existing_gitfile_repository(&destination, &metadata);

        assert!(!init_git_repo(&destination, "main").unwrap());
        assert!(
            fs::symlink_metadata(destination.join(".git"))
                .unwrap()
                .is_file()
        );
        assert!(metadata.is_dir());
        assert_no_private_git_staging(&destination);
    }

    #[test]
    fn git_init_rejects_a_gitfile_that_resolves_an_outer_worktree() {
        let temp = tempdir().unwrap();
        let outer = temp.path().join("outer-repository");
        let destination = outer.join("nested-destination");
        fs::create_dir(&outer).unwrap();
        create_existing_git_repository(&outer);
        fs::create_dir(&destination).unwrap();
        let mut config = Command::new("git");
        super::scrub_git_repository_environment(&mut config);
        let status = config
            .args(["config", "--file"])
            .arg(outer.join(".git/config"))
            .arg("core.worktree")
            .arg(&outer)
            .status()
            .unwrap();
        assert!(status.success());
        fs::write(
            destination.join(".git"),
            format!("gitdir: {}\n", outer.join(".git").display()),
        )
        .unwrap();

        let error = init_git_repo(&destination, "main").unwrap_err();

        let message = format!("{error:#}");
        assert!(message.contains("different work-tree root"), "{message}");
        assert!(fs::symlink_metadata(destination.join(".git")).is_ok());
        assert!(outer.join(".git/HEAD").is_file());
        assert_no_private_git_staging(&destination);
    }

    #[test]
    fn git_init_rejects_unusable_existing_git_files_and_directories() {
        for kind in ["garbage-file", "partial-directory"] {
            let temp = tempdir().unwrap();
            let destination = temp.path().join("repository");
            fs::create_dir(&destination).unwrap();
            match kind {
                "garbage-file" => fs::write(destination.join(".git"), b"not a gitfile\n").unwrap(),
                "partial-directory" => fs::create_dir(destination.join(".git")).unwrap(),
                _ => unreachable!(),
            }

            let error = init_git_repo(&destination, "main").unwrap_err();

            let message = format!("{error:#}");
            assert!(message.contains("usable"), "{kind}: {message}");
            assert!(fs::symlink_metadata(destination.join(".git")).is_ok());
            assert_no_private_git_staging(&destination);
        }
    }

    #[cfg(unix)]
    #[test]
    fn git_init_rejects_existing_git_symlinks_without_following_them() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let valid = temp.path().join("valid");
        fs::create_dir(&valid).unwrap();
        create_existing_git_repository(&valid);

        for target in [valid.join(".git"), temp.path().join("missing-git-target")] {
            let destination = temp.path().join(format!(
                "repository-{}",
                if target.exists() { "valid" } else { "dangling" }
            ));
            fs::create_dir(&destination).unwrap();
            symlink(&target, destination.join(".git")).unwrap();

            let error = init_git_repo(&destination, "main").unwrap_err();

            let message = format!("{error:#}");
            assert!(message.contains("not a symlink"), "{message}");
            assert!(fs::symlink_metadata(destination.join(".git")).is_ok());
            assert_no_private_git_staging(&destination);
        }
    }

    #[test]
    fn git_init_rejects_and_preserves_an_invalid_concurrent_git_winner() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        fs::create_dir(&destination).unwrap();
        let calls = Cell::new(0);

        let error = init_git_repo_with_validation(&destination, "main", || {
            let call = calls.get() + 1;
            calls.set(call);
            if call == 3 {
                fs::write(destination.join(".git"), b"concurrent invalid metadata\n").unwrap();
            }
            Ok(())
        })
        .unwrap_err();

        let message = format!("{error:#}");
        assert!(message.contains("usable"), "{message}");
        assert_eq!(
            fs::read(destination.join(".git")).unwrap(),
            b"concurrent invalid metadata\n"
        );
        assert_no_private_git_staging(&destination);
    }

    #[test]
    fn git_init_accepts_a_valid_concurrent_git_winner() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        fs::create_dir(&destination).unwrap();
        let calls = Cell::new(0);

        let initialized = init_git_repo_with_validation(&destination, "main", || {
            let call = calls.get() + 1;
            calls.set(call);
            if call == 3 {
                create_existing_git_repository(&destination);
            }
            Ok(())
        })
        .unwrap();

        assert!(!initialized);
        assert!(calls.get() >= 5);
        assert!(destination.join(".git").is_dir());
        assert_no_private_git_staging(&destination);
    }

    #[test]
    fn existing_git_validation_retains_identity_across_the_final_boundary() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        let displaced = temp.path().join("displaced-valid-git");
        fs::create_dir(&destination).unwrap();
        create_existing_git_repository(&destination);
        let calls = Cell::new(0);

        let error = init_git_repo_with_validation(&destination, "main", || {
            let call = calls.get() + 1;
            calls.set(call);
            if call == 2 {
                fs::rename(destination.join(".git"), &displaced).unwrap();
                fs::write(destination.join(".git"), b"foreign replacement\n").unwrap();
            }
            Ok(())
        })
        .unwrap_err();

        let message = format!("{error:#}");
        assert!(message.contains("changed concurrently"), "{message}");
        assert_eq!(
            fs::read(destination.join(".git")).unwrap(),
            b"foreign replacement\n"
        );
        assert!(displaced.join("HEAD").is_file());
    }

    #[test]
    fn git_metadata_transfer_rejects_entries_added_after_its_sorted_snapshot() {
        let temp = tempdir().unwrap();
        let source_root = super::private_tempdir_in(temp.path(), ".source-stage-").unwrap();
        let destination_root =
            super::private_tempdir_in(temp.path(), ".destination-stage-").unwrap();
        let source = source_root.path().join(".git");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("z-last"), b"last").unwrap();
        fs::write(source.join("a-first"), b"first").unwrap();
        let source_commit = super::super::path::repository_directory_commit_at(&source).unwrap();

        let error = super::move_directory_contents_with(
            &source,
            destination_root.path(),
            &source_root,
            &source_commit,
            &destination_root,
            || {
                fs::write(source.join("late-entry"), b"preserve me").unwrap();
                Ok(())
            },
        )
        .unwrap_err();

        let message = format!("{error:#}");
        assert!(
            message.contains("appeared after the transfer snapshot"),
            "{message}"
        );
        assert!(source_root.preserve_for_recovery.get());
        assert_eq!(fs::read(source.join("late-entry")).unwrap(), b"preserve me");
        assert_eq!(
            fs::read(destination_root.path().join("a-first")).unwrap(),
            b"first"
        );
        assert_eq!(
            fs::read(destination_root.path().join("z-last")).unwrap(),
            b"last"
        );
    }

    #[test]
    fn invalid_existing_git_cannot_be_authorized_by_an_ambient_repository_redirect() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        let external = temp.path().join("external");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(&external).unwrap();
        create_existing_git_repository(&external);
        fs::write(destination.join(".git"), b"invalid local metadata\n").unwrap();

        let output = git_init_helper_command(&destination)
            .env(HELPER_EXPECT_EXISTING_GIT_PRESERVED, "usable")
            .env("GIT_DIR", external.join(".git"))
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "existing Git validation helper failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read(destination.join(".git")).unwrap(),
            b"invalid local metadata\n"
        );
        assert!(external.join(".git/HEAD").is_file());
        assert_no_private_git_staging(&destination);
    }

    #[test]
    fn existing_git_validation_ignores_ambient_global_worktree_redirection() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        let external = temp.path().join("external-worktree");
        let global_config = temp.path().join("ambient.gitconfig");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(&external).unwrap();
        create_existing_git_repository(&destination);
        fs::write(
            &global_config,
            format!("[core]\n\tworktree = {}\n", external.display()),
        )
        .unwrap();

        let output = git_init_helper_command(&destination)
            .env(HELPER_EXPECT_EXISTING_GIT_ACCEPTED, "1")
            .env("GIT_CONFIG_GLOBAL", &global_config)
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "existing Git config-scrub helper failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(destination.join(".git/HEAD").is_file());
        assert_no_private_git_staging(&destination);
    }

    #[test]
    fn git_init_scrubs_ambient_git_dir_in_a_subprocess() {
        run_ambient_repository_redirect_regression("GIT_DIR");
    }

    #[test]
    fn git_init_scrubs_ambient_git_common_dir_in_a_subprocess() {
        run_ambient_repository_redirect_regression("GIT_COMMON_DIR");
    }

    #[test]
    fn git_init_scrubs_ambient_git_object_directory_in_a_subprocess() {
        run_ambient_repository_redirect_regression("GIT_OBJECT_DIRECTORY");
    }

    #[test]
    fn git_init_scrubs_command_scoped_config_injection_in_a_subprocess() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        let injected_template = temp.path().join("injected-template");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(&injected_template).unwrap();
        fs::write(
            injected_template.join("injected-template-marker"),
            b"must not be copied",
        )
        .unwrap();
        let config_file = temp.path().join("injected-config");
        fs::write(&config_file, b"[core]\n\tbare = true\n").unwrap();

        run_init_helper(
            &destination,
            [
                (OsString::from("GIT_CONFIG_COUNT"), OsString::from("2")),
                (
                    OsString::from("GIT_CONFIG_KEY_0"),
                    OsString::from("core.bare"),
                ),
                (OsString::from("GIT_CONFIG_VALUE_0"), OsString::from("true")),
                (
                    OsString::from("GIT_CONFIG_KEY_1"),
                    OsString::from("init.templateDir"),
                ),
                (
                    OsString::from("GIT_CONFIG_VALUE_1"),
                    injected_template.as_os_str().to_owned(),
                ),
                (
                    OsString::from("GIT_CONFIG_PARAMETERS"),
                    OsString::from("malformed-command-config"),
                ),
                (
                    OsString::from("GIT_CONFIG"),
                    config_file.as_os_str().to_owned(),
                ),
            ],
        );

        assert!(!destination.join(".git/injected-template-marker").exists());
        assert_no_private_git_staging(&destination);
    }

    #[test]
    fn git_init_cleanup_failure_cannot_follow_publication() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        fs::create_dir(&destination).unwrap();

        let output = git_init_helper_command(&destination)
            .env(
                HELPER_EXPECT_FAILURE,
                "injected git init staging cleanup failure",
            )
            .env("JIG_TEST_GIT_STAGING_CLOSE_FAILURE", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git init cleanup helper failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!destination.join(".git").exists());
        assert_no_private_git_staging(&destination);
    }

    #[test]
    fn git_init_preserves_a_safe_ambient_template_through_private_staging() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        let template = temp.path().join("template");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(&template).unwrap();
        fs::write(template.join("custom-marker"), b"safe template contents").unwrap();

        run_init_helper(
            &destination,
            [(
                OsString::from("GIT_TEMPLATE_DIR"),
                template.as_os_str().to_owned(),
            )],
        );

        assert_eq!(
            fs::read(destination.join(".git/custom-marker")).unwrap(),
            b"safe template contents"
        );
        assert_no_private_git_staging(&destination);
    }

    #[test]
    fn git_init_preserves_a_safe_template_from_global_config() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        let template = temp.path().join("template");
        let global_config = temp.path().join("global-config");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(&template).unwrap();
        fs::write(template.join("configured-marker"), b"safe config template").unwrap();
        let mut config_command = Command::new("git");
        super::scrub_git_repository_environment(&mut config_command);
        let status = config_command
            .args(["config", "--file"])
            .arg(&global_config)
            .arg("init.templateDir")
            .arg(&template)
            .status()
            .unwrap();
        assert!(status.success());
        let mut object_format_command = Command::new("git");
        super::scrub_git_repository_environment(&mut object_format_command);
        let status = object_format_command
            .args(["config", "--file"])
            .arg(&global_config)
            .arg("init.defaultObjectFormat")
            .arg("sha256")
            .status()
            .unwrap();
        assert!(status.success());

        run_init_helper(
            &destination,
            [(
                OsString::from("GIT_CONFIG_GLOBAL"),
                global_config.as_os_str().to_owned(),
            )],
        );

        assert_eq!(
            fs::read(destination.join(".git/configured-marker")).unwrap(),
            b"safe config template"
        );
        let mut object_format = Command::new("git");
        super::scrub_git_repository_environment(&mut object_format);
        let output = object_format
            .arg("-C")
            .arg(&destination)
            .args(["config", "--local", "--get", "extensions.objectFormat"])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "ambient global config leaked into mutating init: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert_no_private_git_staging(&destination);
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn git_init_preserves_a_non_utf8_template_path_from_global_config() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        let template = temp
            .path()
            .join(OsString::from_vec(b"template-\xff".to_vec()));
        let global_config = temp.path().join("global-config");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(&template).unwrap();
        fs::write(template.join("non-utf8-marker"), b"preserved").unwrap();
        let mut config = b"[init]\n\ttemplateDir = ".to_vec();
        config.extend_from_slice(template.as_os_str().as_bytes());
        config.push(b'\n');
        fs::write(&global_config, config).unwrap();

        run_init_helper(
            &destination,
            [
                (
                    OsString::from("GIT_CONFIG_GLOBAL"),
                    global_config.as_os_str().to_owned(),
                ),
                (
                    OsString::from("GIT_CONFIG_SYSTEM"),
                    OsString::from("/dev/null"),
                ),
            ],
        );

        assert_eq!(
            fs::read(destination.join(".git/non-utf8-marker")).unwrap(),
            b"preserved"
        );
        assert_no_private_git_staging(&destination);
    }

    #[cfg(unix)]
    #[test]
    fn configured_git_path_preserves_non_utf8_bytes() {
        use std::os::unix::ffi::OsStrExt;

        let path = super::git_path_from_bytes(b"template-\xff".to_vec()).unwrap();
        assert_eq!(path.as_bytes(), b"template-\xff");
    }

    #[cfg(unix)]
    #[test]
    fn git_init_rejects_a_template_symlink_before_git_can_mutate_its_target() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        let template = temp.path().join("template");
        let external = temp.path().join("external-config");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(&template).unwrap();
        fs::write(&external, b"external sentinel\n").unwrap();
        symlink(&external, template.join("config")).unwrap();

        run_failing_init_helper(
            &destination,
            [(
                OsString::from("GIT_TEMPLATE_DIR"),
                template.as_os_str().to_owned(),
            )],
            "symbolic link or special file",
        );

        assert_eq!(fs::read(&external).unwrap(), b"external sentinel\n");
        assert!(!destination.join(".git").exists());
        assert_no_private_git_staging(&destination);
    }

    #[test]
    fn git_init_revalidates_destination_at_each_mutation_boundary() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        fs::create_dir(&destination).unwrap();
        let calls = Cell::new(0);

        let error = init_git_repo_with_validation(&destination, "main", || {
            let call = calls.get() + 1;
            calls.set(call);
            #[cfg(unix)]
            if call > 1 {
                use std::os::unix::fs::PermissionsExt;

                let private = fs::read_dir(&destination)
                    .unwrap()
                    .map(|entry| entry.unwrap())
                    .filter(|entry| {
                        let name = entry.file_name();
                        let name = name.to_string_lossy();
                        name.starts_with(".jig-git-init-") || name.starts_with(".jig-git-metadata-")
                    })
                    .collect::<Vec<_>>();
                assert_eq!(private.len(), 1, "expected one private staging root");
                assert_eq!(
                    private[0].metadata().unwrap().permissions().mode() & 0o777,
                    0o700
                );
            }
            if call == 3 {
                return Err(anyhow::anyhow!("destination changed before publication"));
            }
            Ok(())
        })
        .unwrap_err();

        assert_eq!(calls.get(), 3);
        assert!(
            error
                .to_string()
                .contains("destination validation failed before .git publication")
        );
        assert!(!destination.join(".git").exists());
        assert_no_private_git_staging(&destination);
    }

    #[test]
    fn git_init_preserves_a_concurrently_replaced_worktree_staging_directory() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        fs::create_dir(&destination).unwrap();
        let calls = Cell::new(0);
        let replaced = RefCell::new(None);

        let error = init_git_repo_with_validation(&destination, "main", || {
            let call = calls.get() + 1;
            calls.set(call);
            if call == 2 {
                let staging = private_staging_path(&destination, ".jig-git-init-");
                let displaced = destination.join("displaced-owned-worktree-stage");
                fs::rename(&staging, &displaced).unwrap();
                fs::create_dir(&staging).unwrap();
                fs::write(staging.join("foreign-sentinel"), b"preserve me").unwrap();
                *replaced.borrow_mut() = Some((staging, displaced));
            }
            Ok(())
        })
        .unwrap_err();

        assert_eq!(calls.get(), 2);
        assert!(format!("{error:#}").contains("was replaced concurrently"));
        let (replacement, displaced) = replaced.into_inner().unwrap();
        assert_eq!(
            fs::read(replacement.join("foreign-sentinel")).unwrap(),
            b"preserve me"
        );
        assert!(displaced.join(".git").is_dir());
        assert!(!destination.join(".git").exists());
    }

    #[test]
    fn git_init_preserves_a_concurrently_replaced_staged_git_child() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        fs::create_dir(&destination).unwrap();
        let calls = Cell::new(0);
        let replaced = RefCell::new(None);

        let error = init_git_repo_with_validation(&destination, "main", || {
            let call = calls.get() + 1;
            calls.set(call);
            if call == 2 {
                let staging = private_staging_path(&destination, ".jig-git-init-");
                let staged_git = staging.join(".git");
                let displaced = destination.join("displaced-owned-git-metadata");
                fs::rename(&staged_git, &displaced).unwrap();
                fs::create_dir(&staged_git).unwrap();
                fs::write(staged_git.join("foreign-sentinel"), b"preserve me").unwrap();
                *replaced.borrow_mut() = Some((staging, displaced));
            }
            Ok(())
        })
        .unwrap_err();

        assert_eq!(calls.get(), 2);
        assert!(format!("{error:#}").contains("metadata directory"));
        assert!(format!("{error:#}").contains("preserving the staging tree"));
        let (staging, displaced) = replaced.into_inner().unwrap();
        assert_eq!(
            fs::read(staging.join(".git/foreign-sentinel")).unwrap(),
            b"preserve me"
        );
        assert!(displaced.join("HEAD").is_file());
        assert!(!destination.join(".git").exists());
    }

    #[test]
    fn git_init_preserves_a_concurrently_replaced_metadata_staging_directory() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        fs::create_dir(&destination).unwrap();
        let calls = Cell::new(0);
        let replaced = RefCell::new(None);

        let error = init_git_repo_with_validation(&destination, "main", || {
            let call = calls.get() + 1;
            calls.set(call);
            if call == 3 {
                let staging = private_staging_path(&destination, ".jig-git-metadata-");
                let displaced = destination.join("displaced-owned-metadata-stage");
                fs::rename(&staging, &displaced).unwrap();
                fs::create_dir(&staging).unwrap();
                fs::write(staging.join("foreign-sentinel"), b"preserve me").unwrap();
                *replaced.borrow_mut() = Some((staging, displaced));
            }
            Ok(())
        })
        .unwrap_err();

        assert_eq!(calls.get(), 3);
        assert!(format!("{error:#}").contains("was replaced concurrently"));
        let (replacement, displaced) = replaced.into_inner().unwrap();
        assert_eq!(
            fs::read(replacement.join("foreign-sentinel")).unwrap(),
            b"preserve me"
        );
        assert!(displaced.join("HEAD").is_file());
        assert!(!destination.join(".git").exists());
    }

    fn run_ambient_repository_redirect_regression(variable: &str) {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        let external = temp.path().join("external");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(&external).unwrap();
        fs::write(external.join("sentinel"), b"unchanged").unwrap();

        run_init_helper(
            &destination,
            [(OsString::from(variable), external.as_os_str().to_owned())],
        );

        assert_eq!(fs::read(external.join("sentinel")).unwrap(), b"unchanged");
        let entries = fs::read_dir(&external)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, [OsString::from("sentinel")]);
        assert_no_private_git_staging(&destination);
    }

    fn create_existing_git_repository(destination: &Path) {
        let mut command = Command::new("git");
        super::scrub_git_repository_environment(&mut command);
        let output = command
            .current_dir(destination)
            .env("GIT_CONFIG_GLOBAL", super::null_git_config_path())
            .env("GIT_CONFIG_SYSTEM", super::null_git_config_path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args(["-c", "init.templateDir=", "init"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "failed to prepare existing Git repository\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn create_existing_gitfile_repository(destination: &Path, metadata: &Path) {
        let mut command = Command::new("git");
        super::scrub_git_repository_environment(&mut command);
        let output = command
            .current_dir(destination.parent().unwrap())
            .env("GIT_CONFIG_GLOBAL", super::null_git_config_path())
            .env("GIT_CONFIG_SYSTEM", super::null_git_config_path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args(["-c", "init.templateDir=", "init", "--separate-git-dir"])
            .arg(metadata)
            .arg(destination)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "failed to prepare existing Git gitfile repository\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn run_init_helper<const N: usize>(destination: &Path, environment: [(OsString, OsString); N]) {
        let output = git_init_helper_command(destination)
            .envs(environment)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git init helper failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(destination.join(".git").is_dir());
    }

    #[cfg(unix)]
    fn run_failing_init_helper<const N: usize>(
        destination: &Path,
        environment: [(OsString, OsString); N],
        expected: &str,
    ) {
        let output = git_init_helper_command(destination)
            .env(HELPER_EXPECT_FAILURE, expected)
            .envs(environment)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "failing git init helper failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_init_helper_command(destination: &Path) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", HELPER_TEST, "--nocapture"])
            .env_remove(GIT_BIN_ENV)
            .env_remove("GIT_TEMPLATE_DIR")
            .env_remove("GIT_CONFIG_GLOBAL")
            .env_remove("GIT_CONFIG_SYSTEM")
            .env_remove("GIT_CONFIG_NOSYSTEM")
            .env_remove("GIT_CONFIG")
            .env_remove("GIT_CONFIG_COUNT")
            .env_remove("GIT_CONFIG_PARAMETERS")
            .env_remove(HELPER_EXPECT_EXISTING_GIT_PRESERVED)
            .env_remove(HELPER_EXPECT_FAILURE)
            .env(HELPER_DESTINATION, destination);
        command
    }

    fn assert_no_private_git_staging(destination: &Path) {
        let private = fs::read_dir(destination)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| {
                let name = name.to_string_lossy();
                name.starts_with(".jig-git-init-") || name.starts_with(".jig-git-metadata-")
            })
            .collect::<Vec<_>>();
        assert!(private.is_empty(), "private staging leaked: {private:?}");
    }

    fn private_staging_path(destination: &Path, prefix: &str) -> PathBuf {
        let matches = fs::read_dir(destination)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(prefix))
            })
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 1, "expected one {prefix} staging root");
        matches.into_iter().next().unwrap()
    }
}
