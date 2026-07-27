use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Output};

use jig_contract::status_provider::v1::Input;
use serde::Serialize;

use super::sanitize_observer_environment;
use crate::process::format_exit_status;

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

pub(super) fn input_freshness(
    root: &Path,
    input: &Input,
    observations: &mut BTreeMap<String, GitCheckoutObservation>,
) -> InputFreshness {
    if input.kind != "git" {
        return InputFreshness {
            name: input.name.clone(),
            kind: input.kind.clone(),
            path: input.path.clone(),
            expected_revision: input.revision.clone(),
            observed_revision: None,
            dirty: None,
            status: "not_applicable",
            reason: Some("Jig compares revision freshness only for git inputs".into()),
        };
    }

    let key = input.path.clone().unwrap_or_else(|| ".".into());
    let observation = observations
        .entry(key.clone())
        .or_insert_with(|| observe_git_checkout(&root.join(&key)));
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
    InputFreshness {
        name: input.name.clone(),
        kind: input.kind.clone(),
        path: input.path.clone(),
        expected_revision: input.revision.clone(),
        observed_revision: observation.revision.clone(),
        dirty: observation.dirty,
        status,
        reason: (!observation.errors.is_empty()).then(|| observation.errors.join("; ")),
    }
}

#[derive(Clone)]
pub(super) struct GitCheckoutObservation {
    pub(super) revision: Option<String>,
    pub(super) dirty: Option<bool>,
    pub(super) errors: Vec<String>,
}

pub(super) fn observe_git_checkout(path: &Path) -> GitCheckoutObservation {
    let mut errors = Vec::new();
    let revision = match git_text(path, &["rev-parse", "--verify", "HEAD"]) {
        Ok(revision) => Some(revision),
        Err(error) => {
            errors.push(error);
            None
        }
    };
    let dirty = match git_output(
        path,
        &["status", "--porcelain=v1", "-z", "--untracked-files=normal"],
    ) {
        Ok(output) => Some(!output.stdout.is_empty()),
        Err(error) => {
            errors.push(error);
            None
        }
    };
    GitCheckoutObservation {
        revision,
        dirty,
        errors,
    }
}

pub(super) fn git_text(path: &Path, args: &[&str]) -> Result<String, String> {
    let output = git_output(path, args)?;
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|error| format!("git {} returned non-UTF-8 output: {error}", args.join(" ")))?;
    Ok(text.trim().to_string())
}

fn git_output(path: &Path, args: &[&str]) -> Result<Output, String> {
    let mut command = Command::new("git");
    command.current_dir(path).args(args);
    sanitize_observer_environment(&mut command);
    let output = command.output().map_err(|error| {
        format!(
            "Failed to run git {} in {}: {error}",
            args.join(" "),
            path.display()
        )
    })?;
    if output.status.success() {
        Ok(output)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "git {} failed with {} in {}: {}",
            args.join(" "),
            format_exit_status(&output.status),
            path.display(),
            stderr.trim()
        ))
    }
}
