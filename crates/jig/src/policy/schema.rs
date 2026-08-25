use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use jig_contract::TargetId;
use tempfile::TempDir;

use crate::context::{CommandOutputLimit, RepoContext};
use crate::execution::{
    ExecutionCancellation, ExecutionObserver, SupervisedExecutionError,
    run_supervised_execution_command,
};
use crate::repository_path::normalize_repo_relative_path;
use crate::source_projection::{
    IGNORED_DOTENV_PATHSPECS, MAX_SUBMODULE_DEPTH, initialized_submodule_paths,
};

use super::{
    NativeToolOutput, controlled_git_bytes, controlled_git_output, controlled_git_text,
    controlled_output,
};

mod runner;
use runner::SchemaDumpRunner;

const SCHEMA_SNAPSHOT_GIT_NAME: &str = "user.name=Jig Schema Snapshot";
const SCHEMA_SNAPSHOT_GIT_EMAIL: &str = "user.email=jig-schema-snapshot@example.invalid";

pub(super) fn validate_runner(ctx: &RepoContext, schema_check_target: &TargetId) -> Result<()> {
    runner::resolve(ctx, Some(schema_check_target)).map(|_| ())
}

pub(super) fn check_with_control(
    ctx: &RepoContext,
    schema_check_target: Option<&TargetId>,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeToolOutput> {
    let runner = runner::resolve(ctx, schema_check_target)?;
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
        &runner,
        schema_docs_dir,
        ctx.command_output_limit(),
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

    let head = repository_head(repository_root, deadline, cancelled)?;
    let unborn = head.is_none();
    if let Some(head) = head.as_deref() {
        clone_committed_snapshot(repository_root, sandbox_root, head, deadline, cancelled)?;
        overlay_worktree_files(repository_root, sandbox_root, false, deadline, cancelled)?;
    } else {
        initialize_unborn_snapshot(sandbox_root, deadline, cancelled)?;
        overlay_worktree_files(repository_root, sandbox_root, true, deadline, cancelled)?;
    }

    for relative in initialized_submodules(repository_root, deadline, cancelled)? {
        clone_worktree_snapshot(
            &repository_root.join(&relative),
            &sandbox_root.join(&relative),
            deadline,
            cancelled,
            submodule_depth + 1,
        )?;
    }
    if unborn {
        commit_unborn_snapshot(sandbox_root, deadline, cancelled)?;
    }
    Ok(())
}

fn repository_head(
    repository_root: &Path,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<String>> {
    let output = controlled_git_output(
        repository_root,
        &["rev-parse", "--verify", "--quiet", "HEAD"],
        deadline,
        cancelled,
    )?;
    if output.status.success() {
        let stdout = std::str::from_utf8(&output.stdout)
            .context("Git returned a non-UTF-8 schema-check repository HEAD")?;
        return Ok(Some(stdout.trim().to_owned()));
    }
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    bail!(
        "failed to inspect schema-check repository HEAD with status {}\nstderr:\n{}",
        output.status.code().unwrap_or(1),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn clone_committed_snapshot(
    repository_root: &Path,
    sandbox_root: &Path,
    head: &str,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    // `stash create` writes an unreferenced snapshot commit into the live
    // object database without changing refs, the index, or the worktree. It
    // intentionally excludes untracked files; `overlay_worktree_files` below
    // is therefore part of the snapshot contract, not an optional supplement.
    let snapshot = controlled_git_text(
        repository_root,
        &[
            "-c",
            SCHEMA_SNAPSHOT_GIT_NAME,
            "-c",
            SCHEMA_SNAPSHOT_GIT_EMAIL,
            "stash",
            "create",
            "jig schema freshness snapshot",
        ],
        deadline,
        cancelled,
    )?;
    let snapshot = if snapshot.trim().is_empty() {
        head
    } else {
        snapshot.trim()
    };

    remove_snapshot_path(sandbox_root)?;
    let mut clone = Command::new("git");
    clone.args(["clone", "--quiet", "--no-checkout", "--shared", "--"]);
    clone.arg(repository_root).arg(sandbox_root);
    crate::bootstrap::scrub_known_repository_git_environment(&mut clone);
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
        .args(["checkout", "--quiet", "--detach", snapshot]);
    crate::bootstrap::scrub_known_repository_git_environment(&mut checkout);
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

    Ok(())
}

fn initialize_unborn_snapshot(
    sandbox_root: &Path,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    remove_snapshot_path(sandbox_root)?;
    fs::create_dir_all(sandbox_root).with_context(|| {
        format!(
            "Failed to create unborn schema-check snapshot {}",
            sandbox_root.display()
        )
    })?;
    run_schema_git(
        sandbox_root,
        &["init", "--quiet"],
        deadline,
        cancelled,
        "initialize unborn schema-check snapshot",
    )
}

fn commit_unborn_snapshot(
    sandbox_root: &Path,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    run_schema_git(
        sandbox_root,
        &["add", "--all"],
        deadline,
        cancelled,
        "stage unborn schema-check snapshot",
    )?;
    run_schema_git(
        sandbox_root,
        &[
            "-c",
            SCHEMA_SNAPSHOT_GIT_NAME,
            "-c",
            SCHEMA_SNAPSHOT_GIT_EMAIL,
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "Jig schema snapshot baseline",
        ],
        deadline,
        cancelled,
        "commit unborn schema-check snapshot",
    )
}

fn run_schema_git(
    root: &Path,
    args: &[&str],
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
    context: &str,
) -> Result<()> {
    controlled_git_text(root, args, deadline, cancelled)
        .with_context(|| format!("Failed to {context}"))?;
    Ok(())
}

fn overlay_worktree_files(
    repository_root: &Path,
    sandbox_root: &Path,
    include_cached: bool,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    let mut file_args = vec!["ls-files"];
    if include_cached {
        file_args.push("--cached");
    }
    file_args.extend(["--others", "--exclude-standard", "-z"]);
    let worktree_files = controlled_git_bytes(repository_root, &file_args, deadline, cancelled)?;
    let mut ignored_dotenv_args = vec![
        "ls-files",
        "--others",
        "--ignored",
        "--exclude-standard",
        "--directory",
        "-z",
        "--",
    ];
    ignored_dotenv_args.extend_from_slice(IGNORED_DOTENV_PATHSPECS);
    let ignored_dotenv =
        controlled_git_bytes(repository_root, &ignored_dotenv_args, deadline, cancelled)?;

    let mut paths = worktree_files
        .split(|byte| *byte == 0)
        .chain(ignored_dotenv.split(|byte| *byte == 0))
        .filter(|path| !path.is_empty() && path.last() != Some(&b'/'))
        .collect::<Vec<_>>();
    paths.sort_unstable();
    paths.dedup();

    for relative in paths {
        let relative = git_path_from_bytes(relative)?;
        let relative = normalize_repo_relative_path(&relative, "untracked path")?;
        let source = repository_root.join(&relative);
        let metadata = match fs::symlink_metadata(&source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("Failed to inspect snapshot file {}", source.display())
                });
            }
        };
        let destination = sandbox_root.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        if metadata.file_type().is_file() {
            fs::copy(&source, &destination).with_context(|| {
                format!(
                    "Failed to copy snapshot file {} into the schema-check sandbox",
                    relative.display()
                )
            })?;
        } else if metadata.file_type().is_symlink() {
            copy_symlink(&source, &destination)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn git_path_from_bytes(bytes: &[u8]) -> Result<PathBuf> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    Ok(PathBuf::from(OsStr::from_bytes(bytes)))
}

#[cfg(not(unix))]
fn git_path_from_bytes(bytes: &[u8]) -> Result<PathBuf> {
    let path = std::str::from_utf8(bytes).context("Git returned a non-UTF-8 worktree path")?;
    Ok(PathBuf::from(path))
}

fn initialized_submodules(
    repository_root: &Path,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<PathBuf>> {
    if !repository_root.join(".gitmodules").is_file() {
        return Ok(Vec::new());
    }
    let output = controlled_git_output(
        repository_root,
        &[
            "config",
            "-z",
            "--file",
            ".gitmodules",
            "--get-regexp",
            "^submodule\\..*\\.path$",
        ],
        deadline,
        cancelled,
    )
    .context("Failed to inspect schema-check submodules")?;
    if !output.status.success() {
        if output.status.code() == Some(1) {
            return Ok(Vec::new());
        }
        bail!(
            "failed to inspect schema-check submodules with status {}\nstderr:\n{}",
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    initialized_submodule_paths(repository_root, &output.stdout)
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

#[cfg(not(unix))]
fn copy_symlink(source: &Path, destination: &Path) -> Result<()> {
    bail!(
        "Schema-check snapshots cannot copy untracked symlink {} to {} on this platform",
        source.display(),
        destination.display()
    )
}

fn run_schema_drift_check(
    sandbox_root: &Path,
    runner: &SchemaDumpRunner<'_>,
    schema_docs_dir: &str,
    output_limit: CommandOutputLimit,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeToolOutput> {
    let working_directory = crate::repository_path::resolve_repository_working_directory(
        sandbox_root,
        runner.working_directory,
    )?;
    let mut dump = Command::new("bash");
    dump.current_dir(working_directory)
        .envs(
            runner
                .environment
                .into_iter()
                .flat_map(|values| values.iter()),
        )
        .env("JIG_REPO_ROOT", sandbox_root)
        .arg("-c")
        .arg(runner.command_text);
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(jig_owned_process::OwnedProcessTreeError::TimedOut.into());
    }
    let mut control = SchemaCommandControl(cancelled);
    let output = match run_supervised_execution_command(
        &mut dump,
        remaining,
        output_limit,
        runner.command_key,
        &mut control,
    ) {
        Ok(output) => output,
        Err(SupervisedExecutionError::CancelledBeforeStart) => {
            return Err(jig_owned_process::OwnedProcessTreeError::CancelledBeforeStart.into());
        }
        Err(SupervisedExecutionError::Cancelled) => {
            return Err(jig_owned_process::OwnedProcessTreeError::Cancelled.into());
        }
        Err(SupervisedExecutionError::TimedOut) => {
            return Err(jig_owned_process::OwnedProcessTreeError::TimedOut.into());
        }
        Err(SupervisedExecutionError::OutputLimitExceeded {
            stream,
            stdout,
            stderr,
        }) => {
            let mut stderr = String::from_utf8_lossy(&stderr).into_owned();
            if !stderr.is_empty() && !stderr.ends_with('\n') {
                stderr.push('\n');
            }
            stderr.push_str(&format!(
                "{} exceeded the {} byte {stream} capture limit",
                runner.command_key,
                output_limit.bytes()
            ));
            return Ok(NativeToolOutput {
                exit_status: 1,
                stdout: String::from_utf8_lossy(&stdout).into_owned(),
                stderr,
            });
        }
        Err(SupervisedExecutionError::Failed { error, .. }) => {
            return Err(error).with_context(|| format!("Failed to run {}", runner.command_key));
        }
    };
    if !output.status.success() {
        return Ok(NativeToolOutput {
            exit_status: output.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    let status = schema_path_status(sandbox_root, schema_docs_dir, deadline, cancelled)?;
    if !status.trim().is_empty() {
        let schema_pathspec = literal_git_pathspec(schema_docs_dir);
        let diff = controlled_git_text(
            sandbox_root,
            &["--no-pager", "diff", "HEAD", "--", schema_pathspec.as_str()],
            deadline,
            cancelled,
        )?;
        return Ok(NativeToolOutput {
            exit_status: 1,
            stdout: String::new(),
            stderr: format!(
                "Schema dump is stale. Re-run {} and commit {schema_docs_dir} changes.\n{status}{diff}",
                runner.command_text
            ),
        });
    }
    Ok(NativeToolOutput {
        exit_status: 0,
        stdout: "Schema dump is up to date.\n".into(),
        stderr: String::new(),
    })
}

struct SchemaCommandControl<'a>(&'a dyn Fn() -> bool);

impl ExecutionObserver for SchemaCommandControl<'_> {}

impl ExecutionCancellation for SchemaCommandControl<'_> {
    fn cancelled(&self) -> bool {
        (self.0)()
    }
}

fn schema_path_status(
    root: &Path,
    schema_docs_dir: &str,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    let schema_pathspec = literal_git_pathspec(schema_docs_dir);
    controlled_git_text(
        root,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--",
            schema_pathspec.as_str(),
        ],
        deadline,
        cancelled,
    )
}

fn literal_git_pathspec(path: &str) -> String {
    format!(":(literal){path}")
}

#[cfg(test)]
mod pathspec_tests {
    use super::*;

    #[test]
    fn schema_status_treats_pathspec_magic_as_a_literal_directory_name() {
        let temp = tempfile::tempdir().unwrap();
        let schema_dir = ":(exclude)docs/schema";
        fs::create_dir_all(temp.path().join(schema_dir)).unwrap();
        fs::write(temp.path().join(schema_dir).join("schema.json"), "{}\n").unwrap();
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(temp.path())
            .status()
            .unwrap();
        assert!(status.success());

        let status = schema_path_status(
            temp.path(),
            schema_dir,
            Instant::now() + Duration::from_secs(5),
            &|| false,
        )
        .unwrap();

        assert!(status.contains("schema.json"), "{status}");
    }
}
