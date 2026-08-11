use std::env;
use std::ffi::{OsStr, OsString};
#[cfg(unix)]
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

#[cfg(unix)]
use anyhow::anyhow;
use anyhow::{Context, Result, bail};
use serde_json::{Value as JsonValue, json};

use self::home::{
    canonical_or, conventional_home, current_codex_home, discover_homes, discover_homes_from,
    expand_tilde_path, has_tilde_prefix, home_name, home_name_matches, is_bare_home_name,
    same_path, user_home,
};
use self::inspection::{inspect_home, inspection_failure};
pub(crate) use self::resume::{
    normalize_session_id, resolve_resume_home, resolve_resume_home_with_progress,
};

mod app_server;
mod home;
mod inspection;
mod resume;

const CODEX_BIN_ENV: &str = "JIG_CODEX_BIN";
pub(crate) const CODEX_HOME_ENV: &str = "CODEX_HOME";
const MAX_PARALLEL_HOME_WORKERS: usize = 4;
const SESSION_LOOKUP_CANCELLED: &str = "Codex session lookup was cancelled";

#[derive(Clone)]
struct DiscoveredHomes {
    paths: Vec<PathBuf>,
    issues: Vec<DiscoveryIssue>,
    representation_lossy: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiscoveryIssueKind {
    CandidateMissing,
    CandidateUnreadable,
    EntryUnreadable,
    ScanIncomplete,
}

#[derive(Clone, Debug)]
struct DiscoveryIssue {
    kind: DiscoveryIssueKind,
    message: String,
}

impl DiscoveryIssue {
    fn new(kind: DiscoveryIssueKind, message: String) -> Self {
        Self { kind, message }
    }

    fn blocks_resume_uniqueness(&self) -> bool {
        self.kind != DiscoveryIssueKind::CandidateMissing
    }
}

impl DiscoveredHomes {
    fn resume_coverage_complete(&self) -> bool {
        !self
            .issues
            .iter()
            .any(DiscoveryIssue::blocks_resume_uniqueness)
    }
}

/// Exact discovered homes plus the inputs needed for background inspection.
pub(crate) struct CodexHomeInspection {
    discovered: DiscoveredHomes,
    current: PathBuf,
    codex_bin: OsString,
}

/// Inexpensive display metadata for one exact discovered home.
pub(crate) struct CodexHomeCandidate {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) current: bool,
}

#[derive(Debug)]
enum ThreadHomeProbe {
    Found,
    Missing,
    Failed(ResumeProbeFailure),
}

#[derive(Debug)]
enum ResumeProbeFailure {
    Cancelled,
    Inspection(String),
    WorkerPanicked,
    WorkerStopped,
}

impl ResumeProbeFailure {
    fn message(&self) -> &str {
        match self {
            Self::Cancelled => SESSION_LOOKUP_CANCELLED,
            Self::Inspection(message) => message,
            Self::WorkerPanicked => "Codex session lookup worker panicked",
            Self::WorkerStopped => "Codex session lookup worker stopped",
        }
    }
}

#[derive(Debug)]
struct ResumeHomeProbeFailure<'a> {
    home: &'a PathBuf,
    failure: ResumeProbeFailure,
}

/// The complete, closed set of policy outcomes from resume-home probing.
///
/// This deliberately carries only the probe classification and its associated
/// homes. Path canonicalization and user-facing diagnostics happen after this
/// phase so they cannot accidentally change the selection policy.
#[derive(Debug)]
enum ResumeHomeSelection<'a> {
    Cancelled,
    Unique(&'a PathBuf),
    Unconfirmed {
        home: &'a PathBuf,
        failures: Vec<ResumeHomeProbeFailure<'a>>,
    },
    Ambiguous(Vec<&'a PathBuf>),
    Missing {
        failures: Vec<ResumeHomeProbeFailure<'a>>,
        discovery_incomplete: bool,
    },
}

#[derive(Debug)]
pub(crate) struct CodexChildExitStatus(pub(crate) i32);

impl std::fmt::Display for CodexChildExitStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Codex exited with status {}", self.0)
    }
}

