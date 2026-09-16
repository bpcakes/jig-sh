#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::fs;
use std::os::unix::fs::symlink;
use std::time::Duration;

use serde_json::json;

use super::*;
use crate::test_env::{EnvVarGuard, lock_env};

mod fixture;
use fixture::*;
mod budget;
mod mutation_authority;
mod security;

const WORKSPACE_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const ISSUE_ID: &str = "ExampleProject-1";
const ACTOR: &str = "Agent $(not-executed)";

#[test]
fn supported_profile_normalizes_reads_mutations_and_exact_argv() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let _bd_db = EnvVarGuard::set("BD_DB", "/tmp/foreign.db");
    let _bd_database = EnvVarGuard::set("BD_DATABASE", "/tmp/foreign-two.db");
    let _beads_dir = EnvVarGuard::set("BEADS_DIR", "/tmp/foreign-beads");
    let _jsonl = EnvVarGuard::set("BEADS_JSONL", "/tmp/foreign.jsonl");
    let _cache = EnvVarGuard::set("BEADS_CACHE_DIR", "/tmp/foreign-cache");
    let _format = EnvVarGuard::set("BR_OUTPUT_FORMAT", "toon");
    let _toon = EnvVarGuard::set("TOON_DEFAULT_FORMAT", "toon");
    let _no_db = EnvVarGuard::set("BD_NO_DB", "true");
    let _disable_fast_open = EnvVarGuard::set("BR_DISABLE_READ_ONLY_FAST_OPEN", "1");
    let _startup_cache = EnvVarGuard::set("BR_STARTUP_CACHE", "1");
    let _startup_cache_dir =
        EnvVarGuard::set("BR_STARTUP_CACHE_DIR", fixture.root.join("ambient-cache"));
    let _unknown_bd = EnvVarGuard::set("BD_ALLOW_STALE", "true");
    let _unknown_br = EnvVarGuard::set("BR_INHERITED_CONTEXT", "1");
    let _unknown_beads = EnvVarGuard::set("BEADS_REMOTE_SYNC_INTERVAL", "1");
    let _unknown_toon = EnvVarGuard::set("TOON_STATS", "1");
    // Snapshot the complete tracker store, not only the two known authority
    // files, so an unexpected cache/lock artifact is also observable.
    let store_before = tree_identity(&fixture.root.join(".beads"));
    let (adapter, discovery) = fixture.discover(TrackerProcessPolicy::default());

    assert_eq!(discovery.version, "0.5.7");
    assert_eq!(discovery.profile, TrackerProfile::Beads0_5_7);
    assert_eq!(discovery.capabilities, PROFILE_CAPABILITIES);
    let mut never_cancelled = || false;
    let issue = adapter.show_issue(ISSUE_ID, &mut never_cancelled).unwrap();
    assert_eq!(issue.provider, "beads");
    assert_eq!(issue.workspace_id, WORKSPACE_ID);
    assert_eq!(issue.id, ISSUE_ID);
    assert_eq!(issue.description, "Generic fixture description");
    assert_eq!(issue.acceptance_criteria, "It remains portable.");
    assert!(issue.semantic_revision.starts_with("sha256:"));

    let comments = adapter
        .list_comments(ISSUE_ID, &mut never_cancelled)
        .unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].id, "6");

    let message = "Marker; $(touch should-not-exist)\nsecond line";
    let added = adapter
        .add_comment(ISSUE_ID, ACTOR, message, &mut never_cancelled)
        .unwrap();
    assert_eq!(added.id, "7");
    let claimed = adapter
        .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
        .unwrap();
    assert_eq!(claimed.status.as_deref(), Some("in_progress"));
    assert_eq!(claimed.assignee.as_deref(), Some(ACTOR));
    let closed = adapter
        .close_issue(
            ISSUE_ID,
            ACTOR,
            "Finished; no shell expansion",
            &mut never_cancelled,
        )
        .unwrap();
    assert_eq!(closed.status.as_deref(), Some("closed"));

    let groups = logged_argument_groups(&fixture.log);
    assert_supported_profile_argv(
        &groups,
        message,
        fixture.root.join(".beads/beads.db").to_str().unwrap(),
    );
    assert_eq!(tree_identity(&fixture.root.join(".beads")), store_before);
}

