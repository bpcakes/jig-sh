use std::cell::Cell;
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::json;

use super::*;
use crate::test_env::{EnvVarGuard, lock_env};

#[test]
fn issue_reads_use_private_store_snapshots() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let store_before = tree_identity(&fixture.root.join(".beads"));
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "database_read");

    let mut never_cancelled = || false;
    adapter.show_issue(ISSUE_ID, &mut never_cancelled).unwrap();
    adapter
        .list_comments(ISSUE_ID, &mut never_cancelled)
        .unwrap();

    assert_eq!(tree_identity(&fixture.root.join(".beads")), store_before);
    for args in logged_argument_groups(&fixture.log).iter().skip(2) {
        let database = args
            .windows(2)
            .find(|pair| pair[0] == "--db")
            .map(|pair| pair[1].as_str())
            .unwrap();
        assert!(database.contains("jig-tracker-store-"));
        assert!(database.ends_with("/beads.db"));
    }
}

#[test]
fn hard_linked_database_sidecar_is_rejected_before_a_read_process() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let wal = fixture.root.join(".beads/beads.db-wal");
    fs::write(&wal, b"wal fixture").unwrap();
    fs::hard_link(&wal, fixture._temp.path().join("outside-wal-alias")).unwrap();
    let mut never_cancelled = || false;

    assert_eq!(
        adapter
            .show_issue(ISSUE_ID, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::InvalidWorkspace {
            reason: InvalidWorkspaceReason::HardLinkedAuthority
        }
    );
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);
}

#[test]
fn provider_resolution_ignores_cwd_dependent_and_repository_owned_path_entries() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let repository_provider = fixture.root.join("br");
    let relative_directory = fixture.root.join("relative");
    fs::create_dir(&relative_directory).unwrap();
    let marker = fixture._temp.path().join("repository-provider-ran");
    let repository_script = format!(
        "#!/bin/sh\n: > {}\nprintf '%s\\n' '{{\"version\":\"0.5.7\"}}'\n",
        shell_quote(&marker.to_string_lossy())
    );
    write_executable(&repository_provider, &repository_script);
    write_executable(&relative_directory.join("br"), &repository_script);
    let search_path = std::env::join_paths([
        PathBuf::new(),
        PathBuf::from("relative"),
        fixture.root.clone(),
        relative_directory,
        fixture.bin.clone(),
    ])
    .unwrap();
    let _path = EnvVarGuard::set("PATH", search_path);
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");

    let (_, discovery) = fixture.discover(TrackerProcessPolicy::default());

    assert_eq!(discovery.profile, TrackerProfile::Beads0_5_7);
    assert!(!marker.exists());
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);
}

#[test]
fn provider_resolution_skips_an_invalid_external_candidate() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let invalid_bin = fixture._temp.path().join("invalid-bin");
    fs::create_dir(&invalid_bin).unwrap();
    let invalid_candidate = invalid_bin.join("br");
    write_executable(&invalid_candidate, "#!/bin/sh\nexit 99\n");
    fs::OpenOptions::new()
        .write(true)
        .open(&invalid_candidate)
        .unwrap()
        .set_len(process::MAX_EXECUTABLE_BYTES + 1)
        .unwrap();
    let search_path = std::env::join_paths([invalid_bin, fixture.bin.clone()]).unwrap();
    let _path = EnvVarGuard::set("PATH", search_path);
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");

    let (_, discovery) = fixture.discover(TrackerProcessPolicy::default());

    assert_eq!(discovery.profile, TrackerProfile::Beads0_5_7);
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);
}

#[test]
fn success_response_shapes_reject_conflicting_error_members() {
    let error = json!({
        "code": "POLICY_VIOLATION",
        "message": "contradictory provider response",
        "hint": null,
        "retryable": false,
        "context": null
    });
    let version = json!({"version": "0.5.7", "error": error});
    assert_eq!(
        profile_0_5_7::parse_version(&version).unwrap_err(),
        TrackerError::UnsupportedResponse {
            operation: TrackerOperation::Version
        }
    );

    let info = json!({
        "path": ".beads",
        "database_path": ".beads/beads.db",
        "jsonl_path": ".beads/issues.jsonl",
        "error": error
    });
    assert_eq!(
        profile_0_5_7::parse_info(&info).unwrap_err(),
        TrackerError::UnsupportedResponse {
            operation: TrackerOperation::Info
        }
    );

    let issue = json!([{
        "id": ISSUE_ID,
        "title": "Generic task",
        "description": "Do the work",
        "acceptance_criteria": "The result is bounded",
        "status": "open",
        "assignee": null,
        "updated_at": "2026-01-01T00:00:00Z",
        "error": error
    }]);
    assert_eq!(
        profile_0_5_7::parse_issue(&issue, WORKSPACE_ID, ISSUE_ID).unwrap_err(),
        TrackerError::UnsupportedResponse {
            operation: TrackerOperation::ShowIssue
        }
    );
}

