//! Finds the Claude home that holds a session transcript.
//!
//! Claude stores each session as `projects/PROJECT/SESSION_ID.jsonl` inside its
//! configuration directory and finds the session's project itself on resume, so
//! only the home has to be chosen. The layout is a Claude implementation detail;
//! an explicit `--home` bypasses this lookup.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use jig_tui::sanitize_text;

use super::{Home, Homes, discover_homes, validate_home};
use crate::home_paths::{canonical_key, same_path};

const SESSION_LOOKUP_CANCELLED: &str = "Claude session lookup was cancelled";

pub fn normalize_session_id(input: &str) -> Result<String> {
    crate::session_id::normalize(input, "Claude")
}

pub fn resolve_resume_home_with_cancellation(
    session_id: &str,
    cancelled: &(dyn Fn() -> bool + Sync),
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Home> {
    resolve_resume_home(&discover_homes()?, session_id, cancelled, progress)
}

/// A missing path, or one beneath a file, cannot hold sessions.
pub(super) fn is_absent(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

#[derive(Debug)]
enum Probe {
    Found,
    Missing,
    Failed(String),
}

fn resolve_resume_home(
    homes: &Homes,
    session_id: &str,
    cancelled: &(dyn Fn() -> bool + Sync),
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Home> {
    let total = homes.paths.len();
    progress(0, total);
    let mut probes = Vec::with_capacity(total);
    for home in &homes.paths {
        if cancelled() {
            bail!(SESSION_LOOKUP_CANCELLED);
        }
        probes.push(probe_home(home, session_id, cancelled)?);
        progress(probes.len(), total);
    }
    select_resume_home(homes, session_id, probes)
}

fn probe_home(
    home: &Path,
    session_id: &str,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Probe> {
    let projects = home.join("projects");
    let entries = match fs::read_dir(&projects) {
        Ok(entries) => entries,
        Err(error) if is_absent(&error) => return Ok(Probe::Missing),
        Err(error) => {
            return Ok(Probe::Failed(format!(
                "Failed to read {}: {error}",
                projects.display()
            )));
        }
    };
    let transcript = format!("{session_id}.jsonl");
    // A match settles this home; a failure only matters if nothing matched.
    let mut failure = None;
    for entry in entries {
        if cancelled() {
            bail!(SESSION_LOOKUP_CANCELLED);
        }
        let candidate = match entry {
            Ok(entry) => entry.path().join(&transcript),
            Err(error) => {
                failure.get_or_insert_with(|| {
                    format!("Failed to read {}: {error}", projects.display())
                });
                continue;
            }
        };
        match fs::metadata(&candidate) {
            Ok(metadata) if metadata.is_file() => return Ok(Probe::Found),
            Ok(_) => {}
            Err(error) if is_absent(&error) => {}
            Err(error) => {
                failure.get_or_insert_with(|| {
                    format!("Failed to inspect {}: {error}", candidate.display())
                });
            }
        }
    }
    Ok(failure.map_or(Probe::Missing, Probe::Failed))
}

fn select_resume_home(homes: &Homes, session_id: &str, probes: Vec<Probe>) -> Result<Home> {
    debug_assert_eq!(homes.paths.len(), probes.len());
    let mut matches = Vec::new();
    let mut failures = Vec::new();
    for (home, probe) in homes.paths.iter().zip(probes) {
        match probe {
            Probe::Found => matches.push(home),
            Probe::Missing => {}
            Probe::Failed(message) => failures.push((home, message)),
        }
    }

    let checked = display_paths(&homes.paths);
    match matches.as_slice() {
        [home] if failures.is_empty() && homes.complete => launch_home(homes, home),
        [home] => {
            let reason = match (homes.complete, failures.is_empty()) {
                (false, false) => {
                    "home discovery was incomplete and some discovered homes could not be inspected"
                }
                (false, true) => "home discovery was incomplete",
                (true, _) => "some discovered homes could not be inspected",
            };
            let mut message = format!(
                "Claude session '{session_id}' was found in {}, but uniqueness could not be confirmed because {reason}; checked homes: {checked}",
                display_path(home)
            );
            append_lookup_failures(&mut message, &failures, &homes.warnings);
            message.push_str("\nPass --home HOME to resume the confirmed home explicitly.");
            Err(anyhow!(message))
        }
        [_, _, ..] => bail!(
            "Claude session '{session_id}' exists in multiple homes: {}; pass --home HOME to choose one explicitly",
            display_paths(matches.iter().copied())
        ),
        [] => {
            let mut message = if failures.is_empty() && homes.complete {
                format!("Claude session '{session_id}' was not found; checked homes: {checked}")
            } else {
                format!(
                    "Claude session '{session_id}' could not be resolved because lookup coverage was incomplete; checked homes: {checked}"
                )
            };
            append_lookup_failures(&mut message, &failures, &homes.warnings);
            message.push_str(
                "\nPass --home HOME to choose a non-conventional home or bypass transcript lookup explicitly.",
            );
            Err(anyhow!(message))
        }
    }
}

/// Resumes in the mode the directory is used with: Claude's native default for
/// `~/.claude`, unless `CLAUDE_CONFIG_DIR` currently selects that directory.
fn launch_home(homes: &Homes, path: &Path) -> Result<Home> {
    let explicit_default = !homes.current_is_default && same_path(&homes.current, &homes.default);
    if same_path(path, &homes.default) && !explicit_default {
        return Ok(Home {
            path: canonical_key(path),
            default_config: true,
        });
    }
    validate_home(path)
}

fn append_lookup_failures(
    message: &mut String,
    failures: &[(&PathBuf, String)],
    discovery_warnings: &[String],
) {
    for (home, failure) in failures {
        message.push_str(&format!(
            "\n  - {}: {}",
            display_path(home),
            sanitize_text(failure)
        ));
    }
    for warning in discovery_warnings {
        message.push_str(&format!("\n  - discovery: {}", sanitize_text(warning)));
    }
}

fn display_paths<'a>(paths: impl IntoIterator<Item = &'a PathBuf>) -> String {
    let paths = paths
        .into_iter()
        .map(|path| display_path(path))
        .collect::<Vec<_>>();
    if paths.is_empty() {
        "none".into()
    } else {
        paths.join(", ")
    }
}

fn display_path(path: &Path) -> String {
    sanitize_text(&path.display().to_string())
}

#[cfg(test)]
mod tests;
