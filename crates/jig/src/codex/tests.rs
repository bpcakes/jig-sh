use std::io::{BufReader, Cursor, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[cfg(unix)]
use super::app_server::app_server_account_with_timeout;
use super::app_server::{
    APP_SERVER_INSPECTION_CANCELLED, APP_SERVER_PROTOCOL_MESSAGE_LIMIT, app_server_protocol,
    protocol_message_too_large, read_next_response, read_response,
};
use super::*;

struct FragmentedNonblockingReader {
    bytes: &'static [u8],
    offset: usize,
    would_block: bool,
}

impl Read for FragmentedNonblockingReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.would_block {
            self.would_block = false;
            return Err(std::io::ErrorKind::WouldBlock.into());
        }
        if self.offset == self.bytes.len() {
            return Ok(0);
        }
        let read = buffer
            .len()
            .min(3)
            .min(self.bytes.len().saturating_sub(self.offset));
        buffer[..read].copy_from_slice(&self.bytes[self.offset..self.offset + read]);
        self.offset += read;
        self.would_block = true;
        Ok(read)
    }
}

struct RepeatingReader {
    bytes: &'static [u8],
    offset: usize,
}

struct CancelAfterEofReader {
    bytes: &'static [u8],
    offset: usize,
    cancelled: Arc<AtomicBool>,
}

impl Read for CancelAfterEofReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.offset == self.bytes.len() {
            self.cancelled.store(true, Ordering::Release);
            return Err(std::io::ErrorKind::WouldBlock.into());
        }
        let read = buffer
            .len()
            .min(self.bytes.len().saturating_sub(self.offset));
        buffer[..read].copy_from_slice(&self.bytes[self.offset..self.offset + read]);
        self.offset += read;
        Ok(read)
    }
}

impl Read for RepeatingReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        for byte in buffer.iter_mut() {
            *byte = self.bytes[self.offset];
            self.offset = (self.offset + 1) % self.bytes.len();
        }
        Ok(buffer.len())
    }
}

#[test]
fn normalizes_all_rate_limit_buckets_and_dynamic_windows() {
    let limits = normalize_rate_limits(&json!({
        "rateLimits": {
            "limitId": "legacy",
            "primary": { "usedPercent": 99, "windowDurationMins": 300 }
        },
        "rateLimitsByLimitId": {
            "codex": {
                "limitId": "codex",
                "primary": {
                    "usedPercent": 25,
                    "windowDurationMins": 10080,
                    "resetsAt": 1234
                },
                "secondary": null
            },
            "spark": {
                "limitId": "spark",
                "limitName": "Spark",
                "primary": { "usedPercent": 5, "windowDurationMins": 60 }
            }
        }
    }));

    assert_eq!(limits.len(), 2);
    assert_eq!(limits[0]["id"], "codex");
    assert_eq!(limits[0]["primary"]["duration_minutes"], 10080);
    assert!(limits[0]["secondary"].is_null());
    assert_eq!(limits[1]["name"], "Spark");
}

#[test]
fn account_normalization_never_includes_tokens_or_unknown_fields() {
    let normalized = normalize_account(&json!({
        "account": {
            "type": "chatgpt",
            "email": "person@example.com",
            "planType": "pro",
            "accessToken": "secret"
        },
        "requiresOpenaiAuth": true
    }))
    .unwrap();

    assert_eq!(normalized["email"], "person@example.com");
    assert_eq!(normalized["plan_type"], "pro");
    assert!(normalized.get("accessToken").is_none());
    assert!(
        !serde_json::to_string(&normalized)
            .unwrap()
            .contains("secret")
    );
}

#[test]
fn launch_home_resolution_distinguishes_names_paths_and_default() {
    let temp = tempfile::tempdir().unwrap();
    let user_home = temp.path().join("user");
    let current_dir = temp.path().join("repo");
    let default = user_home.join(".codex");
    let work = user_home.join(".codex-work");
    let local = current_dir.join("local-home");
    let local_work = current_dir.join("work");
    fs::create_dir_all(&default).unwrap();
    fs::create_dir_all(&work).unwrap();
    fs::create_dir_all(&local).unwrap();
    fs::create_dir_all(&local_work).unwrap();
    let discovered = vec![default.clone(), work.clone(), local_work.clone()];

    assert_eq!(
        resolve_launch_home_from(Path::new("codex"), &user_home, &discovered).unwrap(),
        default.canonicalize().unwrap()
    );
    assert_eq!(
        resolve_launch_home_from(Path::new("default"), &user_home, &discovered).unwrap(),
        default.canonicalize().unwrap()
    );
    assert_eq!(
        resolve_launch_home_from(Path::new("work"), &user_home, &discovered).unwrap(),
        work.canonicalize().unwrap()
    );

    let resolved = resolve_launch_home_with_sources(
        Path::new("./work"),
        || Ok(current_dir.clone()),
        || -> Result<PathBuf> { panic!("explicit relative paths must not resolve the user home") },
        |_| -> Result<Vec<PathBuf>> { panic!("explicit relative paths must not run discovery") },
    )
    .unwrap();
    assert_eq!(resolved, local_work.canonicalize().unwrap());

    let resolved = resolve_launch_home_with_sources(
        Path::new("./local-home"),
        || Ok(current_dir.clone()),
        || -> Result<PathBuf> { panic!("explicit relative paths must not resolve the user home") },
        |_| -> Result<Vec<PathBuf>> { panic!("explicit relative paths must not run discovery") },
    )
    .unwrap();
    assert_eq!(resolved, local.canonicalize().unwrap());
}

