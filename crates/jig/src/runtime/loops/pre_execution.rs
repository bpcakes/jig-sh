use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};

use crate::bootstrap::{GIT_BIN_ENV, external_program, scrub_known_repository_git_environment};
use crate::context::RepoContext;
use crate::execution::{ExecutionControl, run_authoritative_execution_command};

use super::state::LOOP_RUNTIME_DIR;

pub(super) fn require_ignored_loop_runtime_root(
    ctx: &RepoContext,
    observer: &mut dyn ExecutionControl,
) -> Result<()> {
    if !ctx.root().join(".git").try_exists().with_context(|| {
        format!(
            "Failed to inspect Git metadata entry at {}",
            ctx.root().display()
        )
    })? {
        return Ok(());
    }
    require_ignored_runtime_path(
        ctx,
        Path::new(LOOP_RUNTIME_DIR),
        "Loop runtime root",
        "loop",
        observer,
    )
}

pub(super) fn require_ignored_runtime_path(
    ctx: &RepoContext,
    path: &Path,
    description: &str,
    checkout: &str,
    observer: &mut dyn ExecutionControl,
) -> Result<()> {
    let args = [
        OsString::from("check-ignore"),
        OsString::from("--quiet"),
        OsString::from("--"),
        path.as_os_str().to_os_string(),
    ];
    let mut command = Command::new(external_program(GIT_BIN_ENV, "git"));
    command
        .current_dir(ctx.root())
        .arg("--no-replace-objects")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    scrub_known_repository_git_environment(&mut command);
    let output = run_authoritative_execution_command(
        &mut command,
        ctx.command_timeout(),
        crate::execution::internal_execution_output_limit(),
        "loop git check-ignore",
        observer,
    )
    .map_err(|error| error.into_anyhow())?;
    match output.status.code() {
        Some(0) => Ok(()),
        Some(1) => bail!(
            "{description} is not ignored by Git: {}; refresh the managed .gitignore with `scripts/jig update --recopy` before using {checkout}",
            path.display()
        ),
        _ => Err(anyhow!(
            "Failed to verify that the {description} is ignored with status {}. stdout: {} stderr: {}",
            output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".into()),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )),
    }
}