fn assert_supported_profile_argv(groups: &[Vec<String>], message: &str, live_database: &str) {
    assert!(groups.iter().all(|args| {
        args.starts_with(&[
            "--no-auto-import".into(),
            "--no-auto-flush".into(),
            "--no-color".into(),
            "--json".into(),
        ])
    }));
    assert_eq!(groups[0].last().map(String::as_str), Some("version"));
    assert!(groups[1].ends_with(&["--no-db".into(), "where".into()]));
    let mutation = groups
        .iter()
        .find(|args| args.iter().any(|arg| arg.starts_with("--message=")))
        .unwrap();
    assert!(
        mutation
            .iter()
            .any(|arg| arg == &format!("--message={message}"))
    );
    assert!(
        mutation
            .iter()
            .any(|arg| arg == &format!("--actor={ACTOR}"))
    );
    assert_eq!(mutation.last().map(String::as_str), Some(ISSUE_ID));
    for args in groups.iter().skip(2) {
        let database = args
            .windows(2)
            .find(|pair| pair[0] == "--db")
            .map(|pair| pair[1].as_str())
            .unwrap();
        let uses_private_snapshot = args
            .iter()
            .any(|arg| matches!(arg.as_str(), "sync" | "show"))
            || args.windows(2).any(|pair| pair == ["comments", "list"]);
        if uses_private_snapshot {
            assert!(database.contains("jig-tracker-store-"));
            assert!(database.ends_with("/beads.db"));
        } else {
            assert_eq!(database, live_database);
        }
    }
}

#[test]
fn provider_routing_artifacts_fail_before_process_start() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let policy = TrackerProcessPolicy::default();
    let mut never_cancelled = || false;

    fs::write(
        fixture.root.join(".beads/routes.jsonl"),
        b"{\"prefix\":\"ExampleProject-\",\"path\":\"../OtherProject\"}\n",
    )
    .unwrap();
    assert_eq!(
        BeadsAdapter::discover(&fixture.root, WORKSPACE_ID, policy, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::InvalidWorkspace {
            reason: InvalidWorkspaceReason::Routing
        }
    );
    assert!(!fixture.log.exists());
    fs::remove_file(fixture.root.join(".beads/routes.jsonl")).unwrap();

    fs::write(
        fixture.root.join(".beads/redirect"),
        b"../OtherProject/.beads\n",
    )
    .unwrap();
    assert_eq!(
        BeadsAdapter::discover(&fixture.root, WORKSPACE_ID, policy, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::InvalidWorkspace {
            reason: InvalidWorkspaceReason::Routing
        }
    );
    assert!(!fixture.log.exists());
    fs::remove_file(fixture.root.join(".beads/redirect")).unwrap();

    let town_root = fixture.root.parent().unwrap();
    fs::create_dir_all(town_root.join("mayor")).unwrap();
    fs::create_dir(town_root.join(".beads")).unwrap();
    fs::write(town_root.join("mayor/town.json"), b"{}\n").unwrap();
    fs::write(
        town_root.join(".beads/routes.jsonl"),
        b"{\"prefix\":\"ExampleProject-\",\"path\":\"OtherProject\"}\n",
    )
    .unwrap();
    assert_eq!(
        BeadsAdapter::discover(&fixture.root, WORKSPACE_ID, policy, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::InvalidWorkspace {
            reason: InvalidWorkspaceReason::Routing
        }
    );
    assert!(!fixture.log.exists());
}

#[test]
fn routing_added_after_discovery_fails_before_the_next_operation() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);

    fs::write(
        fixture.root.join(".beads/routes.jsonl"),
        b"{\"prefix\":\"ExampleProject-\",\"path\":\"../OtherProject\"}\n",
    )
    .unwrap();
    let mut never_cancelled = || false;
    assert_eq!(
        adapter
            .show_issue(ISSUE_ID, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::InvalidWorkspace {
            reason: InvalidWorkspaceReason::Routing
        }
    );
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);
}