#[test]
fn bare_launch_home_never_falls_back_to_current_directory() {
    let temp = tempfile::tempdir().unwrap();
    let user_home = temp.path().join("user");
    let current_dir = temp.path().join("repo");
    let local_work = current_dir.join("work");
    fs::create_dir_all(&user_home).unwrap();
    fs::create_dir_all(&local_work).unwrap();

    let error = resolve_launch_home_with_sources(
        Path::new("work"),
        || Ok(current_dir.clone()),
        || Ok(user_home.clone()),
        |_| Ok(Vec::new()),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Codex home 'work' was not found"));
    assert!(error.contains(&user_home.join(".codex-work").display().to_string()));
    assert!(!error.contains(&local_work.display().to_string()));
}

#[test]
fn explicit_launch_paths_do_not_resolve_or_discover_named_homes() {
    let temp = tempfile::tempdir().unwrap();
    let absolute = temp.path().join("absolute-home");
    fs::create_dir(&absolute).unwrap();

    let resolved = resolve_launch_home_with_sources(
        &absolute,
        || -> Result<PathBuf> { panic!("absolute paths must not resolve the current directory") },
        || -> Result<PathBuf> { panic!("absolute paths must not resolve the user home") },
        |_| -> Result<Vec<PathBuf>> { panic!("absolute paths must not run discovery") },
    )
    .unwrap();
    assert_eq!(resolved, absolute.canonicalize().unwrap());

    let current_dir = temp.path().join("repo");
    let relative = current_dir.join("nested/home");
    fs::create_dir_all(&relative).unwrap();
    let resolved = resolve_launch_home_with_sources(
        Path::new("nested/home"),
        || Ok(current_dir.clone()),
        || -> Result<PathBuf> { panic!("explicit relative paths must not resolve the user home") },
        |_| -> Result<Vec<PathBuf>> { panic!("explicit relative paths must not run discovery") },
    )
    .unwrap();
    assert_eq!(resolved, relative.canonicalize().unwrap());

    let user_home = temp.path().join("user");
    let tilde = user_home.join("custom-home");
    fs::create_dir_all(&tilde).unwrap();
    let resolved = resolve_launch_home_with_sources(
        Path::new("~/custom-home"),
        || -> Result<PathBuf> { panic!("tilde paths must not resolve the current directory") },
        || Ok(user_home.clone()),
        |_| -> Result<Vec<PathBuf>> { panic!("tilde paths must not run discovery") },
    )
    .unwrap();
    assert_eq!(resolved, tilde.canonicalize().unwrap());
}

#[cfg(unix)]
#[test]
fn configured_bare_home_does_not_fall_back_to_ambient_codex_home() {
    let _env = crate::test_env::lock_env();
    let temp = tempfile::tempdir().unwrap();
    let user_home = temp.path().join("user");
    let repo = temp.path().join("repo");
    let ambient = temp.path().join("ambient/.codex-work");
    fs::create_dir_all(&user_home).unwrap();
    fs::create_dir(&repo).unwrap();
    fs::create_dir_all(&ambient).unwrap();
    let _home = crate::test_env::EnvVarGuard::set("HOME", &user_home);
    let _codex_home = crate::test_env::EnvVarGuard::set(CODEX_HOME_ENV, ambient.as_os_str());

    let error = resolve_configured_home_from_dir(Path::new("work"), &repo)
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("Configured Codex home 'work' was not found"),
        "{error}"
    );
    assert!(
        error.contains(&user_home.join(".codex-work").display().to_string()),
        "{error}"
    );
    assert!(
        error.contains("use an explicit path for a non-conventional home"),
        "{error}"
    );

    let conventional = user_home.join(".codex-work");
    fs::create_dir(&conventional).unwrap();
    assert_eq!(
        resolve_configured_home_from_dir(Path::new("work"), &repo).unwrap(),
        conventional.canonicalize().unwrap()
    );
}

