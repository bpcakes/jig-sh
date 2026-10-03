//! Jig-owned Git helpers for repository policy checks.
//!
//! Every Git process starts from `git_command`, which withholds the reserved
//! vault passphrase variables: repository policy code also runs during
//! launcher validation, before a `vault` command captures and clears its
//! passphrase, and repository configuration can make Git start other programs.

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

fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(root);
    crate::runtime::withhold_vault_passphrase(&mut command);
    command
}

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
    let mut command = git_command(root);
    command.args(args);
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
    Ok(git_command(root)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success())
}

pub(super) fn git_output(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = git_command(root).args(args).output()?;
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

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use jig_vault::{VAULT_NEW_PASSPHRASE_ENV, VAULT_PASSPHRASE_ENV};

    use super::*;

    #[test]
    fn policy_git_commands_withhold_reserved_passphrase_variables() {
        let command = git_command(Path::new("."));
        let removed = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        for name in [VAULT_PASSPHRASE_ENV, VAULT_NEW_PASSPHRASE_ENV] {
            assert!(removed.contains(&OsStr::new(name)), "{name}");
        }
    }
}