#[test]
fn missing_binary_is_a_typed_discovery_failure() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(None);
    let mut never_cancelled = || false;
    assert_eq!(
        BeadsAdapter::discover(
            &fixture.root,
            WORKSPACE_ID,
            TrackerProcessPolicy::default(),
            &mut never_cancelled,
        )
        .unwrap_err(),
        TrackerError::BinaryMissing
    );
}

#[test]
fn executable_is_resolved_once_and_unknown_version_cannot_mutate() {
    let _env = lock_env();
    let supported = Fixture::new("0.5.7");
    let unsupported = Fixture::new("9.0.0");
    let _br = TestBrOverride::set(Some(&supported.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &supported.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = supported.discover(TrackerProcessPolicy::default());
    let _changed_br = TestBrOverride::set(Some(&unsupported.bin.join("br")));
    let mut never_cancelled = || false;
    assert_eq!(
        adapter
            .show_issue(ISSUE_ID, &mut never_cancelled)
            .unwrap()
            .id,
        ISSUE_ID
    );

    drop(_changed_br);
    drop(_log);
    let _unsupported_br = TestBrOverride::set(Some(&unsupported.bin.join("br")));
    let _unsupported_log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &unsupported.log);
    let (adapter, discovery) = unsupported.discover(TrackerProcessPolicy::default());
    assert_eq!(discovery.profile, TrackerProfile::Unsupported);
    assert!(discovery.capabilities.is_empty());
    assert_eq!(
        adapter
            .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::UnsupportedBinary {
            version: "9.0.0".into()
        }
    );
    assert_eq!(logged_argument_groups(&unsupported.log).len(), 1);
}

#[test]
fn executable_replacement_after_discovery_starts_no_further_process() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);

    write_executable(
        &fixture.bin.join("replacement"),
        &fake_br_script(&fixture.root, fixture._temp.path(), "9.0.0"),
    );
    fs::rename(fixture.bin.join("replacement"), fixture.bin.join("br")).unwrap();

    let mut never_cancelled = || false;
    assert_eq!(
        adapter
            .show_issue(ISSUE_ID, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::ExecutableChanged
    );
    assert_eq!(
        adapter
            .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::ExecutableChanged
    );
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);
}

#[test]
fn executable_in_place_change_after_verification_runs_the_immutable_snapshot() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let policy = TrackerProcessPolicy::default();
    let (adapter, _) = fixture.discover(policy);
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);

    let replacement_ran = fixture._temp.path().join("replacement-ran");
    let replacement_body = format!(
        "#!/bin/sh\n: > {}\nexit 97\n",
        shell_quote(&replacement_ran.to_string_lossy())
    );
    let runner = process::ProcessRunner::new(
        &adapter.root,
        &adapter.executable,
        adapter.store.as_ref(),
        policy,
    );
    let mut never_cancelled = || false;
    let value = runner
        .run_json_after_prepare(
            TrackerOperation::ShowIssue,
            &["show", "--", ISSUE_ID],
            &mut never_cancelled,
            || write_executable(&fixture.bin.join("br"), &replacement_body),
        )
        .unwrap();
    assert_eq!(
        profile_0_5_7::parse_issue(&value, WORKSPACE_ID, ISSUE_ID)
            .unwrap()
            .id,
        ISSUE_ID
    );
    assert!(!replacement_ran.exists());
    assert_eq!(logged_argument_groups(&fixture.log).len(), 3);

    assert_eq!(
        adapter
            .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::ExecutableChanged
    );
    assert!(!replacement_ran.exists());
    assert_eq!(logged_argument_groups(&fixture.log).len(), 3);
}

