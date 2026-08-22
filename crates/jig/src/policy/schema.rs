use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use tempfile::TempDir;

use crate::context::RepoContext;

use super::{
    NativeToolOutput, controlled_git_text, controlled_output, normalize_repo_relative_path,
};

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
        let temp = tempfile::tempdir().context("Failed to create schema-check sandbox")?;
        let root = temp.path().join("repository");

        let mut clone = Command::new("git");
        clone.args(["clone", "--quiet", "--no-checkout", "--shared", "--"]);
        clone.arg(repository_root).arg(&root);
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
            .current_dir(&root)
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

        copy_untracked_files(repository_root, &root, deadline, cancelled)?;
        Ok(Self { _temp: temp, root })
    }

    fn root(&self) -> &Path {
        &self.root
    }
}

fn copy_untracked_files(
    repository_root: &Path,
    sandbox_root: &Path,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    let paths = controlled_git_text(
        repository_root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        deadline,
        cancelled,
    )?;
    for relative in paths.split('\0').filter(|path| !path.is_empty()) {
        let relative = normalize_repo_relative_path(Path::new(relative), "untracked path")?;
        let source = repository_root.join(&relative);
        let metadata = fs::symlink_metadata(&source)
            .with_context(|| format!("Failed to inspect untracked file {}", source.display()))?;
        if !metadata.file_type().is_file() {
            bail!(
                "schema check cannot snapshot non-file untracked path {}",
                relative.display()
            );
        }
        let destination = sandbox_root.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        fs::copy(&source, &destination).with_context(|| {
            format!(
                "Failed to copy untracked file {} into the schema-check sandbox",
                relative.display()
            )
        })?;
    }
    Ok(())
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