#[test]
fn oversized_mutation_arguments_fail_before_readiness_or_spawn() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let mut never_cancelled = || false;

    assert_eq!(
        adapter
            .claim_issue(
                ISSUE_ID,
                &"a".repeat(MAX_TRACKER_ACTOR_BYTES + 1),
                &mut never_cancelled,
            )
            .unwrap_err(),
        TrackerError::InvalidInput { field: "actor" }
    );
    assert_eq!(
        adapter
            .add_comment(
                ISSUE_ID,
                ACTOR,
                &"m".repeat(MAX_TRACKER_MUTATION_TEXT_BYTES + 1),
                &mut never_cancelled,
            )
            .unwrap_err(),
        TrackerError::InvalidInput { field: "comment" }
    );
    assert_eq!(
        adapter
            .close_issue(
                ISSUE_ID,
                ACTOR,
                &"r".repeat(MAX_TRACKER_MUTATION_TEXT_BYTES + 1),
                &mut never_cancelled,
            )
            .unwrap_err(),
        TrackerError::InvalidInput {
            field: "close reason"
        }
    );
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);
}

#[test]
fn private_snapshot_retries_transient_wal_changes_and_omits_shm() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let wal = fixture.root.join(".beads/beads.db-wal");
    let shm = fixture.root.join(".beads/beads.db-shm");
    fs::write(&wal, b"initial wal").unwrap();
    fs::write(&shm, b"disposable shared memory").unwrap();
    let attempts = Rc::new(Cell::new(0));
    let hook_attempts = Rc::clone(&attempts);
    let _hook = process::TestStoreSnapshotHook::set(move |database| {
        let attempt = hook_attempts.get() + 1;
        hook_attempts.set(attempt);
        if attempt == 1 {
            let mut wal = database.as_os_str().to_os_string();
            wal.push("-wal");
            fs::write(PathBuf::from(wal), b"concurrent wal generation").unwrap();
        }
    });
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "shm_not_copied");
    let mut never_cancelled = || false;

    assert_eq!(
        adapter
            .show_issue(ISSUE_ID, &mut never_cancelled)
            .unwrap()
            .id,
        ISSUE_ID
    );
    assert_eq!(attempts.get(), 2);

    drop(_hook);
    attempts.set(0);
    let hook_attempts = Rc::clone(&attempts);
    let _busy_hook = process::TestStoreSnapshotHook::set(move |database| {
        hook_attempts.set(hook_attempts.get() + 1);
        let mut wal = database.as_os_str().to_os_string();
        wal.push("-wal");
        use std::io::Write as _;
        fs::OpenOptions::new()
            .append(true)
            .open(PathBuf::from(wal))
            .unwrap()
            .write_all(b"x")
            .unwrap();
    });
    assert_eq!(
        adapter
            .show_issue(ISSUE_ID, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::StoreChangedDuringSnapshot
    );
    assert_eq!(attempts.get(), 3);
}

#[test]
fn private_snapshot_timeout_is_distinct_from_provider_timeout() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let policy = TrackerProcessPolicy {
        read_timeout: Duration::from_millis(5),
        ..TrackerProcessPolicy::default()
    };
    let (adapter, _) = fixture.discover(policy);
    let _hook = process::TestStoreSnapshotHook::set(|_| {
        thread::sleep(Duration::from_millis(20));
    });
    let mut never_cancelled = || false;

    assert_eq!(
        adapter
            .show_issue(ISSUE_ID, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::StoreSnapshotTimedOut
    );
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);
}