#[test]
fn mutation_provider_uses_the_validated_live_database_namespace() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "database_write");
    let policy = TrackerProcessPolicy::default();
    let (adapter, _) = fixture.discover(policy);

    let database_path = fixture.root.join(".beads/beads.db");
    let runner = process::ProcessRunner::new(
        &adapter.root,
        &adapter.executable,
        adapter.store.as_ref(),
        policy,
    );
    let mut never_cancelled = || false;
    let value = runner
        .run_json_after_prepare(
            TrackerOperation::ClaimIssue,
            &[
                "update",
                "--claim",
                &format!("--actor={ACTOR}"),
                "--",
                ISSUE_ID,
            ],
            &mut never_cancelled,
            || {},
        )
        .unwrap();
    profile_0_5_7::parse_claim(&value, ISSUE_ID, ACTOR).unwrap();
    assert!(
        fs::read(&database_path)
            .unwrap()
            .ends_with(b"provider-write")
    );
    let mutation = logged_argument_groups(&fixture.log).pop().unwrap();
    let configured_database = mutation
        .windows(2)
        .find(|pair| pair[0] == "--db")
        .map(|pair| pair[1].as_str())
        .unwrap();
    assert_eq!(configured_database, database_path.to_str().unwrap());
}

#[test]
fn discovery_budget_and_cancellation_cover_executable_snapshotting() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let mut never_cancelled = || false;
    let zero_budget = TrackerProcessPolicy {
        discovery_timeout: Duration::ZERO,
        ..TrackerProcessPolicy::default()
    };
    assert_eq!(
        BeadsAdapter::discover(
            &fixture.root,
            WORKSPACE_ID,
            zero_budget,
            &mut never_cancelled
        )
        .unwrap_err(),
        TrackerError::TimedOut {
            operation: TrackerOperation::Version
        }
    );
    assert!(!fixture.log.exists());

    let mut checkpoints = 0_u8;
    let mut cancel_during_preparation = || {
        checkpoints += 1;
        checkpoints >= 7
    };
    assert_eq!(
        BeadsAdapter::discover(
            &fixture.root,
            WORKSPACE_ID,
            TrackerProcessPolicy::default(),
            &mut cancel_during_preparation,
        )
        .unwrap_err(),
        TrackerError::CancelledBeforeStart {
            operation: TrackerOperation::Version
        }
    );
    assert!(!fixture.log.exists());
}

#[test]
fn hard_linked_tracker_files_block_mutations_before_process_start() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);

    let database_alias = fixture._temp.path().join("external-database.db");
    fs::hard_link(fixture.root.join(".beads/beads.db"), &database_alias).unwrap();
    let mut never_cancelled = || false;
    assert_eq!(
        adapter
            .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::InvalidWorkspace {
            reason: InvalidWorkspaceReason::HardLinkedAuthority
        }
    );
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);
    fs::remove_file(database_alias).unwrap();

    let jsonl_alias = fixture._temp.path().join("external-issues.jsonl");
    fs::hard_link(fixture.root.join(".beads/issues.jsonl"), &jsonl_alias).unwrap();
    assert_eq!(
        adapter
            .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::InvalidWorkspace {
            reason: InvalidWorkspaceReason::HardLinkedAuthority
        }
    );
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);
}

