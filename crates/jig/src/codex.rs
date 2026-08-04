use std::collections::HashSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Map as JsonMap, Value as JsonValue, json};

use self::app_server::{AppServerAccountResponse, app_server_account};

mod app_server;

const CODEX_BIN_ENV: &str = "JIG_CODEX_BIN";
pub(crate) const CODEX_HOME_ENV: &str = "CODEX_HOME";
const MAX_PARALLEL_INSPECTIONS: usize = 4;

#[derive(Clone)]
struct DiscoveredHomes {
    paths: Vec<PathBuf>,
    errors: Vec<String>,
    representation_lossy: bool,
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
        .errors
        .iter()
        .map(|message| {
            json!({
                "home": null,
                "kind": "discovery",
                "message": message
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
    pub(crate) fn discovery_warnings(&self) -> &[String] {
        &self.discovered.errors
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
    if discovered.is_empty() {
        return Ok(Vec::new());
    }

    let worker_count = discovered.len().min(MAX_PARALLEL_INSPECTIONS);
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    let mut inspected = vec![None; discovered.len()];
    let mut progress_error = None;
    thread::scope(|scope| {
        for _ in 0..worker_count {
            let sender = sender.clone();
            let inspect = &inspect;
            let next = &next;
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(home) = discovered.get(index).cloned() else {
                        break;
                    };
                    let result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| inspect(home)))
                            .unwrap_or_else(|_| {
                                inspection_failure("Codex home inspection worker panicked")
                            });
                    if sender.send((index, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        for (index, mut result) in receiver {
            if progress_error.is_none() {
                if let Err(error) = progress(index, &mut result) {
                    progress_error = Some(error);
                }
            }
            inspected[index] = Some(result);
        }
    });

    if let Some(error) = progress_error {
        return Err(error);
    }

    Ok(inspected
        .into_iter()
        .map(|result| {
            result.unwrap_or_else(|| inspection_failure("Codex home inspection worker stopped"))
        })
        .collect())
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
    let codex_bin = codex_bin();
    let representation_lossy = home.as_os_str().to_str().is_none()
        || codex_bin.to_str().is_none()
        || args.iter().any(|arg| arg.to_str().is_none());
    json!({
        "schema_version": 1,
        "ok": true,
        "command": "codex launch",
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

fn inspect_home(
    home: PathBuf,
    codex_bin: &OsStr,
    include_usage: bool,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> JsonValue {
    if cancelled() {
        return inspection_failure("Codex app-server inspection was cancelled");
    }
    match app_server_account(&home, codex_bin, include_usage, cancelled) {
        Ok(response) => inspected_home_json(response, include_usage),
        Err(error) => inspection_failure(error),
    }
}

fn inspection_failure(error: impl Into<String>) -> JsonValue {
    let error = error.into();
    json!({
        "account": null,
        "rate_limits": [],
        "status": "unknown",
        "inspection_error": error,
        "usage_error": null
    })
}

fn inspected_home_json(response: AppServerAccountResponse, include_usage: bool) -> JsonValue {
    let account = match normalize_account(&response.account) {
        Ok(account) => account,
        Err(error) => return inspection_failure(error),
    };
    let rate_limits = if account.is_null() {
        Vec::new()
    } else {
        response
            .rate_limits
            .as_ref()
            .map(normalize_rate_limits)
            .unwrap_or_default()
    };
    let status = if account.is_null() {
        Some("not logged in".to_owned())
    } else {
        None
    };
    let mut usage_error = if account.is_null() {
        None
    } else {
        response.usage_error
    };
    if !account.is_null()
        && include_usage
        && usage_error.is_none()
        && !rate_limits_have_usage_data(&rate_limits)
    {
        usage_error = Some("account/rateLimits/read returned no usage data".into());
    }
    json!({
        "account": account,
        "rate_limits": rate_limits,
        "usage_included": include_usage,
        "status": status,
        "inspection_error": null,
        "usage_error": usage_error
    })
}

fn normalize_account(result: &JsonValue) -> std::result::Result<JsonValue, String> {
    let Some(account) = result.get("account") else {
        return Err("account/read result did not include an account field".into());
    };
    if account.is_null() {
        return Ok(JsonValue::Null);
    }
    let Some(account) = account.as_object() else {
        return Err("account/read result included an invalid account field".into());
    };
    Ok(json!({
        "type": account.get("type").cloned().unwrap_or(JsonValue::Null),
        "email": account.get("email").cloned().unwrap_or(JsonValue::Null),
        "plan_type": account.get("planType").cloned().unwrap_or(JsonValue::Null)
    }))
}

fn normalize_rate_limits(result: &JsonValue) -> Vec<JsonValue> {
    let mut buckets = result
        .get("rateLimitsByLimitId")
        .and_then(JsonValue::as_object)
        .filter(|buckets| !buckets.is_empty())
        .map(|buckets| {
            buckets
                .iter()
                .map(|(id, bucket)| normalized_bucket(id, bucket))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            result
                .get("rateLimits")
                .filter(|bucket| bucket.is_object())
                .map(|bucket| {
                    let id = bucket
                        .get("limitId")
                        .and_then(JsonValue::as_str)
                        .unwrap_or("codex");
                    vec![normalized_bucket(id, bucket)]
                })
                .unwrap_or_default()
        });
    buckets.sort_by(|left, right| {
        left.get("id")
            .and_then(JsonValue::as_str)
            .cmp(&right.get("id").and_then(JsonValue::as_str))
    });
    buckets
}

fn rate_limits_have_usage_data(buckets: &[JsonValue]) -> bool {
    buckets.iter().any(|bucket| {
        ["primary", "secondary"].into_iter().any(|window| {
            bucket
                .get(window)
                .and_then(|window| window.get("used_percent"))
                .and_then(JsonValue::as_f64)
                .is_some()
        })
    })
}

fn normalized_bucket(id: &str, bucket: &JsonValue) -> JsonValue {
    json!({
        "id": id,
        "name": bucket.get("limitName").cloned().unwrap_or(JsonValue::Null),
        "plan_type": bucket.get("planType").cloned().unwrap_or(JsonValue::Null),
        "primary": normalized_window(bucket.get("primary")),
        "secondary": normalized_window(bucket.get("secondary")),
        "reached": bucket.get("rateLimitReachedType").cloned().unwrap_or(JsonValue::Null)
    })
}

fn normalized_window(window: Option<&JsonValue>) -> JsonValue {
    let Some(window) = window.and_then(JsonValue::as_object) else {
        return JsonValue::Null;
    };
    let mut normalized = JsonMap::new();
    normalized.insert(
        "used_percent".into(),
        window
            .get("usedPercent")
            .cloned()
            .unwrap_or(JsonValue::Null),
    );
    normalized.insert(
        "duration_minutes".into(),
        window
            .get("windowDurationMins")
            .cloned()
            .unwrap_or(JsonValue::Null),
    );
    normalized.insert(
        "resets_at".into(),
        window.get("resetsAt").cloned().unwrap_or(JsonValue::Null),
    );
    JsonValue::Object(normalized)
}

fn discover_homes() -> Result<DiscoveredHomes> {
    let user_home = user_home()?;
    let current = current_codex_home()?;
    Ok(discover_homes_from(&user_home, &current))
}

fn discover_homes_from(user_home: &Path, current: &Path) -> DiscoveredHomes {
    discover_homes_from_with_metadata(user_home, current, |path| fs::metadata(path))
}

fn discover_homes_from_with_metadata<F>(
    user_home: &Path,
    current: &Path,
    metadata: F,
) -> DiscoveredHomes
where
    F: Fn(&Path) -> io::Result<fs::Metadata>,
{
    discover_homes_from_with_sources(user_home, current, metadata, |path, inspect_entry| {
        for entry in fs::read_dir(path)? {
            inspect_entry(entry.map(|entry| (entry.file_name(), entry.path())));
        }
        Ok(())
    })
}

fn discover_homes_from_with_sources<F, R>(
    user_home: &Path,
    current: &Path,
    metadata: F,
    read_entries: R,
) -> DiscoveredHomes
where
    F: Fn(&Path) -> io::Result<fs::Metadata>,
    R: FnOnce(&Path, &mut dyn FnMut(io::Result<(OsString, PathBuf)>)) -> io::Result<()>,
{
    let mut candidates = Vec::new();
    let mut errors = Vec::new();
    let mut representation_lossy = false;
    let mut inspected = HashSet::new();
    let default = user_home.join(".codex");
    inspected.insert(default.clone());
    add_directory_candidate(
        default,
        false,
        &metadata,
        &mut candidates,
        &mut errors,
        &mut representation_lossy,
    );
    let scan_result = {
        let mut inspect_entry = |entry: io::Result<(OsString, PathBuf)>| {
            let (name, path) = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    errors.push(format!("Failed to inspect a Codex home candidate: {error}"));
                    return;
                }
            };
            if name.to_string_lossy().starts_with(".codex-") {
                inspected.insert(path.clone());
                add_directory_candidate(
                    path,
                    true,
                    &metadata,
                    &mut candidates,
                    &mut errors,
                    &mut representation_lossy,
                );
            }
        };
        read_entries(user_home, &mut inspect_entry)
    };
    if let Err(error) = scan_result {
        errors.push(format!(
            "Failed to inspect {} for Codex homes: {error}",
            user_home.display()
        ));
    }
    if !inspected.contains(current) {
        add_directory_candidate(
            current.to_path_buf(),
            true,
            &metadata,
            &mut candidates,
            &mut errors,
            &mut representation_lossy,
        );
    }

    let mut seen = HashSet::new();
    candidates.retain(|candidate| seen.insert(canonical_key(candidate)));
    candidates.sort_by(|left, right| {
        let left_name = home_name(left);
        let right_name = home_name(right);
        (left_name != "codex", left_name).cmp(&(right_name != "codex", right_name))
    });
    DiscoveredHomes {
        paths: candidates,
        errors,
        representation_lossy,
    }
}

fn add_directory_candidate<F>(
    candidate: PathBuf,
    report_not_found: bool,
    metadata: &F,
    candidates: &mut Vec<PathBuf>,
    errors: &mut Vec<String>,
    representation_lossy: &mut bool,
) where
    F: Fn(&Path) -> io::Result<fs::Metadata>,
{
    *representation_lossy |= candidate.as_os_str().to_str().is_none();
    match metadata(&candidate) {
        Ok(metadata) if metadata.is_dir() => candidates.push(candidate),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound && !report_not_found => {}
        Err(error) => errors.push(format!(
            "Failed to inspect Codex home candidate {}: {error}",
            candidate.display()
        )),
    }
}

fn current_codex_home() -> Result<PathBuf> {
    let configured = configured_codex_home()
        .ok_or_else(|| anyhow!("Could not determine the Codex home directory"))?;
    absolute_path(configured)
}

fn user_home() -> Result<PathBuf> {
    dirs::home_dir().ok_or_else(|| anyhow!("Could not determine the user home directory"))
}

fn expand_tilde_path(input: &Path, user_home: &Path) -> Option<PathBuf> {
    let mut components = input.components();
    if components.next() == Some(std::path::Component::Normal(OsStr::new("~"))) {
        return Some(user_home.join(components.as_path()));
    }
    None
}

fn has_tilde_prefix(input: &Path) -> bool {
    input.components().next() == Some(std::path::Component::Normal(OsStr::new("~")))
}

fn is_bare_home_name(input: &Path) -> bool {
    let mut components = input.components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

fn absolute_path(path: PathBuf) -> Result<PathBuf> {
    absolute_path_with_current_dir(path, || {
        env::current_dir().context("Failed to resolve the current directory")
    })
}

fn absolute_path_with_current_dir<C>(path: PathBuf, current_dir: C) -> Result<PathBuf>
where
    C: FnOnce() -> Result<PathBuf>,
{
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(current_dir()?.join(path))
    }
}

fn canonical_or(path: PathBuf) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("Failed to resolve Codex home {}", path.display()))
}

fn canonical_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn same_path(left: &Path, right: &Path) -> bool {
    canonical_key(left) == canonical_key(right)
}

fn home_name(path: &Path) -> String {
    let name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy();
    name.strip_prefix('.').unwrap_or(&name).to_string()
}

fn home_name_matches(path: &Path, requested: &OsStr) -> bool {
    let name = path.file_name().unwrap_or(path.as_os_str());
    let encoded = name.as_encoded_bytes();
    encoded.strip_prefix(b".").unwrap_or(encoded) == requested.as_encoded_bytes()
}

fn conventional_home(user_home: &Path, requested: &OsStr) -> PathBuf {
    if requested == "codex" || requested == "default" {
        return user_home.join(".codex");
    }
    let mut name = OsString::from(".");
    if !requested.as_encoded_bytes().starts_with(b"codex-") {
        name.push("codex-");
    }
    name.push(requested);
    user_home.join(name)
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
