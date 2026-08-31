use std::collections::BTreeMap;
#[cfg(unix)]
use std::fs;
use std::path::Path;
#[cfg(unix)]
use std::process::Command;
#[cfg(unix)]
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use jig_contract::status_provider::v1::Input;
use serde_json::{Value, json};
use tempfile::tempdir;

use super::*;
use crate::test_env::TestRepoBuilder;

const PROVIDER_ID: &str = "example.test-status";

fn report_value(outcome: &str, revision: Option<&str>) -> Value {
    let inputs = revision
        .map(|revision| {
            vec![json!({
                "name": "target",
                "kind": "git",
                "revision": revision,
            })]
        })
        .unwrap_or_default();
    json!({
        "protocol": "jig.status-provider/v1",
        "provider": {
            "id": PROVIDER_ID,
            "adapter_version": "1.0.0"
        },
        "observed_at_ms": 1_785_142_200_000_u64,
        "outcome": outcome,
        "inputs": inputs,
        "work_packages": [{
            "id": "WP-0001",
            "title": "Test package",
            "specification": {
                "state": "ready",
                "category": "ready"
            },
            "implementation": {
                "state": "in_progress",
                "category": "active"
            },
            "verification": {
                "state": "unverified",
                "category": "pending"
            },
            "acceptance_checks": [{
                "ordinal": 1,
                "state": "covered",
                "category": "complete"
            }],
            "blockers": [{
                "code": "dependency_not_verified",
                "message": "A dependency is not verified"
            }]
        }],
        "diagnostics": []
    })
}

fn provider(argv: Vec<String>) -> StatusProviderConfig {
    StatusProviderConfig {
        id: PROVIDER_ID.into(),
        argv,
        timeout_seconds: 2,
    }
}

#[test]
fn decoder_preserves_additive_fields_and_builds_normalized_summary() {
    let mut value = report_value("complete", None);
    value["future_report_field"] = json!({"kept": true});
    value["provider"]["future_provider_field"] = json!("also kept");
    let decoded = decode_report(
        &provider(vec!["unused".into()]),
        &serde_json::to_string(&value).unwrap(),
    )
    .unwrap();

    assert_eq!(decoded.raw["future_report_field"]["kept"], true);
    assert_eq!(
        decoded.raw["provider"]["future_provider_field"],
        "also kept"
    );
    let summary = serde_json::to_value(ProviderSummary::from_report(&decoded.decoded)).unwrap();
    assert_eq!(summary["work_packages"], 1);
    assert_eq!(summary["work_packages_with_blockers"], 1);
    assert_eq!(summary["blockers"], 1);
    assert_eq!(summary["implementation"]["active"], 1);
    assert_eq!(summary["verification"]["pending"], 1);
    assert_eq!(summary["acceptance"]["complete"], 1);
}

#[test]
fn decoder_rejects_multiple_documents_identity_mismatch_and_semantic_errors() {
    let valid = serde_json::to_string(&report_value("complete", None)).unwrap();
    let multiple = decode_report(
        &provider(vec!["unused".into()]),
        &format!("{valid}\n{valid}"),
    )
    .unwrap_err();
    assert_eq!(multiple.code, "invalid_json");

    let mut mismatch = report_value("complete", None);
    mismatch["provider"]["id"] = json!("example.other");
    let mismatch = decode_report(
        &provider(vec!["unused".into()]),
        &serde_json::to_string(&mismatch).unwrap(),
    )
    .unwrap_err();
    assert_eq!(mismatch.code, "provider_id_mismatch");

    let mut invalid = report_value("complete", None);
    invalid["work_packages"][0]["acceptance_checks"][0]["ordinal"] = json!(0);
    let invalid = decode_report(
        &provider(vec!["unused".into()]),
        &serde_json::to_string(&invalid).unwrap(),
    )
    .unwrap_err();
    assert_eq!(invalid.code, "invalid_report");
    assert!(invalid.message.contains("validation error"));
}