#[test]
fn strict_json_path_boundaries_and_process_budgets_fail_closed() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let policy = TrackerProcessPolicy {
        discovery_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_millis(500),
        mutation_timeout: Duration::from_millis(500),
        output_limits: ProcessOutputLimits {
            stdout: 1024,
            stderr: 128,
        },
    };

    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "trailing");
    let mut never_cancelled = || false;
    assert_eq!(
        BeadsAdapter::discover(&fixture.root, WORKSPACE_ID, policy, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::UnsupportedResponse {
            operation: TrackerOperation::Version
        }
    );
    drop(_mode);
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "outside");
    assert_eq!(
        BeadsAdapter::discover(&fixture.root, WORKSPACE_ID, policy, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::InvalidWorkspace {
            reason: InvalidWorkspaceReason::Database
        }
    );
    drop(_mode);
    let (adapter, _) = fixture.discover(policy);
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "overflow");
    assert_eq!(
        adapter
            .show_issue(ISSUE_ID, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::OutputLimit {
            operation: TrackerOperation::ShowIssue,
            stream: TrackerOutputStream::Stdout
        }
    );
    drop(_mode);
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "timeout");
    assert_eq!(
        adapter
            .show_issue(ISSUE_ID, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::TimedOut {
            operation: TrackerOperation::ShowIssue
        }
    );
    drop(_mode);
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "claim_timeout");
    assert_eq!(
        adapter
            .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::IndeterminateWrite {
            operation: TrackerOperation::ClaimIssue
        }
    );
    drop(_mode);
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "duplicate");
    assert_eq!(
        BeadsAdapter::discover(&fixture.root, WORKSPACE_ID, policy, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::UnsupportedResponse {
            operation: TrackerOperation::Version
        }
    );
    drop(_mode);
    let mut cancelled = || true;
    assert_eq!(
        BeadsAdapter::discover(&fixture.root, WORKSPACE_ID, policy, &mut cancelled).unwrap_err(),
        TrackerError::CancelledBeforeStart {
            operation: TrackerOperation::Version
        }
    );
}

#[test]
fn provider_failures_and_argument_boundaries_are_typed_without_shells() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let unusual_id = "ExampleProject: item $(not-executed); [one]";
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "missing");
    let mut never_cancelled = || false;
    assert_eq!(
        adapter
            .show_issue(unusual_id, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::IssueMissing {
            issue_id: unusual_id.into()
        }
    );
    assert_eq!(
        logged_argument_groups(&fixture.log)
            .last()
            .unwrap()
            .last()
            .map(String::as_str),
        Some(unusual_id)
    );
    drop(_mode);

    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "blocked");
    assert_eq!(
        adapter
            .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::BlockedTransition {
            issue_id: ISSUE_ID.into()
        }
    );
    drop(_mode);
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "assignment");
    assert_eq!(
        adapter
            .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::AssignmentConflict {
            issue_id: ISSUE_ID.into()
        }
    );
    drop(_mode);

    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "comment_mismatch");
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
    drop(_mode);

    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "close_blocked");
    assert_eq!(
        adapter
            .close_issue(ISSUE_ID, ACTOR, "Done", &mut never_cancelled)
            .unwrap_err(),
        TrackerError::BlockedTransition {
            issue_id: ISSUE_ID.into()
        }
    );
    drop(_mode);
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "close_missing");
    assert_eq!(
        adapter
            .close_issue(ISSUE_ID, ACTOR, "Done", &mut never_cancelled)
            .unwrap_err(),
        TrackerError::IssueMissing {
            issue_id: ISSUE_ID.into()
        }
    );
    drop(_mode);
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "close_ambiguous");
    assert_eq!(
        adapter
            .close_issue(ISSUE_ID, ACTOR, "Done", &mut never_cancelled)
            .unwrap_err(),
        TrackerError::IndeterminateWrite {
            operation: TrackerOperation::CloseIssue
        }
    );
    drop(_mode);
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "unstructured_failure");
    assert_eq!(
        adapter
            .show_issue(ISSUE_ID, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::ProcessFailure {
            operation: TrackerOperation::ShowIssue,
            exit_code: Some(5)
        }
    );
}