#[test]
fn absolute_current_home_does_not_resolve_the_current_directory() {
    let temp = tempfile::tempdir().unwrap();
    let absolute = temp.path().join("codex-home");

    let resolved = absolute_path_with_current_dir(absolute.clone(), || -> Result<PathBuf> {
        panic!("absolute homes must not resolve the current directory")
    })
    .unwrap();

    assert_eq!(resolved, absolute);
}

#[test]
fn launch_home_resolution_reports_named_attempts() {
    let temp = tempfile::tempdir().unwrap();
    let user_home = temp.path().join("user");
    let current_dir = temp.path().join("repo");
    let default = user_home.join(".codex");
    let conventional = user_home.join(".codex-new");
    fs::create_dir_all(&default).unwrap();
    fs::create_dir_all(&conventional).unwrap();
    fs::create_dir_all(&current_dir).unwrap();
    let discovered = vec![default];

    assert_eq!(
        resolve_launch_home_from(Path::new("new"), &user_home, &discovered).unwrap(),
        conventional.canonicalize().unwrap()
    );

    let error = resolve_launch_home_from(Path::new("missing"), &user_home, &discovered)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Codex home 'missing' was not found"));
    assert!(error.contains(&user_home.join(".codex-missing").display().to_string()));
    assert!(!error.contains(&current_dir.join("missing").display().to_string()));
    assert!(error.contains("Discovered homes: codex"));
}

#[test]
fn logged_out_home_is_observed_without_becoming_an_inspection_error() {
    let response = AppServerAccountResponse {
        account: json!({ "account": null }),
        rate_limits: None,
        usage_error: None,
    };
    let report = assembled_home_report(
        PathBuf::from("/tmp/.codex-scratch"),
        Path::new("/tmp/.codex"),
        false,
        inspected_home_json(response, false),
    );
    let home = &report["homes"][0];

    assert!(report["errors"].as_array().unwrap().is_empty());
    assert_eq!(home["status"], "not logged in");
    assert!(home["inspection_error"].is_null());
    assert!(home["usage_error"].is_null());
}

#[test]
fn logged_out_usage_failure_is_not_a_report_error() {
    let response = AppServerAccountResponse {
        account: json!({ "account": null }),
        rate_limits: Some(json!({
            "rateLimits": {
                "limitId": "codex",
                "primary": { "usedPercent": 99, "windowDurationMins": 10080 }
            }
        })),
        usage_error: Some("usage unavailable".into()),
    };
    let report = assembled_home_report(
        PathBuf::from("/tmp/.codex-scratch"),
        Path::new("/tmp/.codex"),
        true,
        inspected_home_json(response, true),
    );
    let home = &report["homes"][0];

    assert!(report["errors"].as_array().unwrap().is_empty());
    assert_eq!(home["status"], "not logged in");
    assert_eq!(home["rate_limits"], json!([]));
    assert!(home["inspection_error"].is_null());
    assert!(home["usage_error"].is_null());
}

#[test]
fn logged_in_usage_failure_remains_a_report_error() {
    let response = AppServerAccountResponse {
        account: json!({
            "account": {
                "type": "chatgpt",
                "email": "person@example.com",
                "planType": "pro"
            }
        }),
        rate_limits: None,
        usage_error: Some("usage unavailable".into()),
    };
    let report = assembled_home_report(
        PathBuf::from("/tmp/.codex-work"),
        Path::new("/tmp/.codex"),
        true,
        inspected_home_json(response, true),
    );
    let home = &report["homes"][0];
    let errors = report["errors"].as_array().unwrap();

    assert_eq!(home["usage_error"], "usage unavailable");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["kind"], "usage");
}

#[test]
fn logged_in_semantically_empty_usage_payload_is_a_partial_report() {
    for rate_limits in [
        json!({}),
        json!({ "rateLimits": { "limitId": "codex" } }),
        json!({
            "rateLimitsByLimitId": {
                "codex": { "primary": {}, "secondary": null }
            }
        }),
    ] {
        let response = AppServerAccountResponse {
            account: json!({
                "account": {
                    "type": "chatgpt",
                    "email": "person@example.com",
                    "planType": "pro"
                }
            }),
            rate_limits: Some(rate_limits),
            usage_error: None,
        };
        let report = assembled_home_report(
            PathBuf::from("/tmp/.codex-work"),
            Path::new("/tmp/.codex"),
            true,
            inspected_home_json(response, true),
        );
        let home = &report["homes"][0];

        assert_eq!(report["outcome"], "partial");
        assert!(!rate_limits_have_usage_data(
            home["rate_limits"].as_array().unwrap()
        ));
        assert_eq!(
            home["usage_error"],
            "account/rateLimits/read returned no usage data"
        );
        assert_eq!(report["errors"][0]["kind"], "usage");
    }
}

