use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};
use jig_tui::sanitize_text;

use super::app_server::{self, AppServerThreadLookup, app_server_thread};
#[cfg(all(unix, not(test)))]
use super::finish_signal_supervised;
use super::{
    DiscoveredHomes, DiscoveryIssue, MAX_PARALLEL_HOME_WORKERS, ResumeHomeProbeFailure,
    ResumeHomeSelection, ResumeProbeFailure, SESSION_LOOKUP_CANCELLED, ThreadHomeProbe,
    canonical_or, codex_bin, discover_homes, execute_homes_parallel,
};

pub(crate) fn normalize_session_id(input: &str) -> Result<String> {
    let bytes = input.as_bytes();
    let valid = bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        });
    if !valid {
        bail!(
            "Invalid Codex session ID '{}'; expected a UUID",
            sanitize_text(input)
        )
    }
    Ok(input.to_ascii_lowercase())
}

pub(crate) fn resolve_resume_home(thread_id: &str) -> Result<PathBuf> {
    resolve_resume_home_with_progress(thread_id, |_, _| {})
}

pub(crate) fn resolve_resume_home_with_progress<F>(thread_id: &str, progress: F) -> Result<PathBuf>
where
    F: FnMut(usize, usize),
{
    #[cfg(all(unix, not(test)))]
    {
        let signal_session = crate::doctor::DoctorSignalSession::start().map_err(|_| {
            anyhow!(
                "Codex session lookup was not started because the process-wide signal session is unavailable"
            )
        })?;
        let result = resolve_resume_home_with_cancellation(
            thread_id,
            &|| signal_session.cancelled(),
            progress,
        );
        finish_signal_supervised(
            result,
            signal_session.finish(),
            "Codex session lookup signal supervision could not retire safely",
        )
    }
    #[cfg(any(not(unix), test))]
    {
        resolve_resume_home_with_cancellation(thread_id, &|| false, progress)
    }
}

pub(super) fn resolve_resume_home_with_cancellation<F>(
    thread_id: &str,
    cancelled: &(dyn Fn() -> bool + Sync),
    progress: F,
) -> Result<PathBuf>
where
    F: FnMut(usize, usize),
{
    let discovered = discover_homes()?;
    let codex_bin = codex_bin();
    let probes = probe_thread_homes_parallel_with_progress(
        &discovered.paths,
        cancelled,
        |home| match app_server_thread(&home, &codex_bin, thread_id, cancelled) {
            Ok(AppServerThreadLookup::Found) => ThreadHomeProbe::Found,
            Ok(AppServerThreadLookup::Missing) => ThreadHomeProbe::Missing,
            Err(error) if error == app_server::APP_SERVER_INSPECTION_CANCELLED => {
                ThreadHomeProbe::Failed(ResumeProbeFailure::Cancelled)
            }
            Err(error) => ThreadHomeProbe::Failed(ResumeProbeFailure::Inspection(error)),
        },
        progress,
    );
    select_resume_home(thread_id, discovered, probes)
}

#[cfg(test)]
pub(super) fn probe_thread_homes_parallel<F>(
    homes: &[PathBuf],
    cancelled: &(dyn Fn() -> bool + Sync),
    probe: F,
) -> Vec<ThreadHomeProbe>
where
    F: Fn(PathBuf) -> ThreadHomeProbe + Sync,
{
    probe_thread_homes_parallel_with_limit_and_progress(
        homes,
        cancelled,
        probe,
        MAX_PARALLEL_HOME_WORKERS,
        |_, _| {},
    )
}

pub(super) fn probe_thread_homes_parallel_with_progress<F, P>(
    homes: &[PathBuf],
    cancelled: &(dyn Fn() -> bool + Sync),
    probe: F,
    progress: P,
) -> Vec<ThreadHomeProbe>
where
    F: Fn(PathBuf) -> ThreadHomeProbe + Sync,
    P: FnMut(usize, usize),
{
    probe_thread_homes_parallel_with_limit_and_progress(
        homes,
        cancelled,
        probe,
        MAX_PARALLEL_HOME_WORKERS,
        progress,
    )
}

#[cfg(test)]
pub(super) fn probe_thread_homes_parallel_with_limit<F>(
    homes: &[PathBuf],
    cancelled: &(dyn Fn() -> bool + Sync),
    probe: F,
    max_parallel: usize,
) -> Vec<ThreadHomeProbe>
where
    F: Fn(PathBuf) -> ThreadHomeProbe + Sync,
{
    probe_thread_homes_parallel_with_limit_and_progress(
        homes,
        cancelled,
        probe,
        max_parallel,
        |_, _| {},
    )
}

pub(super) fn probe_thread_homes_parallel_with_limit_and_progress<F, P>(
    homes: &[PathBuf],
    cancelled: &(dyn Fn() -> bool + Sync),
    probe: F,
    max_parallel: usize,
    mut progress: P,
) -> Vec<ThreadHomeProbe>
where
    F: Fn(PathBuf) -> ThreadHomeProbe + Sync,
    P: FnMut(usize, usize),
{
    let total = homes.len();
    progress(0, total);
    let mut completed = 0;
    execute_homes_parallel(
        homes,
        max_parallel,
        |_| cancelled().then_some(ThreadHomeProbe::Failed(ResumeProbeFailure::Cancelled)),
        probe,
        |_, _| {
            completed += 1;
            progress(completed, total);
            true
        },
        || ThreadHomeProbe::Failed(ResumeProbeFailure::WorkerPanicked),
        || ThreadHomeProbe::Failed(ResumeProbeFailure::WorkerStopped),
    )
}

