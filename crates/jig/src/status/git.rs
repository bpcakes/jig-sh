use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
use std::{process::Stdio, time::Duration};

use jig_contract::status_provider::v1::Input;
use jig_owned_process::format_exit_status;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
use jig_owned_process::{ProcessOutputLimits, run_owned_process_tree_with_output_limits};
use serde::Serialize;

use super::sanitize_observer_environment;

const GIT_STDOUT_LIMIT: usize = 8 * 1024 * 1024;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
const GIT_STDERR_LIMIT: usize = 64 * 1024;

#[derive(Debug)]
pub(super) enum GitProbeError {
    Cancelled,
    Failed(String),
}

#[derive(Serialize)]
pub(super) struct InputFreshness {
    name: String,
    kind: String,
    path: Option<String>,
    expected_revision: Option<String>,
    pub(super) observed_revision: Option<String>,
    dirty: Option<bool>,
    pub(super) status: &'static str,
    reason: Option<String>,
}

#[cfg(test)]
pub(super) fn input_freshness(
    root: &Path,
    input: &Input,
    observations: &mut BTreeMap<String, GitCheckoutObservation>,
) -> InputFreshness {
    input_freshness_with_cancellation(root, input, observations, &|| false)
        .expect("an always-false cancellation callback cannot cancel input freshness")
}

pub(super) fn input_freshness_with_cancellation(
    root: &Path,
    input: &Input,
    observations: &mut BTreeMap<String, GitCheckoutObservation>,
    cancelled: &dyn Fn() -> bool,
) -> Result<InputFreshness, GitProbeError> {
    if cancelled() {
        return Err(GitProbeError::Cancelled);
    }
    if input.kind != "git" {
        return Ok(InputFreshness {
            name: input.name.clone(),
            kind: input.kind.clone(),
            path: input.path.clone(),
            expected_revision: input.revision.clone(),
            observed_revision: None,
            dirty: None,
            status: "not_applicable",
            reason: Some("Jig compares revision freshness only for git inputs".into()),
        });
    }

    let key = input.path.clone().unwrap_or_else(|| ".".into());
    if !observations.contains_key(&key) {
        let observation = observe_git_checkout_with_cancellation(&root.join(&key), cancelled)?;
        observations.insert(key.clone(), observation);
    }
    let observation = observations
        .get(&key)
        .expect("the git observation was inserted above");
    let status = if !observation.errors.is_empty() || observation.revision.is_none() {
        "unavailable"
    } else if input.revision.is_none() {
        "unknown"
    } else if input.revision.as_ref() != observation.revision.as_ref() {
        "stale"
    } else if observation.dirty == Some(true) {
        "dirty"
    } else if observation.dirty == Some(false) {
        "current"
    } else {
        "unknown"
    };
    Ok(InputFreshness {
        name: input.name.clone(),
        kind: input.kind.clone(),
        path: input.path.clone(),
        expected_revision: input.revision.clone(),
        observed_revision: observation.revision.clone(),
        dirty: observation.dirty,
        status,
        reason: (!observation.errors.is_empty()).then(|| observation.errors.join("; ")),
    })
}

#[derive(Clone)]
pub(super) struct GitCheckoutObservation {
    pub(super) revision: Option<String>,
    pub(super) dirty: Option<bool>,
    pub(super) errors: Vec<String>,
}