#[cfg(unix)]
#[test]
fn runner_accepts_one_valid_document_and_never_trusts_nonzero_stdout() {
    let temp = tempdir().unwrap();
    let report = temp.path().join("report.json");
    fs::write(
        &report,
        serde_json::to_vec(&report_value("complete", None)).unwrap(),
    )
    .unwrap();

    let valid = provider(vec!["cat".into(), report.display().to_string()]);
    let output = run_provider_inner(temp.path(), &valid).unwrap();
    assert_eq!(output.decoded.provider.id, PROVIDER_ID);

    let read_only_git = provider(vec![
        "sh".into(),
        "-c".into(),
        "test \"$GIT_OPTIONAL_LOCKS\" = 0 && cat report.json".into(),
    ]);
    let output = run_provider_inner(temp.path(), &read_only_git).unwrap();
    assert_eq!(output.decoded.provider.id, PROVIDER_ID);

    let failed = provider(vec![
        "sh".into(),
        "-c".into(),
        "printf '{\"protocol\":\"jig.status-provider/v1\"}'; printf 'provider broke' >&2; exit 7"
            .into(),
    ]);
    let failure = run_provider_inner(temp.path(), &failed).unwrap_err();
    assert_eq!(failure.code, "exit_nonzero");
    assert_eq!(failure.exit_status, Some(7));
    assert_eq!(failure.stderr.as_deref(), Some("provider broke"));
}

#[cfg(unix)]
#[test]
fn aggregate_runs_independent_providers_concurrently_and_keeps_configured_order() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
[[status.providers]]
id = "example.provider-a"
argv = ["sh", "provider-a.sh"]
timeout_seconds = 3

[[status.providers]]
id = "example.provider-b"
argv = ["sh", "provider-b.sh"]
timeout_seconds = 3
"#,
        )
        .write();

    for (name, other, id) in [
        ("a", "b", "example.provider-a"),
        ("b", "a", "example.provider-b"),
    ] {
        let mut report = report_value("complete", None);
        report["provider"]["id"] = json!(id);
        fs::write(
            temp.path().join(format!("report-{name}.json")),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
        let script = temp.path().join(format!("provider-{name}.sh"));
        fs::write(
            &script,
            format!(
                "#!/bin/sh\ntouch provider-{name}.started\nwhile [ ! -f provider-{other}.started ]; do sleep 0.01; done\ncat report-{name}.json\n"
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(script, permissions).unwrap();
    }

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let runs = run_providers_concurrently(
        &ctx,
        &|| false,
        &mut crate::execution::NoopExecutionObserver,
    )
    .unwrap();

    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].id, "example.provider-a");
    assert_eq!(runs[1].id, "example.provider-b");
    assert!(runs.iter().all(|run| run.report.is_some()));
}

#[cfg(unix)]
#[test]
fn aggregate_refreshes_status_provider_authority_after_startup() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
[[status.providers]]
id = "factorish.old-status"
argv = ["cat", "old-report.json"]
timeout_seconds = 2
"#,
        )
        .write();
    let mut old_report = report_value("complete", None);
    old_report["provider"]["id"] = json!("factorish.old-status");
    fs::write(
        temp.path().join("old-report.json"),
        serde_json::to_vec(&old_report).unwrap(),
    )
    .unwrap();
    let mut new_report = report_value("complete", None);
    new_report["provider"]["id"] = json!("factorish.new-status");
    fs::write(
        temp.path().join("new-report.json"),
        serde_json::to_vec(&new_report).unwrap(),
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace("factorish.old-status", "factorish.new-status")
        .replace("old-report.json", "new-report.json");
    fs::write(config_path, config).unwrap();

    let snapshot = snapshot(&ctx).unwrap();

    assert_eq!(snapshot["providers"][0]["id"], "factorish.new-status");
    assert_eq!(snapshot["providers"][0]["status"], "complete");
}

#[derive(Default)]
struct LifecycleObserver {
    started: Vec<String>,
    finished: Vec<(String, bool)>,
}

impl ExecutionObserver for LifecycleObserver {
    fn event(&mut self, event: ExecutionEvent<'_>) {
        match event {
            ExecutionEvent::PhaseStarted { label, .. } => self.started.push(label.to_string()),
            ExecutionEvent::PhaseFinished { label, success, .. } => {
                self.finished.push((label.to_string(), success));
            }
            ExecutionEvent::Output { .. } | ExecutionEvent::Heartbeat { .. } => {}
        }
    }
}