#[test]
fn readiness_materializes_an_absent_or_replaced_jsonl_export() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let jsonl = fixture.root.join(".beads/issues.jsonl");
    fs::remove_file(&jsonl).unwrap();
    let mut never_cancelled = || false;

    adapter
        .check_storage_readiness(&mut never_cancelled)
        .unwrap();

    fs::write(&jsonl, b"new export generation\n").unwrap();
    adapter
        .check_storage_readiness(&mut never_cancelled)
        .unwrap();
}

#[test]
fn malformed_or_profile_inconsistent_mutation_errors_are_indeterminate() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let mut never_cancelled = || false;

    for mode in [
        "conflicting_error",
        "retryability_mismatch",
        "malformed_error",
        "combined_error",
    ] {
        let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", mode);
        assert_eq!(
            adapter
                .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
                .unwrap_err(),
            TrackerError::IndeterminateWrite {
                operation: TrackerOperation::ClaimIssue
            }
        );
    }

    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "ambiguous_id");
    assert_eq!(
        adapter
            .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::AmbiguousIssueId {
            issue_id: ISSUE_ID.into()
        }
    );
    drop(_mode);

    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "close_incomplete_error");
    assert_eq!(
        adapter
            .close_issue(ISSUE_ID, ACTOR, "Done", &mut never_cancelled)
            .unwrap_err(),
        TrackerError::IndeterminateWrite {
            operation: TrackerOperation::CloseIssue
        }
    );

    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "close_conflicting_noop");
    assert_eq!(
        adapter
            .close_issue(ISSUE_ID, ACTOR, "Done", &mut never_cancelled)
            .unwrap_err(),
        TrackerError::IndeterminateWrite {
            operation: TrackerOperation::CloseIssue
        }
    );
}

#[test]
fn comment_lists_reject_items_that_also_report_provider_errors() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "comments_error");
    let mut never_cancelled = || false;

    assert_eq!(
        adapter
            .list_comments(ISSUE_ID, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::UnsupportedResponse {
            operation: TrackerOperation::ListComments
        }
    );
}

#[test]
fn successful_mutations_require_exclusive_observed_response_shapes() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let mut never_cancelled = || false;

    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "success_stderr");
    assert_eq!(
        adapter
            .add_comment(
                ISSUE_ID,
                ACTOR,
                "Marker; $(touch should-not-exist)\nsecond line",
                &mut never_cancelled,
            )
            .unwrap_err(),
        TrackerError::IndeterminateWrite {
            operation: TrackerOperation::AddComment
        }
    );

    for mode in ["success_combined_error", "success_wrapper"] {
        let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", mode);
        assert_eq!(
            adapter
                .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
                .unwrap_err(),
            TrackerError::IndeterminateWrite {
                operation: TrackerOperation::ClaimIssue
            }
        );
    }

    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "success_item_error");
    assert_eq!(
        adapter
            .close_issue(ISSUE_ID, ACTOR, "Done", &mut never_cancelled)
            .unwrap_err(),
        TrackerError::IndeterminateWrite {
            operation: TrackerOperation::CloseIssue
        }
    );
}

#[test]
fn manual_pending_readiness_requires_one_degraded_db_newer_anomaly() {
    let duplicate = sync_status(false, true, false, "degraded", &["db_newer", "db_newer"]);
    let mut wrong_severity = sync_status(false, true, false, "degraded", &["db_newer"]);
    wrong_severity["reliability_audit"]["anomalies"][0]["severity"] =
        serde_json::Value::String("healthy".into());

    for rejected in [duplicate, wrong_severity] {
        assert_eq!(
            profile_0_5_7::require_mutation_ready(&rejected),
            Err(TrackerError::StaleStorage)
        );
    }
}

#[test]
fn contradictory_readiness_stops_before_the_mutation_process() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "sync_conflicting_error");

    let mut never_cancelled = || false;
    assert_eq!(
        adapter
            .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::UnsupportedResponse {
            operation: TrackerOperation::SyncStatus
        }
    );
    assert!(
        logged_argument_groups(&fixture.log)
            .iter()
            .all(|args| !args.iter().any(|arg| arg == "update"))
    );
}

#[test]
fn option_looking_actor_is_passed_as_one_option_value() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "option_actor");

    let mut never_cancelled = || false;
    let claimed = adapter
        .claim_issue(ISSUE_ID, "--force", &mut never_cancelled)
        .unwrap();
    assert_eq!(claimed.assignee.as_deref(), Some("--force"));
    let arguments = logged_argument_groups(&fixture.log);
    let arguments = arguments.last().unwrap();
    assert!(arguments.iter().any(|arg| arg == "--actor=--force"));
    assert!(!arguments.iter().any(|arg| arg == "--force"));
}

