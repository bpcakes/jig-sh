use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use tempfile::TempDir;

use crate::context::RepoContext;
use crate::repository_path::normalize_repo_relative_path;
use crate::source_projection::{
    IGNORED_DOTENV_PATHSPECS, MAX_SUBMODULE_DEPTH, initialized_submodule_paths,
};

use super::{NativeToolOutput, controlled_git_text, controlled_output};

#[cfg(test)]
pub(super) fn check(ctx: &RepoContext) -> Result<NativeToolOutput> {
    check_with_control(ctx, Duration::from_secs(30 * 60), &|| false)
}

pub(super) fn check_with_control(
    ctx: &RepoContext,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeToolOutput> {
    let command_text = ctx.schema_dump_command();
    if command_text.trim().is_empty() {
        bail!("schema_dump_command is empty");
    }
    let configured_schema_docs_dir =
        env::var("SCHEMA_DOCS_DIR").unwrap_or_else(|_| "docs/schema".into());
    let schema_docs_dir =
        normalize_repo_relative_path(Path::new(&configured_schema_docs_dir), "SCHEMA_DOCS_DIR")?;
    let schema_docs_dir = schema_docs_dir
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("SCHEMA_DOCS_DIR must be valid UTF-8"))?;
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| anyhow::anyhow!("schema check timeout is too large"))?;

    let initial_status = schema_path_status(ctx.root(), schema_docs_dir, deadline, cancelled)?;
    if !initial_status.trim().is_empty() {
        return Ok(NativeToolOutput {
            exit_status: 1,
            stdout: String::new(),
            stderr: format!(
                "Schema output path {schema_docs_dir} already has uncommitted changes; preserve or commit them before checking generated schema drift.\n{initial_status}"
            ),
        });
    }

    let sandbox = SchemaSandbox::create(ctx.root(), deadline, cancelled)?;
    run_schema_drift_check(
        sandbox.root(),
        command_text,
        schema_docs_dir,
        deadline,
        cancelled,
    )
}

/// Disposable repository snapshot used to keep a freshness query physically
/// separate from the generator's writes.
struct SchemaSandbox {
    _temp: TempDir,
    root: PathBuf,
}

impl SchemaSandbox {
    fn create(
        repository_root: &Path,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        let temp = tempfile::tempdir().context("Failed to create schema-check sandbox")?;
        let root = temp.path().join("repository");
        clone_worktree_snapshot(repository_root, &root, deadline, cancelled, 0)?;
        Ok(Self { _temp: temp, root })
    }

    fn root(&self) -> &Path {
        &self.root
    }
}

/// Materializes the repository state that a local generator can observe without
/// letting the generator mutate the live checkout. Git supplies tracked and
/// staged content; explicit overlays supply state that no Git tree can encode.
fn clone_worktree_snapshot(
    repository_root: &Path,
    sandbox_root: &Path,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
    submodule_depth: usize,
) -> Result<()> {
    if submodule_depth > MAX_SUBMODULE_DEPTH {
        bail!("schema-check submodules exceed the supported nesting depth");
    }

    let snapshot = controlled_git_text(
        repository_root,
        &["stash", "create", "jig schema freshness snapshot"],
        deadline,
        cancelled,
    )?;
    let snapshot = if snapshot.trim().is_empty() {
        controlled_git_text(repository_root, &["rev-parse", "HEAD"], deadline, cancelled)?
    } else {
        snapshot
    };

    remove_snapshot_path(sandbox_root)?;
    let mut clone = Command::new("git");
    clone.args(["clone", "--quiet", "--no-checkout", "--shared", "--"]);
    clone.arg(repository_root).arg(sandbox_root);
    let output = controlled_output(&mut clone, deadline, cancelled)
        .context("Failed to clone schema-check sandbox")?;
    if !output.status.success() {
        bail!(
            "failed to clone schema-check sandbox with status {}\nstdout:\n{}\nstderr:\n{}",
            output.status.code().unwrap_or(1),
            output.stdout,
            output.stderr
        );
    }

    let mut checkout = Command::new("git");
    checkout
        .current_dir(sandbox_root)
        .args(["checkout", "--quiet", "--detach", snapshot.trim()]);
    let output = controlled_output(&mut checkout, deadline, cancelled)
        .context("Failed to check out schema-check snapshot")?;
    if !output.status.success() {
        bail!(
            "failed to check out schema-check snapshot with status {}\nstdout:\n{}\nstderr:\n{}",
            output.status.code().unwrap_or(1),
            output.stdout,
            output.stderr
        );
    }

    overlay_worktree_files(repository_root, sandbox_root, deadline, cancelled)?;
    for relative in initialized_submodules(repository_root, deadline, cancelled)? {
        clone_worktree_snapshot(
            &repository_root.join(&relative),
            &sandbox_root.join(&relative),
            deadline,
            cancelled,
            submodule_depth + 1,
        )?;
    }
    Ok(())
}