#[cfg(unix)]
#[test]
fn signal_retirement_failure_retains_the_operation_error() {
    let error = finish_signal_supervised::<()>(
        Err(anyhow::anyhow!("picker drawing failed")),
        Err(std::io::Error::other("handler restoration failed")),
        "Codex home picker signal supervision could not retire safely",
    )
    .unwrap_err();
    let rendered = format!("{error:#}");

    assert!(
        rendered.contains("signal supervision could not retire safely"),
        "{rendered}"
    );
    assert!(rendered.contains("picker drawing failed"), "{rendered}");
}

fn assembled_home_report(
    home: PathBuf,
    current: &Path,
    include_usage: bool,
    inspected: JsonValue,
) -> JsonValue {
    homes_report_from_discovered(
        include_usage,
        DiscoveredHomes {
            paths: vec![home],
            errors: Vec::new(),
            representation_lossy: false,
        },
        current,
        OsStr::new("codex"),
        |_| inspected.clone(),
        |_, _, _| Ok(()),
    )
    .unwrap()
    .0
}

#[test]
fn homes_report_envelope_maps_complete_and_partial_outcomes() {
    let home = PathBuf::from("/tmp/.codex-work");
    let logged_out = || {
        inspected_home_json(
            AppServerAccountResponse {
                account: json!({ "account": null }),
                rate_limits: None,
                usage_error: None,
            },
            true,
        )
    };
    let (complete, paths) = homes_report_from_discovered(
        true,
        DiscoveredHomes {
            paths: vec![home.clone()],
            errors: Vec::new(),
            representation_lossy: false,
        },
        &home,
        OsStr::new("codex"),
        |_| logged_out(),
        |_, _, _| Ok(()),
    )
    .unwrap();

    assert_eq!(paths, vec![home.clone()]);
    assert_eq!(complete["schema_version"], 1);
    assert_eq!(complete["outcome"], "complete");
    assert_eq!(complete["usage_included"], true);
    assert_eq!(complete["representation_lossy"], false);
    assert_eq!(complete["homes"][0]["current"], true);
    assert!(complete["errors"].as_array().unwrap().is_empty());

    let (partial, _) = homes_report_from_discovered(
        true,
        DiscoveredHomes {
            paths: vec![home.clone()],
            errors: vec!["could not inspect one directory entry".into()],
            representation_lossy: false,
        },
        &home,
        OsStr::new("codex"),
        |_| inspection_failure("app-server unavailable"),
        |_, _, _| Ok(()),
    )
    .unwrap();

    assert_eq!(partial["outcome"], "partial");
    assert_eq!(partial["errors"][0]["kind"], "discovery");
    assert_eq!(partial["errors"][1]["kind"], "inspection");
    assert_eq!(partial["homes"][0]["usage_included"], true);
}

#[test]
fn picker_inspection_retains_nonfatal_discovery_warnings() {
    let inspection = CodexHomeInspection {
        discovered: DiscoveredHomes {
            paths: vec![PathBuf::from("/tmp/.codex")],
            errors: vec!["could not inspect one directory entry".into()],
            representation_lossy: false,
        },
        current: PathBuf::from("/tmp/.codex"),
        codex_bin: OsString::from("codex"),
    };

    assert_eq!(
        inspection.discovery_warnings(),
        ["could not inspect one directory entry"]
    );
}

#[test]
fn discovery_retains_candidate_metadata_errors() {
    let temp = tempfile::tempdir().unwrap();
    let user_home = temp.path();
    let default = user_home.join(".codex");
    let inaccessible = user_home.join(".codex-private");
    fs::create_dir_all(&default).unwrap();
    fs::create_dir_all(&inaccessible).unwrap();

    let discovered = discover_homes_from_with_metadata(user_home, &inaccessible, |path| {
        if path == inaccessible {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
        } else {
            fs::metadata(path)
        }
    });

    assert_eq!(discovered.paths, vec![default]);
    assert_eq!(discovered.errors.len(), 1);
    assert!(discovered.errors[0].contains(&inaccessible.display().to_string()));
    assert!(discovered.errors[0].contains("denied"));
}

#[test]
fn discovery_retains_current_home_when_user_home_scan_fails() {
    let temp = tempfile::tempdir().unwrap();
    let user_home = temp.path().join("user");
    let current = temp.path().join("current-home");
    fs::create_dir(&user_home).unwrap();
    fs::create_dir(&current).unwrap();

    let discovered = discover_homes_from_with_sources(
        &user_home,
        &current,
        |path: &Path| fs::metadata(path),
        |_, _| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "directory scan denied",
            ))
        },
    );
    assert_eq!(discovered.paths, vec![current]);
    assert!(
        discovered.errors.iter().any(|error| {
            error.contains(&user_home.display().to_string()) && error.contains("for Codex homes")
        }),
        "{:?}",
        discovered.errors
    );
}