pub(super) fn select_resume_home(
    thread_id: &str,
    discovered: DiscoveredHomes,
    probes: Vec<ThreadHomeProbe>,
) -> Result<PathBuf> {
    debug_assert_eq!(discovered.paths.len(), probes.len());
    let selection = classify_resume_home(
        &discovered.paths,
        discovered.resume_coverage_complete(),
        probes,
    );
    resolve_resume_home_selection(thread_id, &discovered, selection)
}

pub(super) fn classify_resume_home<'a>(
    homes: &'a [PathBuf],
    discovery_complete: bool,
    probes: Vec<ThreadHomeProbe>,
) -> ResumeHomeSelection<'a> {
    let mut matches = Vec::new();
    let mut failures = Vec::new();
    let mut cancelled = false;
    for (home, probe) in homes.iter().zip(probes) {
        match probe {
            ThreadHomeProbe::Found => matches.push(home),
            ThreadHomeProbe::Missing => {}
            ThreadHomeProbe::Failed(ResumeProbeFailure::Cancelled) => cancelled = true,
            ThreadHomeProbe::Failed(failure) => {
                failures.push(ResumeHomeProbeFailure { home, failure });
            }
        }
    }

    if cancelled {
        return ResumeHomeSelection::Cancelled;
    }

    match matches.as_slice() {
        [home] if failures.is_empty() && discovery_complete => ResumeHomeSelection::Unique(home),
        [home] => ResumeHomeSelection::Unconfirmed { home, failures },
        [_, ..] => ResumeHomeSelection::Ambiguous(matches),
        [] => ResumeHomeSelection::Missing {
            failures,
            discovery_incomplete: !discovery_complete,
        },
    }
}

pub(super) fn resolve_resume_home_selection(
    thread_id: &str,
    discovered: &DiscoveredHomes,
    selection: ResumeHomeSelection<'_>,
) -> Result<PathBuf> {
    match selection {
        ResumeHomeSelection::Cancelled => bail!(SESSION_LOOKUP_CANCELLED),
        ResumeHomeSelection::Unique(home) => canonical_or(home.clone()),
        ResumeHomeSelection::Unconfirmed { home, failures } => {
            let home = sanitize_text(&home.display().to_string());
            let checked = discovered
                .paths
                .iter()
                .map(|home| sanitize_text(&home.display().to_string()))
                .collect::<Vec<_>>()
                .join(", ");
            let inspection_incomplete = !failures.is_empty();
            let discovery_incomplete = !discovered.resume_coverage_complete();
            let reason = match (discovery_incomplete, inspection_incomplete) {
                (true, true) => {
                    "home discovery was incomplete and some discovered homes could not be inspected"
                }
                (true, false) => "home discovery was incomplete",
                (false, true) => "some discovered homes could not be inspected",
                (false, false) => {
                    unreachable!("unconfirmed selection requires incomplete evidence")
                }
            };
            let mut message = format!(
                "Codex session '{thread_id}' was found in {home}, but uniqueness could not be confirmed because {reason}; checked homes: {checked}"
            );
            append_resume_lookup_failures(&mut message, failures, &discovered.issues);
            message.push_str("\nPass --home HOME to resume the confirmed home explicitly.");
            Err(anyhow!(message))
        }
        ResumeHomeSelection::Ambiguous(matches) => {
            let homes = matches
                .iter()
                .map(|home| sanitize_text(&home.display().to_string()))
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "Codex session '{thread_id}' exists in multiple homes: {homes}; pass --home HOME to choose one explicitly"
            )
        }
        ResumeHomeSelection::Missing {
            failures,
            discovery_incomplete,
        } => {
            let homes = discovered
                .paths
                .iter()
                .map(|home| sanitize_text(&home.display().to_string()))
                .collect::<Vec<_>>()
                .join(", ");
            let homes = if homes.is_empty() { "none" } else { &homes };
            let mut message = if failures.is_empty() && !discovery_incomplete {
                format!("Codex session '{thread_id}' was not found; checked homes: {homes}")
            } else {
                format!(
                    "Codex session '{thread_id}' could not be resolved because lookup coverage was incomplete; checked homes: {homes}"
                )
            };
            append_resume_lookup_failures(&mut message, failures, &discovered.issues);
            message.push_str(
                "\nPass --home HOME to choose a non-conventional home or bypass app-server lookup explicitly.",
            );
            Err(anyhow!(message))
        }
    }
}

pub(super) fn append_resume_lookup_failures(
    message: &mut String,
    failures: Vec<ResumeHomeProbeFailure<'_>>,
    discovery_issues: &[DiscoveryIssue],
) {
    for ResumeHomeProbeFailure { home, failure } in failures {
        message.push_str(&format!(
            "\n  - {}: {}",
            sanitize_text(&home.display().to_string()),
            sanitize_text(failure.message())
        ));
    }
    for issue in discovery_issues {
        message.push_str(&format!(
            "\n  - discovery: {}",
            sanitize_text(&issue.message)
        ));
    }
}