#[test]
fn tracker_launch_strips_dynamic_loader_injection_environment() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let _ld_audit = EnvVarGuard::set("LD_AUDIT", fixture._temp.path().join("audit.so"));
    let _ld_library = EnvVarGuard::set("LD_LIBRARY_PATH", fixture._temp.path().join("lib"));
    let _ld_preload = EnvVarGuard::set("LD_PRELOAD", fixture._temp.path().join("preload.so"));
    let _dyld_insert = EnvVarGuard::set(
        "DYLD_INSERT_LIBRARIES",
        fixture._temp.path().join("insert.dylib"),
    );
    let _dyld_library =
        EnvVarGuard::set("DYLD_LIBRARY_PATH", fixture._temp.path().join("dyld-lib"));
    let _dyld_versioned_library = EnvVarGuard::set(
        "DYLD_VERSIONED_LIBRARY_PATH",
        fixture._temp.path().join("dyld-versioned-lib"),
    );
    let _dyld_versioned_framework = EnvVarGuard::set(
        "DYLD_VERSIONED_FRAMEWORK_PATH",
        fixture._temp.path().join("dyld-versioned-framework"),
    );

    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let mut never_cancelled = || false;
    assert_eq!(
        adapter
            .show_issue(ISSUE_ID, &mut never_cancelled)
            .unwrap()
            .id,
        ISSUE_ID
    );
}

#[test]
fn tracker_launch_strips_shell_startup_and_exported_function_environment() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let executable = fixture.bin.join("br");
    let script = fs::read_to_string(&executable).unwrap().replacen(
        "#!/bin/sh\n",
        r#"#!/bin/bash
if [ -n "${BASH_ENV+x}" ] || [ -n "${ENV+x}" ] || [ -n "${CDPATH+x}" ] || [ -n "${BASH_XTRACEFD+x}" ] || declare -F jig_tracker_poison >/dev/null; then exit 82; fi
case "$-" in *x*|*v*) exit 83 ;; esac
shopt -q extglob && exit 84
case "$PS4" in *JIG_TRACKER_PS4_POISON*) exit 85 ;; esac
[ "${JIG_TRACKER_TEST_ORDINARY-}" = preserved ] || exit 86
"#,
        1,
    );
    write_executable(&executable, &script);
    let startup_marker = fixture._temp.path().join("shell-startup-poison-ran");
    let trace_marker = fixture._temp.path().join("shell-trace-poison-ran");
    let startup = fixture._temp.path().join("shell-startup-poison.sh");
    fs::write(
        &startup,
        "printf poison > \"$JIG_TRACKER_STARTUP_MARKER\"\n",
    )
    .unwrap();
    let _br = TestBrOverride::set(Some(&executable));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let _bash_env = EnvVarGuard::set("BASH_ENV", &startup);
    let _env_startup = EnvVarGuard::set("ENV", &startup);
    let _cdpath = EnvVarGuard::set("CDPATH", fixture._temp.path());
    let _shellopts = EnvVarGuard::set("SHELLOPTS", "xtrace:verbose");
    let _bashopts = EnvVarGuard::set("BASHOPTS", "extglob");
    let _ps4 = EnvVarGuard::set(
        "PS4",
        "JIG_TRACKER_PS4_POISON$(printf poison > \"$JIG_TRACKER_TRACE_MARKER\")",
    );
    let _xtracefd = EnvVarGuard::set("BASH_XTRACEFD", "2");
    let _function = EnvVarGuard::set(
        "BASH_FUNC_jig_tracker_poison%%",
        "() { printf poison > \"$JIG_TRACKER_STARTUP_MARKER\"; }",
    );
    let _startup_marker = EnvVarGuard::set("JIG_TRACKER_STARTUP_MARKER", &startup_marker);
    let _trace_marker = EnvVarGuard::set("JIG_TRACKER_TRACE_MARKER", &trace_marker);
    let _ordinary = EnvVarGuard::set("JIG_TRACKER_TEST_ORDINARY", "preserved");

    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let mut never_cancelled = || false;
    assert_eq!(
        adapter
            .show_issue(ISSUE_ID, &mut never_cancelled)
            .unwrap()
            .id,
        ISSUE_ID
    );
    assert!(!startup_marker.exists());
    assert!(!trace_marker.exists());
}