#[test]
fn discovery_processes_directory_entries_as_the_source_yields_them() {
    let temp = tempfile::tempdir().unwrap();
    let user_home = temp.path().join("user");
    let work = user_home.join(".codex-work");
    let current = temp.path().join("current-home");
    for directory in [&user_home, &work, &current] {
        fs::create_dir_all(directory).unwrap();
    }
    let work_was_inspected = std::cell::Cell::new(false);

    let discovered = discover_homes_from_with_sources(
        &user_home,
        &current,
        |path| {
            if path == work {
                work_was_inspected.set(true);
            }
            fs::metadata(path)
        },
        |_, inspect_entry| {
            inspect_entry(Ok((OsString::from(".codex-work"), work.clone())));
            assert!(
                work_was_inspected.get(),
                "entry metadata should be inspected before the source yields another entry"
            );
            Ok(())
        },
    );

    assert_eq!(discovered.paths, vec![work, current]);
    assert!(discovered.errors.is_empty());
}

#[cfg(unix)]
#[test]
fn homes_report_marks_lossy_non_utf8_paths() {
    use std::os::unix::ffi::OsStringExt;

    let home = PathBuf::from(OsString::from_vec(b"/tmp/.codex-\xff".to_vec()));
    let (report, _) = homes_report_from_discovered(
        false,
        DiscoveredHomes {
            paths: vec![home.clone()],
            errors: Vec::new(),
            representation_lossy: false,
        },
        &home,
        OsStr::new("codex"),
        |_| {
            inspected_home_json(
                AppServerAccountResponse {
                    account: json!({ "account": null }),
                    rate_limits: None,
                    usage_error: None,
                },
                false,
            )
        },
        |_, _, _| Ok(()),
    )
    .unwrap();

    assert_eq!(report["representation_lossy"], true);
}

#[cfg(unix)]
#[test]
fn homes_report_marks_lossy_failed_discovery_candidates() {
    use std::os::unix::ffi::OsStringExt;

    let user_home = PathBuf::from("/tmp/jig-codex-discovery-test");
    let current = user_home.join("missing-current");
    let name = OsString::from_vec(b".codex-broken-\xff".to_vec());
    let candidate = user_home.join(&name);
    let candidate_entry = candidate.clone();
    let discovered = discover_homes_from_with_sources(
        &user_home,
        &current,
        |path| {
            if path == candidate {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "candidate denied",
                ))
            } else {
                Err(io::ErrorKind::NotFound.into())
            }
        },
        |_, inspect_entry| {
            inspect_entry(Ok((name, candidate_entry)));
            Ok(())
        },
    );

    assert!(discovered.paths.is_empty());
    assert!(discovered.representation_lossy);
    let (report, _) = homes_report_from_discovered(
        false,
        discovered,
        &current,
        OsStr::new("codex"),
        |_| unreachable!("failed candidates must not be inspected"),
        |_, _, _| Ok(()),
    )
    .unwrap();

    assert_eq!(report["representation_lossy"], true);
}

#[test]
fn malformed_account_results_are_inspection_errors_not_logged_out_accounts() {
    for account in [JsonValue::Null, json!({}), json!({ "account": "invalid" })] {
        let inspected = inspected_home_json(
            AppServerAccountResponse {
                account,
                rate_limits: None,
                usage_error: None,
            },
            false,
        );

        assert_eq!(inspected["status"], "unknown");
        assert!(inspected["inspection_error"].as_str().is_some());
    }
}

#[test]
fn inspection_failure_uses_a_stable_unknown_account_state() {
    let failure = inspection_failure("app-server unavailable");

    assert_eq!(failure["status"], "unknown");
    assert_eq!(failure["inspection_error"], "app-server unavailable");
    assert!(failure["usage_error"].is_null());
}

#[test]
fn dry_run_report_is_schema_versioned() {
    let report = dry_run_report(Path::new("/tmp/.codex-work"), &[]);

    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["representation_lossy"], false);
}

#[cfg(unix)]
#[test]
fn dry_run_report_marks_non_utf8_values_as_lossy() {
    use std::os::unix::ffi::OsStringExt;

    let argument = OsString::from_vec(vec![b'f', b'o', 0x80]);
    let report = dry_run_report(Path::new("/tmp/.codex-work"), &[argument]);

    assert_eq!(report["representation_lossy"], true);
}