impl std::error::Error for CodexChildExitStatus {}

pub(crate) fn homes_report(include_usage: bool) -> Result<JsonValue> {
    homes_report_with_paths(include_usage).map(|(report, _)| report)
}

pub(crate) fn homes_report_with_paths(include_usage: bool) -> Result<(JsonValue, Vec<PathBuf>)> {
    homes_report_with_progress(include_usage, |_, _, _| Ok(()))
}

pub(crate) fn homes_report_with_progress<F>(
    include_usage: bool,
    progress: F,
) -> Result<(JsonValue, Vec<PathBuf>)>
where
    F: FnMut(usize, usize, Option<(usize, &JsonValue)>) -> Result<()>,
{
    #[cfg(all(unix, not(test)))]
    {
        let signal_session = crate::doctor::DoctorSignalSession::start().map_err(|_| {
            anyhow!(
                "Codex home inspection was not started because the process-wide signal session is unavailable"
            )
        })?;
        let result = homes_report_with_progress_and_cancellation(include_usage, progress, &|| {
            signal_session.cancelled()
        });
        finish_signal_supervised(
            result,
            signal_session.finish(),
            "Codex home inspection supervision could not retire safely",
        )
    }
    #[cfg(any(not(unix), test))]
    {
        homes_report_with_progress_and_cancellation(include_usage, progress, &|| false)
    }
}

#[cfg(unix)]
pub(crate) fn finish_signal_supervised<T>(
    outcome: Result<T>,
    retirement: io::Result<()>,
    retirement_message: &'static str,
) -> Result<T> {
    match retirement {
        Ok(()) => outcome,
        Err(_) => match outcome {
            Ok(_) => Err(anyhow!(retirement_message)),
            Err(error) => Err(error.context(format!(
                "{retirement_message}; the supervised operation also failed"
            ))),
        },
    }
}

