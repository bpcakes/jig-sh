//! Jig-owned Git helpers for repository policy checks.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use jig_owned_process::ProcessOutputLimits;

use super::{ControlledBytesOutput, controlled_output_bytes_with_limits};

// Git output is sometimes repository authority rather than user-facing
// diagnostics (for example an unborn schema snapshot's complete file list).
// Keep that capture bounded, but large enough for ordinary repositories and
// fail closed below if the bound is ever exceeded.
const CONTROLLED_GIT_OUTPUT_LIMIT: usize = 64 * 1024 * 1024;

pub(super) fn controlled_git_text(
    root: &Path,
    args: &[&str],
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    let output = controlled_git_output(root, args, deadline, cancelled)?;
    if !output.status.success() {
        bail!(
            "git {} failed with status {}\nstderr:\n{}",
            args.join(" "),
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("git {} returned non-UTF-8 text", args.join(" ")))
}

pub(super) fn controlled_git_bytes(
    root: &Path,
    args: &[&str],
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>> {
    let output = controlled_git_output(root, args, deadline, cancelled)?;
    if !output.status.success() {
        bail!(
            "git {} failed with status {}\nstderr:\n{}",
            args.join(" "),
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output.stdout)
}

pub(super) fn controlled_git_output(
    root: &Path,
    args: &[&str],
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<ControlledBytesOutput> {
    let mut command = Command::new("git");
    command.current_dir(root).args(args);
    crate::bootstrap::scrub_known_repository_git_environment(&mut command);
    let output = controlled_output_bytes_with_limits(
        &mut command,
        deadline,
        ProcessOutputLimits {
            stdout: CONTROLLED_GIT_OUTPUT_LIMIT,
            stderr: CONTROLLED_GIT_OUTPUT_LIMIT,
        },
        cancelled,
    )?;
    if output.stdout_truncated || output.stderr_truncated {
        bail!(
            "git {} output exceeded the {} byte schema-check capture limit",
            args.join(" "),
            CONTROLLED_GIT_OUTPUT_LIMIT
        );
    }
    Ok(output)
}

pub(super) fn git_list_files(root: &Path, roots: &[String]) -> Result<Vec<String>> {
    let mut args = vec!["ls-files", "-z", "--"];
    args.extend(roots.iter().map(String::as_str));
    Ok(split_nul(&git_output(root, &args)?))
}

pub(super) fn git_success(root: &Path, args: &[&str]) -> Result<bool> {
    Ok(Command::new("git")
        .current_dir(root)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success())
}

pub(super) fn git_output(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    if !output.status.success() {
        bail!(
            "git {} failed with status {}\nstderr:\n{}",
            args.join(" "),
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output.stdout)
}

pub(super) fn split_nul(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect()
}