pub(super) fn observe_git_checkout_with_cancellation(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<GitCheckoutObservation, GitProbeError> {
    let mut errors = Vec::new();
    let revision =
        match git_text_with_cancellation(path, &["rev-parse", "--verify", "HEAD"], cancelled) {
            Ok(revision) => Some(revision),
            Err(GitProbeError::Failed(error)) => {
                errors.push(error);
                None
            }
            Err(GitProbeError::Cancelled) => return Err(GitProbeError::Cancelled),
        };
    let dirty = match git_output_with_cancellation(
        path,
        &["status", "--porcelain=v1", "-z", "--untracked-files=normal"],
        cancelled,
    ) {
        Ok(output) => Some(!output.stdout.is_empty()),
        Err(GitProbeError::Failed(error)) => {
            errors.push(error);
            None
        }
        Err(GitProbeError::Cancelled) => return Err(GitProbeError::Cancelled),
    };
    Ok(GitCheckoutObservation {
        revision,
        dirty,
        errors,
    })
}

pub(super) fn git_text_with_cancellation(
    path: &Path,
    args: &[&str],
    cancelled: &dyn Fn() -> bool,
) -> Result<String, GitProbeError> {
    let output = git_output_with_cancellation(path, args, cancelled)?;
    if output.stdout_truncated {
        return Err(GitProbeError::Failed(format!(
            "git {} output exceeded the {} byte limit",
            args.join(" "),
            GIT_STDOUT_LIMIT
        )));
    }
    let text = std::str::from_utf8(&output.stdout).map_err(|error| {
        GitProbeError::Failed(format!(
            "git {} returned non-UTF-8 output: {error}",
            args.join(" ")
        ))
    })?;
    Ok(text.trim().to_string())
}

struct GitOutput {
    stdout: Vec<u8>,
    stdout_truncated: bool,
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn git_output_with_cancellation(
    path: &Path,
    args: &[&str],
    cancelled: &dyn Fn() -> bool,
) -> Result<GitOutput, GitProbeError> {
    let mut command = Command::new("git");
    command
        .current_dir(path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_git_environment(&mut command);
    let output = run_owned_process_tree_with_output_limits(
        &mut command,
        Duration::MAX,
        ProcessOutputLimits {
            stdout: GIT_STDOUT_LIMIT,
            stderr: GIT_STDERR_LIMIT,
        },
        cancelled,
    )
    .map_err(|error| {
        if error.is_cancellation() {
            GitProbeError::Cancelled
        } else {
            GitProbeError::Failed(format!(
                "Failed to run git {} in {}: {error}",
                args.join(" "),
                path.display()
            ))
        }
    })?;
    let stdout = output.stdout.ok_or_else(|| {
        GitProbeError::Failed(format!(
            "git {} did not return captured stdout in {}",
            args.join(" "),
            path.display()
        ))
    })?;
    let stderr = output.stderr.ok_or_else(|| {
        GitProbeError::Failed(format!(
            "git {} did not return captured stderr in {}",
            args.join(" "),
            path.display()
        ))
    })?;
    if !stdout.complete || !stderr.complete {
        return Err(GitProbeError::Failed(format!(
            "git {} output capture did not complete in {}",
            args.join(" "),
            path.display()
        )));
    }
    if output.status.success() {
        Ok(GitOutput {
            stdout: stdout.bytes,
            stdout_truncated: stdout.truncated,
        })
    } else {
        let stderr = String::from_utf8_lossy(&stderr.bytes);
        Err(GitProbeError::Failed(format!(
            "git {} failed with {} in {}: {}",
            args.join(" "),
            format_exit_status(&output.status),
            path.display(),
            stderr.trim()
        )))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn git_output_with_cancellation(
    path: &Path,
    args: &[&str],
    cancelled: &dyn Fn() -> bool,
) -> Result<GitOutput, GitProbeError> {
    if cancelled() {
        return Err(GitProbeError::Cancelled);
    }
    let mut command = Command::new("git");
    command.current_dir(path).args(args);
    configure_git_environment(&mut command);
    let output = command.output().map_err(|error| {
        GitProbeError::Failed(format!(
            "Failed to run git {} in {}: {error}",
            args.join(" "),
            path.display()
        ))
    })?;
    if cancelled() {
        return Err(GitProbeError::Cancelled);
    }
    if output.status.success() {
        Ok(GitOutput {
            stdout: output.stdout,
            stdout_truncated: false,
        })
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(GitProbeError::Failed(format!(
            "git {} failed with {} in {}: {}",
            args.join(" "),
            format_exit_status(&output.status),
            path.display(),
            stderr.trim()
        )))
    }
}

fn configure_git_environment(command: &mut Command) {
    sanitize_observer_environment(command);
    // `git status` may otherwise take an optional lock and refresh stat data
    // in the index. Status collection is observational, so explicitly disable
    // all optional Git writes after inherited Git controls are removed.
    command.env("GIT_OPTIONAL_LOCKS", "0");
}

#[cfg(all(test, unix))]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::{GitProbeError, configure_git_environment, observe_git_checkout_with_cancellation};

    #[test]
    fn configured_git_process_observes_optional_locks_disabled() {
        let mut command = Command::new("sh");
        command
            .args(["-c", "test \"$GIT_OPTIONAL_LOCKS\" = 0"])
            .env("GIT_OPTIONAL_LOCKS", "1");
        configure_git_environment(&mut command);

        assert!(command.status().unwrap().success());
    }

    #[test]
    fn cancellation_before_git_spawn_remains_typed() {
        let result = observe_git_checkout_with_cancellation(Path::new("."), &|| true);

        assert!(matches!(result, Err(GitProbeError::Cancelled)));
    }
}