#[test]
fn migration_state_is_preserved_in_the_private_readiness_snapshot() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let marker_path = fixture.root.join(".beads/beads.db.fsqlite-migration-state");
    let marker = fs::File::create(&marker_path).unwrap();
    fs::write(&marker_path, b"migration-complete\n").unwrap();
    let modified = std::time::SystemTime::now()
        .checked_sub(Duration::from_secs(75))
        .unwrap();
    marker
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    let modified_seconds = marker
        .metadata()
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    let _expected_mtime = EnvVarGuard::set("JIG_TRACKER_TEST_MIGRATION_MTIME", modified_seconds);

    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "migration_state");
    let mut never_cancelled = || false;
    adapter
        .check_storage_readiness(&mut never_cancelled)
        .unwrap();
    assert_eq!(logged_argument_groups(&fixture.log).len(), 3);
}

#[test]
fn repository_local_temporary_directory_is_rejected_before_launch() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let store_before = tree_identity(&fixture.root.join(".beads"));
    let _tmpdir = EnvVarGuard::set("TMPDIR", fixture.root.join(".beads"));

    let mut never_cancelled = || false;
    assert_eq!(
        adapter
            .check_storage_readiness(&mut never_cancelled)
            .unwrap_err(),
        TrackerError::UnsafeTemporaryDirectory {
            operation: TrackerOperation::SyncStatus
        }
    );
    assert_eq!(tree_identity(&fixture.root.join(".beads")), store_before);
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);
}

#[test]
fn stale_legacy_lock_is_preserved_in_the_private_readiness_snapshot() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let lock_path = fixture.root.join(".beads/.beads.lock");
    let lock_file = fs::File::create(&lock_path).unwrap();
    let stale = std::time::SystemTime::now()
        .checked_sub(Duration::from_secs(31 * 60))
        .unwrap();
    lock_file
        .set_times(fs::FileTimes::new().set_modified(stale))
        .unwrap();
    let stale_seconds = lock_file
        .metadata()
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    let _expected_mtime = EnvVarGuard::set("JIG_TRACKER_TEST_LOCK_MTIME", stale_seconds);

    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "stale_lock");
    let mut never_cancelled = || false;
    assert_eq!(
        adapter
            .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::StaleStorage
    );
    assert!(
        logged_argument_groups(&fixture.log)
            .iter()
            .all(|args| !args.iter().any(|arg| arg == "update"))
    );
}

#[test]
fn oversized_sparse_store_fails_before_the_readiness_process_starts() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    fs::OpenOptions::new()
        .write(true)
        .open(fixture.root.join(".beads/beads.db"))
        .unwrap()
        .set_len(process::MAX_STORE_SNAPSHOT_BYTES + 1)
        .unwrap();

    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let mut never_cancelled = || false;
    assert_eq!(
        adapter
            .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::StoreSnapshotTooLarge {
            limit_bytes: process::MAX_STORE_SNAPSHOT_BYTES
        }
    );
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);
}

#[test]
fn fifo_database_sidecar_fails_without_escaping_the_operation_timeout() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let fifo = fixture.root.join(".beads/beads.db-wal");
    let fifo_bytes = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo_bytes.as_ptr(), 0o600) }, 0);

    let (send, receive) = mpsc::channel();
    let worker = thread::spawn(move || {
        let mut never_cancelled = || false;
        send.send(adapter.check_storage_readiness(&mut never_cancelled))
            .unwrap();
    });
    let result = match receive.recv_timeout(Duration::from_secs(1)) {
        Ok(result) => result,
        Err(error) => {
            // Unblock a regressed blocking read-open so the test process can
            // retire the worker before reporting the timeout failure.
            let _writer = fs::OpenOptions::new().write(true).open(&fifo).unwrap();
            let _result_after_unblock = receive.recv_timeout(Duration::from_secs(1)).unwrap();
            worker.join().unwrap();
            panic!("readiness blocked while opening a FIFO sidecar: {error}");
        }
    };
    worker.join().unwrap();
    assert_eq!(
        result.unwrap_err(),
        TrackerError::InvalidWorkspace {
            reason: InvalidWorkspaceReason::Database
        }
    );
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);
}