#[test]
fn parallel_inspection_starts_new_work_before_a_slow_home_finishes() {
    let homes = (0..5)
        .map(|index| PathBuf::from(index.to_string()))
        .collect::<Vec<_>>();
    let fifth_started = Arc::new((Mutex::new(false), Condvar::new()));
    let mut progress = Vec::new();

    let inspected = inspect_homes_parallel(
        &homes,
        |home| {
            let index = home
                .to_string_lossy()
                .parse::<usize>()
                .expect("test home should contain its index");
            if index == 0 {
                let (started, wake) = &*fifth_started;
                let started = started.lock().unwrap();
                let (started, _) = wake
                    .wait_timeout_while(started, Duration::from_secs(1), |started| !*started)
                    .unwrap();
                assert!(
                    *started,
                    "the fifth inspection never entered the rolling pool"
                );
            } else if index == 4 {
                let (started, wake) = &*fifth_started;
                *started.lock().unwrap() = true;
                wake.notify_all();
            }
            json!({ "index": index })
        },
        |_, home| {
            progress.push(home["index"].as_u64().unwrap());
            Ok(())
        },
    )
    .unwrap();

    for (index, result) in inspected.iter().enumerate() {
        assert_eq!(result["index"], index);
    }
    assert_eq!(progress.len(), homes.len());
}

#[test]
fn panicking_inspection_is_enriched_before_progress_is_reported() {
    let homes = vec![PathBuf::from("/tmp/.codex-work")];
    let mut completed = Vec::new();

    let inspected = inspect_homes_parallel(
        &homes,
        |_| panic!("simulated inspection panic"),
        |index, home| {
            enrich_inspected_home(&homes[index], Path::new("/tmp/.codex"), true, home);
            completed.push(home.clone());
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0]["name"], "codex-work");
    assert_eq!(completed[0]["usage_included"], true);
    assert_eq!(inspected[0], completed[0]);
    assert_eq!(
        inspected[0]["inspection_error"],
        "Codex home inspection worker panicked"
    );
}

#[test]
fn discovery_finds_conventional_and_current_homes_and_ignores_other_directories() {
    let temp = tempfile::tempdir().unwrap();
    let user_home = temp.path().join("user");
    let default = user_home.join(".codex");
    let work = user_home.join(".codex-work");
    let unrelated = user_home.join("projects");
    let current = temp.path().join("current-home");
    for directory in [&default, &work, &unrelated, &current] {
        fs::create_dir_all(directory).unwrap();
    }

    let discovered = discover_homes_from(&user_home, &current);

    assert_eq!(discovered.paths, vec![default, work, current]);
    assert!(discovered.errors.is_empty());
}

#[test]
fn discovery_deduplicates_the_current_default_home() {
    let temp = tempfile::tempdir().unwrap();
    let user_home = temp.path().join("user");
    let default = user_home.join(".codex");
    fs::create_dir_all(&default).unwrap();

    let discovered = discover_homes_from(&user_home, &default);

    assert_eq!(discovered.paths, vec![default]);
}

#[cfg(unix)]
#[test]
fn home_helpers_preserve_non_utf8_names_and_tilde_suffixes() {
    use std::os::unix::ffi::OsStringExt;

    let temp = tempfile::tempdir().unwrap();
    let user_home = temp.path().join("user");

    let requested = OsString::from_vec(b"work-\xff".to_vec());
    let mut conventional_name = OsString::from(".codex-");
    conventional_name.push(&requested);
    assert_eq!(
        conventional_home(&user_home, &requested),
        user_home.join(conventional_name)
    );

    let suffix = OsString::from_vec(b"raw-\xfe".to_vec());
    let mut tilde_input = OsString::from("~/");
    tilde_input.push(&suffix);
    assert_eq!(
        expand_tilde_path(Path::new(&tilde_input), &user_home),
        Some(user_home.join(suffix))
    );
}

#[cfg(unix)]
#[test]
fn discovered_non_utf8_names_do_not_collide_through_lossy_display() {
    use std::os::unix::ffi::OsStringExt;

    let first_name = OsString::from_vec(b"work-\x80".to_vec());
    let second_name = OsString::from_vec(b"work-\x81".to_vec());
    let first = Path::new("/homes").join(&first_name);
    let second = Path::new("/homes").join(&second_name);

    assert!(!home_name_matches(&first, &second_name));
    assert!(home_name_matches(&second, &second_name));
}