fn scheduler_providers(count: usize) -> Vec<StatusProviderConfig> {
    (0..count)
        .map(|index| StatusProviderConfig {
            id: format!("example.provider-{index}"),
            argv: vec!["unused".into()],
            timeout_seconds: 1,
        })
        .collect()
}

fn scheduler_run(provider: &StatusProviderConfig, duration: Duration) -> ProviderRun {
    ProviderRun {
        id: provider.id.clone(),
        duration_ms: u64::try_from(duration.as_millis()).unwrap(),
        report: None,
        failure: None,
    }
}

#[test]
fn provider_scheduler_bounds_concurrency_and_preserves_order() {
    use std::sync::atomic::AtomicUsize;

    let providers = scheduler_providers(12);
    let active = AtomicUsize::new(0);
    let maximum = AtomicUsize::new(0);
    let mut observer = LifecycleObserver::default();
    let runs = run_provider_tasks(
        Path::new("."),
        &providers,
        &|| false,
        &mut observer,
        &|_, provider, _| {
            let concurrent = active.fetch_add(1, Ordering::SeqCst) + 1;
            maximum.fetch_max(concurrent, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(30));
            active.fetch_sub(1, Ordering::SeqCst);
            scheduler_run(provider, Duration::from_millis(30))
        },
    )
    .unwrap();

    assert_eq!(maximum.load(Ordering::SeqCst), STATUS_PROVIDER_CONCURRENCY);
    assert_eq!(
        runs.iter().map(|run| run.id.as_str()).collect::<Vec<_>>(),
        providers
            .iter()
            .map(|provider| provider.id.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(observer.started.len(), providers.len());
    assert_eq!(observer.finished.len(), providers.len());
}

#[test]
fn provider_scheduler_cancellation_leaves_queued_providers_unstarted() {
    use std::sync::atomic::AtomicUsize;

    let providers = scheduler_providers(12);
    let cancelled = AtomicBool::new(false);
    let started = AtomicUsize::new(0);
    let mut observer = LifecycleObserver::default();
    let error = run_provider_tasks(
        Path::new("."),
        &providers,
        &|| cancelled.load(Ordering::SeqCst),
        &mut observer,
        &|_, provider, _| {
            let index = started.fetch_add(1, Ordering::SeqCst);
            if index == 0 {
                cancelled.store(true, Ordering::SeqCst);
            }
            thread::sleep(Duration::from_millis(50));
            scheduler_run(provider, Duration::from_millis(50))
        },
    )
    .unwrap_err();

    assert!(is_status_collection_cancellation(&error));
    assert!(started.load(Ordering::SeqCst) <= STATUS_PROVIDER_CONCURRENCY);
    assert_eq!(observer.started.len(), observer.finished.len());
    assert!(observer.started.len() < providers.len());
}

#[test]
fn provider_scheduler_balances_a_panicked_phase() {
    let providers = scheduler_providers(2);
    let mut observer = LifecycleObserver::default();
    let error = run_provider_tasks(
        Path::new("."),
        &providers,
        &|| false,
        &mut observer,
        &|_, provider, cancelled| {
            if provider.id.ends_with("-0") {
                panic!("fixture provider panic");
            }
            while !cancelled() {
                thread::sleep(Duration::from_millis(1));
            }
            scheduler_run(provider, Duration::from_millis(1))
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("example.provider-0"));
    let mut started = observer.started;
    let mut finished = observer
        .finished
        .iter()
        .map(|(label, _)| label.clone())
        .collect::<Vec<_>>();
    started.sort();
    finished.sort();
    assert_eq!(started, finished);
    assert!(
        observer
            .finished
            .iter()
            .any(|(label, success)| label == "example.provider-0" && !success)
    );
}

#[test]
fn provider_scheduler_preserves_every_concurrent_panic_diagnostic() {
    let providers = scheduler_providers(STATUS_PROVIDER_CONCURRENCY);
    let barrier = std::sync::Barrier::new(STATUS_PROVIDER_CONCURRENCY);
    let mut observer = LifecycleObserver::default();

    let error = run_provider_tasks(
        Path::new("."),
        &providers,
        &|| false,
        &mut observer,
        &|_, provider, _| {
            barrier.wait();
            panic!("fixture panic from {}", provider.id);
        },
    )
    .unwrap_err()
    .to_string();

    for provider in &providers {
        assert!(error.contains(&provider.id), "{error}");
    }
    assert_eq!(observer.started.len(), providers.len());
    assert_eq!(observer.finished.len(), providers.len());
}

#[cfg(unix)]
#[test]
fn runner_maps_timeout_non_utf8_and_bounded_output_failures() {
    let mut timed_out = provider(vec!["sh".into(), "-c".into(), "sleep 3".into()]);
    timed_out.timeout_seconds = 1;
    let failure = run_provider_inner(tempdir().unwrap().path(), &timed_out).unwrap_err();
    assert_eq!(failure.code, "timed_out");

    let non_utf8 = provider(vec!["sh".into(), "-c".into(), "printf '\\377'".into()]);
    let failure = run_provider_inner(tempdir().unwrap().path(), &non_utf8).unwrap_err();
    assert_eq!(failure.code, "stdout_not_utf8");

    let oversized = provider(vec![
        "sh".into(),
        "-c".into(),
        "printf 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'".into(),
    ]);
    let failure =
        run_provider_inner_with_limits(tempdir().unwrap().path(), &oversized, 8, 64).unwrap_err();
    assert_eq!(failure.code, "stdout_limit_exceeded");
    assert!(failure.message.contains("8 byte limit"));
}

#[cfg(unix)]
#[test]
fn runner_cancels_an_in_flight_provider_tree_promptly() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let trigger = Arc::clone(&cancelled);
    let setter = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        trigger.store(true, Ordering::SeqCst);
    });
    let long_running = provider(vec!["sh".into(), "-c".into(), "sleep 30 & wait".into()]);
    let started = Instant::now();
    let failure =
        run_provider_inner_with_cancellation(tempdir().unwrap().path(), &long_running, &|| {
            cancelled.load(Ordering::SeqCst)
        })
        .unwrap_err();
    setter.join().unwrap();

    assert_eq!(failure.code, "cancelled");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "provider cancellation took {:?}",
        started.elapsed()
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn repository_observation_cancels_an_in_flight_git_status_promptly() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempdir().unwrap();
    fs::write(root.path().join("README.md"), "fixture").unwrap();
    init_git_repo(root.path());
    let hook = root.path().join("slow-fsmonitor.sh");
    let started_marker = root.path().join("fsmonitor-started");
    fs::write(
        &hook,
        format!(
            "#!/bin/sh\ntouch '{}'\nsleep 30\n",
            started_marker.display()
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&hook).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&hook, permissions).unwrap();
    git(
        root.path(),
        &["config", "core.fsmonitor", &hook.display().to_string()],
    );

    let cancelled = Arc::new(AtomicBool::new(false));
    let trigger = Arc::clone(&cancelled);
    let marker = started_marker;
    let setter = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !marker.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            marker.exists(),
            "git status did not start the fsmonitor hook"
        );
        trigger.store(true, Ordering::SeqCst);
    });

    let started = Instant::now();
    let result =
        observe_git_checkout_with_cancellation(root.path(), &|| cancelled.load(Ordering::SeqCst));
    setter.join().unwrap();

    assert!(matches!(result, Err(GitProbeError::Cancelled)));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "Git observation cancellation took {:?}",
        started.elapsed()
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn aggregate_cancels_an_open_plan_gate_fingerprint_git_process_promptly() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempdir().unwrap();
    TestRepoBuilder::new(root.path())
        .repo_name("cancelled-gate-fingerprint-fixture")
        .config(
            r"
sqlx_enabled = false
",
        )
        .required_commands(["bootstrap_command"])
        .write();
    init_git_repo(root.path());

    let ctx = RepoContext::load_from(root.path()).unwrap();
    crate::state::seed_open_plan_for_test(&ctx, "plan_1", "Open plan", "# Open plan\n").unwrap();

    let hook = root.path().join(".agent/slow-gate-fsmonitor.sh");
    let count = root.path().join(".agent/fsmonitor-count");
    let started_marker = root.path().join(".agent/gate-fingerprint-started");
    fs::write(
        &hook,
        format!(
            "#!/bin/sh\ncount=0\nif test -f '{count}'; then read count < '{count}'; fi\ncount=$((count + 1))\nprintf '%s' \"$count\" > '{count}'\nif test \"$count\" -ge 2; then touch '{started}'; sleep 30; fi\n",
            count = count.display(),
            started = started_marker.display(),
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&hook).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&hook, permissions).unwrap();
    git(
        root.path(),
        &["config", "core.fsmonitor", &hook.display().to_string()],
    );

    let cancelled = Arc::new(AtomicBool::new(false));
    let trigger = Arc::clone(&cancelled);
    let marker = started_marker;
    let setter = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !marker.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            marker.exists(),
            "the open-plan gate fingerprint did not start its Git status probe"
        );
        trigger.store(true, Ordering::SeqCst);
    });

    let started = Instant::now();
    let error = snapshot_with_cancellation(&ctx, &|| cancelled.load(Ordering::SeqCst)).unwrap_err();
    setter.join().unwrap();

    assert_eq!(error.to_string(), "status collection was cancelled");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "gate fingerprint cancellation took {:?}",
        started.elapsed()
    );
    assert!(
        fs::read_to_string(count)
            .unwrap()
            .trim()
            .parse::<u64>()
            .unwrap()
            >= 2
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn aggregate_stops_after_provider_cancellation_without_post_provider_collection() {
    let root = tempdir().unwrap();
    TestRepoBuilder::new(root.path())
        .repo_name("cancelled-status-fixture")
        .config(format!(
            r#"
sqlx_enabled = false

[[status.providers]]
id = "{PROVIDER_ID}"
argv = ["sh", "provider.sh"]
timeout_seconds = 2
"#
        ))
        .required_commands(["bootstrap_command"])
        .write();
    fs::write(
        root.path().join("provider-report.json"),
        serde_json::to_vec(&report_value("complete", None)).unwrap(),
    )
    .unwrap();
    fs::write(
        root.path().join("provider.sh"),
        "#!/bin/sh\nprintf '%s' \"$$\" > provider.pid\ncat provider-report.json\ntouch provider-finished\n",
    )
    .unwrap();

    let ctx = RepoContext::load_from(root.path()).unwrap();
    let cancelled_after_provider_exit = || {
        if !root.path().join("provider-finished").exists() {
            return false;
        }
        let pid = fs::read_to_string(root.path().join("provider.pid"))
            .unwrap()
            .parse::<libc::pid_t>()
            .unwrap();
        // The provider writes its completion marker before exiting. While the
        // provider runner is still polling or owns an unreaped zombie, signal
        // zero succeeds. Cancellation therefore becomes true only after the
        // provider runner has observed and reaped the successful child.
        let result = unsafe { libc::kill(pid, 0) };
        result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    };
    let error = snapshot_with_cancellation(&ctx, &cancelled_after_provider_exit).unwrap_err();

    assert_eq!(error.to_string(), "status collection was cancelled");
    // This fixture is deliberately not a Git checkout. Reaching the
    // post-provider probes would produce a partial snapshot instead.
    assert!(!root.path().join(".agent/state").exists());
}

#[test]
fn work_snapshot_propagates_a_non_sticky_typed_cancellation() {
    use std::cell::Cell;

    let root = tempdir().unwrap();
    TestRepoBuilder::new(root.path())
        .repo_name("typed-cancellation-fixture")
        .config(
            r"
sqlx_enabled = false
",
        )
        .required_commands(["bootstrap_command"])
        .write();
    let ctx = RepoContext::load_from(root.path()).unwrap();
    let calls = Cell::new(0);

    let result = work_snapshot(&ctx, &|| {
        let current = calls.get();
        calls.set(current + 1);
        current == 1
    });
    let Err(error) = result else {
        panic!("non-sticky cancellation was converted into a partial work snapshot")
    };

    assert!(is_status_collection_cancellation(&error));
    assert_eq!(calls.get(), 2);
}

#[cfg(unix)]
#[test]
fn aggregate_joins_provider_git_work_gate_and_loop_state_without_writes() {
    let outer = tempdir().unwrap();
    let root = outer.path().join("repo");
    let report_path = root.join("provider-report.json");
    TestRepoBuilder::new(&root)
        .repo_name("status-fixture")
        .config(format!(
            r#"
sqlx_enabled = false

[[status.providers]]
id = "{PROVIDER_ID}"
argv = ["cat", "provider-report.json"]
timeout_seconds = 2
"#
        ))
        .required_commands(["bootstrap_command"])
        .write();
    fs::write(root.join(".gitignore"), "provider-report.json\n").unwrap();
    init_git_repo(&root);
    let original_revision = git_text_for_test(&root, &["rev-parse", "HEAD"]);
    fs::write(
        &report_path,
        serde_json::to_vec(&report_value("complete", Some(&original_revision))).unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(&root).unwrap();
    let current = snapshot(&ctx).unwrap();
    assert_eq!(current["outcome"], "complete");
    assert_eq!(current["repository"]["dirty"], false);
    assert_eq!(current["work"]["state"]["counts"]["open_plans"], 0);
    assert_eq!(current["work"]["gates"], json!([]));
    assert!(current["loops"]["leases"].as_array().unwrap().is_empty());
    assert_eq!(current["providers"][0]["status"], "complete");
    assert_eq!(
        current["providers"][0]["input_freshness"][0]["status"],
        "current"
    );
    // A domain blocker is a trustworthy fact, not a collection failure.
    assert_eq!(current["providers"][0]["summary"]["blockers"], 1);
    assert!(!root.join(".agent/state").exists());

    fs::write(root.join("untracked.txt"), "local change").unwrap();
    let dirty = snapshot(&ctx).unwrap();
    assert_eq!(dirty["outcome"], "complete");
    assert_eq!(dirty["repository"]["dirty"], true);
    assert_eq!(
        dirty["providers"][0]["input_freshness"][0]["status"],
        "dirty"
    );

    git(&root, &["add", "untracked.txt"]);
    git(&root, &["commit", "-m", "advance target"]);
    let stale = snapshot(&ctx).unwrap();
    assert_eq!(stale["repository"]["dirty"], false);
    assert_eq!(
        stale["providers"][0]["input_freshness"][0]["status"],
        "stale"
    );
    assert_eq!(stale["outcome"], "complete");

    let partial_report = report_value(
        "partial",
        Some(&git_text_for_test(&root, &["rev-parse", "HEAD"])),
    );
    fs::write(&report_path, serde_json::to_vec(&partial_report).unwrap()).unwrap();
    let partial = snapshot(&ctx).unwrap();
    assert_eq!(partial["providers"][0]["status"], "partial");
    assert_eq!(partial["outcome"], "partial");

    fs::write(&report_path, "not JSON").unwrap();
    let failed = snapshot(&ctx).unwrap();
    assert_eq!(failed["providers"][0]["status"], "failed");
    assert_eq!(failed["providers"][0]["report"], Value::Null);
    assert_eq!(failed["providers"][0]["error"]["code"], "invalid_json");
    assert_eq!(failed["outcome"], "partial");
    assert!(!root.join(".agent/state").exists());
}

#[test]
fn non_git_inputs_are_explicitly_not_applicable() {
    let input = Input::new("catalog", "document_catalog");
    let freshness = input_freshness(Path::new("."), &input, &mut BTreeMap::new());
    let value = serde_json::to_value(freshness).unwrap();

    assert_eq!(value["status"], "not_applicable");
    assert_eq!(value["observed_revision"], Value::Null);
}

#[cfg(unix)]
#[test]
fn nested_git_inputs_use_the_reported_repository_relative_checkout() {
    let root = tempdir().unwrap();
    let legacy = root.path().join("legacy/hocr");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("README.md"), "legacy fixture").unwrap();
    init_git_repo(&legacy);
    let revision = git_text_for_test(&legacy, &["rev-parse", "HEAD"]);
    let mut input = Input::new("legacy", "git");
    input.path = Some("legacy/hocr".into());
    input.revision = Some(revision.clone());

    let current = input_freshness(root.path(), &input, &mut BTreeMap::new());
    assert_eq!(current.status, "current");
    assert_eq!(
        current.observed_revision.as_deref(),
        Some(revision.as_str())
    );

    fs::write(legacy.join("untracked.txt"), "change").unwrap();
    let dirty = input_freshness(root.path(), &input, &mut BTreeMap::new());
    assert_eq!(dirty.status, "dirty");
}

#[cfg(unix)]
fn init_git_repo(root: &Path) {
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "fixture@example.com"]);
    git(root, &["config", "user.name", "Fixture"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial fixture"]);
}

#[cfg(unix)]
fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
fn git_text_for_test(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}