fn overlay_worktree_files(
    repository_root: &Path,
    sandbox_root: &Path,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    let untracked = controlled_git_text(
        repository_root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        deadline,
        cancelled,
    )?;
    let mut ignored_dotenv_args = vec![
        "ls-files",
        "--others",
        "--ignored",
        "--exclude-standard",
        "-z",
        "--",
    ];
    ignored_dotenv_args.extend_from_slice(IGNORED_DOTENV_PATHSPECS);
    let ignored_dotenv =
        controlled_git_text(repository_root, &ignored_dotenv_args, deadline, cancelled)?;

    let mut paths = untracked
        .split('\0')
        .chain(ignored_dotenv.split('\0'))
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    paths.sort_unstable();
    paths.dedup();

    for relative in paths {
        let relative = normalize_repo_relative_path(Path::new(relative), "untracked path")?;
        let source = repository_root.join(&relative);
        let metadata = fs::symlink_metadata(&source)
            .with_context(|| format!("Failed to inspect untracked file {}", source.display()))?;
        let destination = sandbox_root.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        if metadata.file_type().is_file() {
            fs::copy(&source, &destination).with_context(|| {
                format!(
                    "Failed to copy untracked file {} into the schema-check sandbox",
                    relative.display()
                )
            })?;
        } else if metadata.file_type().is_symlink() {
            copy_symlink(&source, &destination)?;
        }
    }
    Ok(())
}

fn initialized_submodules(
    repository_root: &Path,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<PathBuf>> {
    if !repository_root.join(".gitmodules").is_file() {
        return Ok(Vec::new());
    }
    let mut command = Command::new("git");
    command.current_dir(repository_root).args([
        "config",
        "-z",
        "--file",
        ".gitmodules",
        "--get-regexp",
        "^submodule\\..*\\.path$",
    ]);
    let output = controlled_output(&mut command, deadline, cancelled)
        .context("Failed to inspect schema-check submodules")?;
    if !output.status.success() {
        if output.status.code() == Some(1) {
            return Ok(Vec::new());
        }
        bail!(
            "failed to inspect schema-check submodules with status {}\nstderr:\n{}",
            output.status.code().unwrap_or(1),
            output.stderr
        );
    }

    initialized_submodule_paths(repository_root, output.stdout.as_bytes())
}

fn remove_snapshot_path(path: &Path) -> Result<()> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .with_context(|| {
        format!(
            "Failed to replace schema-check snapshot path {}",
            path.display()
        )
    })
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path) -> Result<()> {
    let target = fs::read_link(source)
        .with_context(|| format!("Failed to read untracked symlink {}", source.display()))?;
    std::os::unix::fs::symlink(&target, destination).with_context(|| {
        format!(
            "Failed to copy untracked symlink {} into the schema-check sandbox",
            source.display()
        )
    })
}

#[cfg(windows)]
fn copy_symlink(source: &Path, destination: &Path) -> Result<()> {
    let target = fs::read_link(source)
        .with_context(|| format!("Failed to read untracked symlink {}", source.display()))?;
    let result = if source.metadata().is_ok_and(|metadata| metadata.is_dir()) {
        std::os::windows::fs::symlink_dir(&target, destination)
    } else {
        std::os::windows::fs::symlink_file(&target, destination)
    };
    result.with_context(|| {
        format!(
            "Failed to copy untracked symlink {} into the schema-check sandbox",
            source.display()
        )
    })
}

fn run_schema_drift_check(
    sandbox_root: &Path,
    command_text: &str,
    schema_docs_dir: &str,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeToolOutput> {
    let mut dump = Command::new("bash");
    dump.current_dir(sandbox_root)
        .env("JIG_REPO_ROOT", sandbox_root)
        .arg("-c")
        .arg(command_text);
    let output = controlled_output(&mut dump, deadline, cancelled)
        .context("Failed to run schema_dump_command")?;
    if !output.status.success() {
        bail!(
            "schema_dump_command failed with status {}\nstdout:\n{}\nstderr:\n{}",
            output.status.code().unwrap_or(1),
            output.stdout,
            output.stderr
        );
    }
    let status = schema_path_status(sandbox_root, schema_docs_dir, deadline, cancelled)?;
    if !status.trim().is_empty() {
        let diff = controlled_git_text(
            sandbox_root,
            &["--no-pager", "diff", "HEAD", "--", schema_docs_dir],
            deadline,
            cancelled,
        )?;
        return Ok(NativeToolOutput {
            exit_status: 1,
            stdout: String::new(),
            stderr: format!(
                "Schema dump is stale. Re-run {command_text} and commit {schema_docs_dir} changes.\n{status}{diff}"
            ),
        });
    }
    Ok(NativeToolOutput {
        exit_status: 0,
        stdout: "Schema dump is up to date.\n".into(),
        stderr: String::new(),
    })
}

fn schema_path_status(
    root: &Path,
    schema_docs_dir: &str,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    controlled_git_text(
        root,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--",
            schema_docs_dir,
        ],
        deadline,
        cancelled,
    )
}