#[test]
fn cancellation_and_symlinked_database_fail_before_mutation() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let policy = TrackerProcessPolicy {
        discovery_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        mutation_timeout: Duration::from_secs(1),
        ..TrackerProcessPolicy::default()
    };
    let (adapter, _) = fixture.discover(policy);
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "timeout");
    let mut cancel_after_spawn =
        || fixture.log.exists() && logged_argument_groups(&fixture.log).len() > 2;
    assert_eq!(
        adapter
            .show_issue(ISSUE_ID, &mut cancel_after_spawn)
            .unwrap_err(),
        TrackerError::Cancelled {
            operation: TrackerOperation::ShowIssue
        }
    );
    drop(_mode);

    let outside_database = fixture._temp.path().join("outside.db");
    fs::write(&outside_database, b"foreign database").unwrap();
    symlink(&outside_database, fixture.root.join(".beads/link.db")).unwrap();
    let _mode = EnvVarGuard::set("JIG_TRACKER_TEST_MODE", "symlink");
    let mut never_cancelled = || false;
    assert_eq!(
        BeadsAdapter::discover(&fixture.root, WORKSPACE_ID, policy, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::InvalidWorkspace {
            reason: InvalidWorkspaceReason::Database
        }
    );
    assert!(logged_argument_groups(&fixture.log).iter().all(|args| {
        !args
            .iter()
            .any(|arg| matches!(arg.as_str(), "update" | "close"))
    }));
}

#[test]
fn semantic_revision_excludes_operational_fields_and_comments() {
    let base = json!([{
        "id": ISSUE_ID,
        "title": "Generic task",
        "description": "Do the work",
        "acceptance_criteria": "The result is bounded",
        "status": "open",
        "assignee": null,
        "updated_at": "2026-01-01T00:00:00Z",
        "comments": []
    }]);
    let first = profile_0_5_7::parse_issue(&base, WORKSPACE_ID, ISSUE_ID).unwrap();
    let mut operational_change = base.clone();
    let issue = &mut operational_change[0];
    issue["status"] = json!("in_progress");
    issue["assignee"] = json!("ExampleAgent");
    issue["updated_at"] = json!("2026-02-01T00:00:00Z");
    issue["comments"] = json!([{"text": "an audit note"}]);
    let second = profile_0_5_7::parse_issue(&operational_change, WORKSPACE_ID, ISSUE_ID).unwrap();
    assert_eq!(first.semantic_revision, second.semantic_revision);

    let mut acceptance_change = base;
    acceptance_change[0]["acceptance_criteria"] = json!("The changed result is bounded");
    let third = profile_0_5_7::parse_issue(&acceptance_change, WORKSPACE_ID, ISSUE_ID).unwrap();
    assert_ne!(first.semantic_revision, third.semantic_revision);
}

#[test]
fn tombstones_are_distinct_from_missing_issues() {
    let tombstone = json!([{
        "id": ISSUE_ID,
        "title": "Removed generic task",
        "status": "tombstone",
        "deleted_at": "2026-01-01T00:00:00Z"
    }]);
    assert_eq!(
        profile_0_5_7::parse_issue(&tombstone, WORKSPACE_ID, ISSUE_ID),
        Err(TrackerError::IssueTombstoned {
            issue_id: ISSUE_ID.into()
        })
    );
}

#[test]
fn storage_readiness_allows_only_healthy_or_manual_db_newer() {
    let healthy = sync_status(false, false, false, "healthy", &[]);
    profile_0_5_7::require_mutation_ready(&healthy).unwrap();
    let manual = sync_status(false, true, false, "degraded", &["db_newer"]);
    profile_0_5_7::require_mutation_ready(&manual).unwrap();
    for rejected in [
        sync_status(true, false, false, "degraded", &["jsonl_newer"]),
        sync_status(false, true, true, "degraded", &["db_newer"]),
        sync_status(false, false, false, "unsafe", &["jsonl_conflict_markers"]),
    ] {
        assert_eq!(
            profile_0_5_7::require_mutation_ready(&rejected),
            Err(TrackerError::StaleStorage)
        );
    }
}