fn homes_report_with_progress_and_cancellation<F>(
    include_usage: bool,
    progress: F,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<(JsonValue, Vec<PathBuf>)>
where
    F: FnMut(usize, usize, Option<(usize, &JsonValue)>) -> Result<()>,
{
    let discovered = discover_homes()?;
    let current = current_codex_home()?;
    let codex_bin = codex_bin();
    homes_report_from_discovered(
        include_usage,
        discovered,
        &current,
        &codex_bin,
        |home| inspect_home(home, &codex_bin, include_usage, cancelled),
        progress,
    )
}

fn homes_report_from_discovered<F, P>(
    include_usage: bool,
    discovered: DiscoveredHomes,
    current: &Path,
    codex_bin: &OsStr,
    inspect: F,
    mut progress: P,
) -> Result<(JsonValue, Vec<PathBuf>)>
where
    F: Fn(PathBuf) -> JsonValue + Sync,
    P: FnMut(usize, usize, Option<(usize, &JsonValue)>) -> Result<()>,
{
    let total = discovered.paths.len();
    progress(0, total, None)?;
    let mut completed = 0;
    let homes = inspect_homes_parallel(&discovered.paths, inspect, |index, home| {
        enrich_inspected_home(&discovered.paths[index], current, include_usage, home);
        completed += 1;
        progress(completed, total, Some((index, home)))
    })?;

    let mut errors = discovered
        .issues
        .iter()
        .map(|issue| {
            json!({
                "home": null,
                "kind": "discovery",
                "message": issue.message
            })
        })
        .collect::<Vec<_>>();
    errors.extend(inspected_home_errors(&homes));

    let outcome = if errors.is_empty() {
        "complete"
    } else {
        "partial"
    };
    let representation_lossy = codex_bin.to_str().is_none()
        || current.as_os_str().to_str().is_none()
        || discovered.representation_lossy
        || discovered
            .paths
            .iter()
            .any(|home| home.as_os_str().to_str().is_none());
    let report = json!({
        "schema_version": 1,
        "ok": true,
        "outcome": outcome,
        "command": "codex homes",
        "usage_included": include_usage,
        "representation_lossy": representation_lossy,
        "codex_bin": codex_bin.to_string_lossy(),
        "current_home": current.display().to_string(),
        "homes": homes,
        "errors": errors
    });
    Ok((report, discovered.paths))
}

pub(crate) fn discover_home_inspection() -> Result<CodexHomeInspection> {
    Ok(CodexHomeInspection {
        discovered: discover_homes()?,
        current: current_codex_home()?,
        codex_bin: codex_bin(),
    })
}

impl CodexHomeInspection {
    pub(crate) fn discovery_warnings(&self) -> Vec<String> {
        self.discovered
            .issues
            .iter()
            .map(|issue| issue.message.clone())
            .collect()
    }

    pub(crate) fn candidates(&self) -> Vec<CodexHomeCandidate> {
        self.discovered
            .paths
            .iter()
            .map(|path| CodexHomeCandidate {
                path: path.clone(),
                name: home_name(path),
                current: same_path(path, &self.current),
            })
            .collect()
    }

    pub(crate) fn inspect<F>(
        &self,
        cancelled: &(dyn Fn() -> bool + Sync),
        mut emit: F,
    ) -> Result<()>
    where
        F: FnMut(usize, JsonValue) -> Result<()>,
    {
        homes_report_from_discovered(
            true,
            self.discovered.clone(),
            &self.current,
            &self.codex_bin,
            |home| inspect_home(home, &self.codex_bin, true, cancelled),
            |_, _, update| match update {
                Some((index, home)) => emit(index, home.clone()),
                None => Ok(()),
            },
        )?;
        Ok(())
    }
}

fn inspect_homes_parallel<F, P>(
    discovered: &[PathBuf],
    inspect: F,
    mut progress: P,
) -> Result<Vec<JsonValue>>
where
    F: Fn(PathBuf) -> JsonValue + Sync,
    P: FnMut(usize, &mut JsonValue) -> Result<()>,
{
    let mut progress_error = None;
    let inspected = execute_homes_parallel(
        discovered,
        MAX_PARALLEL_HOME_WORKERS,
        |_| None,
        inspect,
        |index, result| match progress(index, result) {
            Ok(()) => true,
            Err(error) => {
                progress_error = Some(error);
                false
            }
        },
        || inspection_failure("Codex home inspection worker panicked"),
        || inspection_failure("Codex home inspection worker stopped"),
    );

    if let Some(error) = progress_error {
        return Err(error);
    }

    Ok(inspected)
}

/// Runs bounded work over exact homes while retaining input-order results.
///
/// Completion callbacks observe results as workers finish. Returning `false`
/// stops further callbacks but deliberately keeps draining worker results so
/// scoped threads finish before the caller observes its callback error.
fn execute_homes_parallel<T, C, F, P, H, S>(
    homes: &[PathBuf],
    max_parallel: usize,
    preflight: C,
    work: F,
    mut on_complete: P,
    panicked: H,
    stopped: S,
) -> Vec<T>
where
    T: Send,
    C: Fn(&Path) -> Option<T> + Sync,
    F: Fn(PathBuf) -> T + Sync,
    P: FnMut(usize, &mut T) -> bool,
    H: Fn() -> T + Sync,
    S: Fn() -> T,
{
    if homes.is_empty() {
        return Vec::new();
    }

    let worker_count = homes.len().min(max_parallel.max(1));
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    let mut results = (0..homes.len()).map(|_| None).collect::<Vec<_>>();
    let mut report_completions = true;
    thread::scope(|scope| {
        for _ in 0..worker_count {
            let sender = sender.clone();
            let next = &next;
            let preflight = &preflight;
            let work = &work;
            let panicked = &panicked;
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(home) = homes.get(index).cloned() else {
                        break;
                    };
                    let result = preflight(&home).unwrap_or_else(|| {
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(home)))
                            .unwrap_or_else(|_| panicked())
                    });
                    if sender.send((index, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        for (index, mut result) in receiver {
            if report_completions {
                report_completions = on_complete(index, &mut result);
            }
            results[index] = Some(result);
        }
    });

    results
        .into_iter()
        .map(|result| result.unwrap_or_else(&stopped))
        .collect()
}

fn enrich_inspected_home(
    home: &Path,
    current: &Path,
    include_usage: bool,
    inspected: &mut JsonValue,
) {
    let name = home_name(home);
    let is_current = same_path(home, current);
    let Some(object) = inspected.as_object_mut() else {
        return;
    };
    object.insert("name".into(), json!(name));
    object.insert("home".into(), json!(home.display().to_string()));
    object.insert("current".into(), json!(is_current));
    object.insert("usage_included".into(), json!(include_usage));
    object.entry("inspection_error").or_insert(JsonValue::Null);
    object.entry("usage_error").or_insert(JsonValue::Null);
}

fn inspected_home_errors(homes: &[JsonValue]) -> Vec<JsonValue> {
    let mut errors = Vec::new();
    for inspected in homes {
        let home = inspected.get("home").cloned().unwrap_or(JsonValue::Null);
        if let Some(message) = inspected
            .get("inspection_error")
            .and_then(JsonValue::as_str)
        {
            errors.push(json!({
                "home": home.clone(),
                "kind": "inspection",
                "message": message
            }));
        }
        if let Some(message) = inspected.get("usage_error").and_then(JsonValue::as_str) {
            errors.push(json!({
                "home": home,
                "kind": "usage",
                "message": message
            }));
        }
    }
    errors
}

pub(crate) fn resolve_launch_home(input: &Path) -> Result<PathBuf> {
    resolve_launch_home_with_sources(
        input,
        || env::current_dir().context("Failed to resolve the current directory"),
        user_home,
        |user_home| {
            let current = current_codex_home()?;
            Ok(discover_homes_from(user_home, &current).paths)
        },
    )
}

pub(crate) fn resolve_configured_home_from_dir(
    input: &Path,
    current_dir: &Path,
) -> Result<PathBuf> {
    if is_bare_home_name(input) && !has_tilde_prefix(input) {
        let user_home = user_home()?;
        let requested = input.as_os_str();
        let conventional = conventional_home(&user_home, requested);
        if conventional.is_dir() {
            return canonical_or(conventional);
        }
        bail!(
            "Configured Codex home '{}' was not found at {}; use an explicit path for a non-conventional home",
            requested.to_string_lossy(),
            conventional.display()
        );
    }

    resolve_launch_home_with_sources(
        input,
        || Ok(current_dir.to_path_buf()),
        user_home,
        |_| -> Result<Vec<PathBuf>> {
            unreachable!("configured bare homes are resolved before discovery")
        },
    )
}

fn resolve_launch_home_with_sources<C, U, D>(
    input: &Path,
    current_dir: C,
    user_home: U,
    discover: D,
) -> Result<PathBuf>
where
    C: FnOnce() -> Result<PathBuf>,
    U: FnOnce() -> Result<PathBuf>,
    D: FnOnce(&Path) -> Result<Vec<PathBuf>>,
{
    if input.is_absolute() {
        return resolve_explicit_launch_home(input.to_path_buf());
    }
    if has_tilde_prefix(input) {
        let candidate = expand_tilde_path(input, &user_home()?)
            .expect("tilde prefix was checked before expansion");
        return resolve_explicit_launch_home(candidate);
    }

    if !is_bare_home_name(input) {
        return resolve_explicit_launch_home(current_dir()?.join(input));
    }

    let user_home = user_home()?;
    let discovered = discover(&user_home)?;
    resolve_launch_home_from(input, &user_home, &discovered)
}

fn resolve_explicit_launch_home(candidate: PathBuf) -> Result<PathBuf> {
    if candidate.is_dir() {
        return canonical_or(candidate);
    }
    bail!("Codex home does not exist: {}", candidate.display())
}

fn resolve_launch_home_from(
    input: &Path,
    user_home: &Path,
    discovered: &[PathBuf],
) -> Result<PathBuf> {
    debug_assert!(is_bare_home_name(input));
    let requested = input.as_os_str();
    let conventional = conventional_home(user_home, requested);
    if conventional.is_dir() {
        return canonical_or(conventional);
    }

    let matches = discovered
        .iter()
        .filter(|home| home_name_matches(home, requested))
        .cloned()
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [home] => return canonical_or(home.clone()),
        [_, ..] => {
            bail!(
                "Codex home name '{}' is ambiguous; pass an explicit path instead",
                requested.to_string_lossy()
            )
        }
        [] => {}
    }

    let discovered_names = discovered
        .iter()
        .map(|home| home_name(home))
        .collect::<Vec<_>>();
    let discovered_names = if discovered_names.is_empty() {
        "none".to_owned()
    } else {
        discovered_names.join(", ")
    };
    bail!(
        "Codex home '{}' was not found; checked {}. Discovered homes: {discovered_names}",
        requested.to_string_lossy(),
        conventional.display()
    )
}

pub(crate) fn dry_run_report(home: &Path, args: &[OsString]) -> JsonValue {
    command_dry_run_report("codex launch", home, args)
}

pub(crate) fn resume_dry_run_report(home: &Path, args: &[OsString]) -> JsonValue {
    command_dry_run_report("codex resume", home, args)
}

fn command_dry_run_report(command: &str, home: &Path, args: &[OsString]) -> JsonValue {
    let codex_bin = codex_bin();
    let representation_lossy = home.as_os_str().to_str().is_none()
        || codex_bin.to_str().is_none()
        || args.iter().any(|arg| arg.to_str().is_none());
    json!({
        "schema_version": 1,
        "ok": true,
        "command": command,
        "dry_run": true,
        "home": home.display().to_string(),
        "codex_bin": codex_bin.to_string_lossy(),
        "args": args.iter().map(|arg| arg.to_string_lossy()).collect::<Vec<_>>(),
        "representation_lossy": representation_lossy
    })
}

pub(crate) fn launch(home: &Path, args: &[OsString]) -> Result<()> {
    let codex_bin = codex_bin();
    let mut command = Command::new(&codex_bin);
    command.args(args).env(CODEX_HOME_ENV, home);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        let error = command.exec();
        Err(error).with_context(|| {
            format!(
                "Failed to launch {} with CODEX_HOME={}",
                codex_bin.to_string_lossy(),
                home.display()
            )
        })
    }

    #[cfg(not(unix))]
    {
        let status = command.status().with_context(|| {
            format!(
                "Failed to launch {} with CODEX_HOME={}",
                codex_bin.to_string_lossy(),
                home.display()
            )
        })?;
        if !status.success() {
            let exit_status = status.code().unwrap_or(1).clamp(1, 255);
            return Err(CodexChildExitStatus(exit_status).into());
        }
        Ok(())
    }
}

pub(crate) fn configured_codex_home() -> Option<PathBuf> {
    env::var_os(CODEX_HOME_ENV)
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
}

pub(crate) fn codex_config_path() -> Option<PathBuf> {
    configured_codex_home().map(|home| home.join("config.toml"))
}

pub(crate) fn codex_bin() -> OsString {
    env::var_os(CODEX_BIN_ENV).unwrap_or_else(|| OsString::from("codex"))
}

#[cfg(test)]
#[path = "codex/tests.rs"]
mod tests;