#[test]
fn app_server_protocol_completes_handshake_before_account_requests() {
    let responses = concat!(
        "{\"id\":0,\"result\":{}}\n",
        "{\"method\":\"account/updated\",\"params\":{}}\n",
        "{\"id\":1,\"result\":{\"account\":{\"type\":\"chatgpt\",\"email\":\"person@example.com\",\"planType\":\"pro\"}}}\n",
        "{\"id\":2,\"result\":{\"rateLimitsByLimitId\":{}}}\n"
    );
    let mut reader = Cursor::new(responses.as_bytes());
    let mut requests = Vec::new();

    let response = app_server_protocol(&mut requests, &mut reader, true, None, &|| false).unwrap();

    assert_eq!(response.account["account"]["email"], "person@example.com");
    assert!(response.rate_limits.is_some());
    let messages = String::from_utf8(requests)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<JsonValue>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(messages[0]["method"], "initialize");
    assert_eq!(messages[1]["method"], "initialized");
    assert_eq!(messages[2]["method"], "account/read");
    assert_eq!(messages[3]["method"], "account/rateLimits/read");
    assert_eq!(messages[2]["params"]["refreshToken"], false);
}

#[test]
fn app_server_protocol_stops_before_writing_when_cancelled() {
    let mut reader = Cursor::new(Vec::<u8>::new());
    let mut requests = Vec::new();

    let error = app_server_protocol(&mut requests, &mut reader, true, None, &|| true).unwrap_err();

    assert_eq!(error, "Codex app-server inspection was cancelled");
    assert!(requests.is_empty());
}

#[test]
fn app_server_protocol_keeps_account_when_usage_is_unavailable() {
    let responses = concat!(
        "{\"id\":0,\"result\":{}}\n",
        "{\"id\":1,\"result\":{\"account\":{\"type\":\"apiKey\"}}}\n",
        "{\"id\":2,\"error\":{\"message\":\"rate limits require ChatGPT auth\"}}\n"
    );
    let mut reader = Cursor::new(responses.as_bytes());
    let mut requests = Vec::new();

    let response = app_server_protocol(&mut requests, &mut reader, true, None, &|| false).unwrap();

    assert_eq!(response.account["account"]["type"], "apiKey");
    assert!(response.rate_limits.is_none());
    assert_eq!(
        response.usage_error.as_deref(),
        Some("account/rateLimits/read failed: rate limits require ChatGPT auth")
    );
}

#[test]
fn app_server_protocol_keeps_account_when_usage_response_never_arrives() {
    let responses = concat!(
        "{\"id\":0,\"result\":{}}\n",
        "{\"id\":1,\"result\":{\"account\":{\"type\":\"chatgpt\",\"email\":\"person@example.com\"}}}\n"
    );
    let mut reader = Cursor::new(responses.as_bytes());
    let mut requests = Vec::new();

    let response = app_server_protocol(&mut requests, &mut reader, true, None, &|| false).unwrap();

    assert_eq!(response.account["account"]["email"], "person@example.com");
    assert!(response.rate_limits.is_none());
    assert!(
        response
            .usage_error
            .as_deref()
            .is_some_and(|error| error.contains("closed before returning"))
    );
}

#[test]
fn app_server_protocol_propagates_cancellation_after_account_response() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let source = CancelAfterEofReader {
        bytes: concat!(
            "{\"id\":0,\"result\":{}}\n",
            "{\"id\":1,\"result\":{\"account\":{\"type\":\"chatgpt\"}}}\n"
        )
        .as_bytes(),
        offset: 0,
        cancelled: Arc::clone(&cancelled),
    };
    let mut reader = BufReader::new(source);
    let mut requests = Vec::new();

    let error = app_server_protocol(&mut requests, &mut reader, true, None, &|| {
        cancelled.load(Ordering::Acquire)
    })
    .unwrap_err();

    assert_eq!(error, APP_SERVER_INSPECTION_CANCELLED);
}

#[test]
fn app_server_protocol_does_not_wait_for_usage_after_logged_out_account() {
    let responses = concat!(
        "{\"id\":0,\"result\":{}}\n",
        "{\"id\":1,\"result\":{\"account\":null}}\n"
    );
    let mut reader = Cursor::new(responses.as_bytes());
    let mut requests = Vec::new();

    let response = app_server_protocol(&mut requests, &mut reader, true, None, &|| false).unwrap();

    assert!(response.account["account"].is_null());
    assert!(response.rate_limits.is_none());
    assert!(response.usage_error.is_none());
}

#[test]
fn app_server_protocol_rejects_a_newline_free_oversized_message() {
    let oversized = vec![b'x'; APP_SERVER_PROTOCOL_MESSAGE_LIMIT + 1];
    let mut reader = Cursor::new(oversized);

    let error = read_next_response(&mut reader, None, &|| false).unwrap_err();

    assert_eq!(error, protocol_message_too_large());
}

#[test]
fn app_server_protocol_accepts_an_exact_limit_payload_plus_newline() {
    let mut message = br#"{"id":1}"#.to_vec();
    message.resize(APP_SERVER_PROTOCOL_MESSAGE_LIMIT, b' ');
    message.push(b'\n');
    let mut reader = Cursor::new(message);

    let response = read_next_response(&mut reader, None, &|| false).unwrap();

    assert_eq!(response["id"], 1);
}

#[test]
fn app_server_protocol_reports_eof_in_a_partial_protocol_line() {
    let mut reader = Cursor::new(br#"{\"id\":1"#.to_vec());

    let error = read_next_response(&mut reader, None, &|| false).unwrap_err();

    assert_eq!(error, "app-server closed before completing a protocol line");
}

#[test]
fn app_server_protocol_preserves_fragmented_nonblocking_utf8_lines() {
    let source = FragmentedNonblockingReader {
        bytes: "{\"id\":1,\"result\":\"é\"}\n".as_bytes(),
        offset: 0,
        would_block: false,
    };
    let mut reader = BufReader::with_capacity(4, source);

    let response = read_next_response(
        &mut reader,
        Instant::now().checked_add(Duration::from_secs(1)),
        &|| false,
    )
    .unwrap();

    assert_eq!(response["result"], "é");
}

#[test]
fn app_server_protocol_deadline_survives_continuous_irrelevant_messages() {
    let source = RepeatingReader {
        bytes: b"{\"method\":\"tick\"}\n",
        offset: 0,
    };
    let mut reader = BufReader::with_capacity(128, source);
    let started = Instant::now();

    let error = read_response(
        &mut reader,
        99,
        Instant::now().checked_add(Duration::from_millis(20)),
        &|| false,
    )
    .unwrap_err();

    assert_eq!(error, "Codex app-server protocol timed out");
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[cfg(unix)]
#[test]
fn app_server_client_interacts_with_and_cleans_up_a_long_lived_process_tree() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex-home");
    fs::create_dir(&home).unwrap();
    let stub = temp.path().join("codex-stub.sh");
    fs::write(
        &stub,
        r#"#!/bin/sh
read -r initialize
printf '%s\n' '{"id":0,"result":{}}'
read -r initialized
read -r account
read -r limits
printf '%s\n' '{"id":1,"result":{"account":{"type":"chatgpt","email":"stub@example.com","planType":"plus"}}}'
printf '%s\n' '{"id":2,"result":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":12,"windowDurationMins":300}}}}'
sleep 30
"#,
    )
    .unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();

    let response = app_server_account_with_timeout(
        &home,
        stub.as_os_str(),
        true,
        Duration::from_secs(2),
        &|| false,
    )
    .unwrap();

    assert_eq!(response.account["account"]["email"], "stub@example.com");
    assert_eq!(
        response.rate_limits.unwrap()["rateLimits"]["limitId"],
        "codex"
    );
}

#[cfg(unix)]
#[test]
fn app_server_client_surfaces_bounded_stderr_from_startup_failures() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex-home");
    fs::create_dir(&home).unwrap();
    let stub = temp.path().join("codex-stub.sh");
    fs::write(
        &stub,
        r#"#!/bin/sh
printf '%s\n' 'unsupported app-server subcommand' >&2
dd if=/dev/zero bs=1024 count=128 2>/dev/null | tr '\000' x >&2
exit 64
"#,
    )
    .unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    let started = Instant::now();

    let error = app_server_account_with_timeout(
        &home,
        stub.as_os_str(),
        false,
        Duration::from_secs(2),
        &|| false,
    )
    .unwrap_err();

    assert!(
        error.contains("unsupported app-server subcommand"),
        "{error}"
    );
    assert!(error.len() < 512, "stderr preview was not bounded: {error}");
    assert!(started.elapsed() < Duration::from_secs(1));
}
#[cfg(unix)]
#[test]
fn app_server_client_bounds_an_unresponsive_child() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex-home");
    fs::create_dir(&home).unwrap();
    let stub = temp.path().join("codex-stub.sh");
    fs::write(&stub, "#!/bin/sh\nsleep 30\n").unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();

    let error = app_server_account_with_timeout(
        &home,
        stub.as_os_str(),
        false,
        Duration::from_millis(50),
        &|| false,
    )
    .unwrap_err();

    assert!(error.contains("timed out"), "{error}");
}

#[cfg(unix)]
#[test]
fn app_server_client_cancels_during_a_live_inspection() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex-home");
    fs::create_dir(&home).unwrap();
    let stub = temp.path().join("codex-stub.sh");
    fs::write(&stub, "#!/bin/sh\nsleep 30\n").unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let started = Instant::now();

    let error = std::thread::scope(|scope| {
        let cancellation = Arc::clone(&cancelled);
        scope.spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            cancellation.store(true, Ordering::Release);
        });
        app_server_account_with_timeout(
            &home,
            stub.as_os_str(),
            false,
            Duration::from_secs(2),
            &|| cancelled.load(Ordering::Acquire),
        )
        .unwrap_err()
    });

    assert!(error.contains("cancelled"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(1));
}
