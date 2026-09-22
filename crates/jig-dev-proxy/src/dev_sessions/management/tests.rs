use super::*;
use crate::state::{DevProcessIdentity, DevSessionControl};
use tempfile::tempdir;

fn cleanup_required_session() -> DevSessionRecord {
    DevSessionRecord {
        session_id: "dev_example".into(),
        repo_name: "ExampleProject".into(),
        repo_root_display: "/tmp/example-project".into(),
        repo_root_identity: "/tmp/example-project".into(),
        phase: DevSessionPhase::Running,
        started_at_ms: 1,
        updated_at_ms: 1,
        cleanup_required: true,
        preflight_cleanup_pending: Some(false),
        supervisor: DevProcessIdentity {
            pid: u32::MAX,
            start_token: Some("example-supervisor".into()),
        },
        control: DevSessionControl {
            port: 1,
            token: "a".repeat(64),
        },
        apps: Vec::new(),
    }
}

#[test]
fn contextless_inspection_finds_deleted_root_legacy_blocker_without_mutation() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("proxy-state");
    let store = StateStore::resolve(Some(state_dir.clone())).unwrap();
    let mut legacy = cleanup_required_session();
    legacy.repo_root_display = temp
        .path()
        .join("deleted-ExampleProject")
        .display()
        .to_string();
    legacy.preflight_cleanup_pending = None;
    store
        .mutate_dev_sessions(|sessions, _| {
            sessions.push(legacy);
            Ok(())
        })
        .unwrap();
    let before = std::fs::read(state_dir.join("dev-sessions.json")).unwrap();

    let all = status_all(Some(state_dir.clone())).unwrap();
    let exact = status_session("dev_example", Some(state_dir.clone())).unwrap();
    assert_eq!(all["sessions"][0]["session_id"], "dev_example");
    assert_eq!(
        all["sessions"][0]["retention_reason"],
        "preflight-cleanup-unknown"
    );
    assert_eq!(
        all["sessions"][0]["repo_root"],
        temp.path()
            .join("deleted-ExampleProject")
            .display()
            .to_string()
    );
    assert_eq!(exact["sessions"].as_array().unwrap().len(), 1);
    assert!(!all.to_string().contains(&"a".repeat(64)));
    assert!(!exact.to_string().contains(&"a".repeat(64)));
    assert_eq!(
        std::fs::read(state_dir.join("dev-sessions.json")).unwrap(),
        before
    );

    let refused = recover_session("dev_example", Some(state_dir.clone())).unwrap();
    assert_eq!(refused["ok"], false);
    assert_eq!(refused["retention_reason"], "preflight-cleanup-unknown");
    assert_eq!(store.snapshot_dev_state().unwrap().sessions.len(), 1);

    let stopped = stop_session("dev_example", Some(state_dir), true).unwrap();
    assert_eq!(stopped["ok"], true);
    assert_eq!(stopped["stopped_sessions"], 1);
    assert!(store.snapshot_dev_state().unwrap().sessions.is_empty());
    assert!(!stopped.to_string().contains(&"a".repeat(64)));
}

#[test]
fn exact_recovery_is_idempotent_and_preserves_unrelated_sessions() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("proxy-state");
    let store = StateStore::resolve(Some(state_dir.clone())).unwrap();
    let mut target = cleanup_required_session();
    target.session_id = "dev_example_target".into();
    let mut other = cleanup_required_session();
    other.session_id = "dev_example_other".into();
    other.preflight_cleanup_pending = Some(true);
    store
        .mutate_dev_sessions(|sessions, _| {
            sessions.extend([target, other]);
            Ok(())
        })
        .unwrap();

    let prefix = recover_session("dev_example", Some(state_dir.clone())).unwrap();
    assert_eq!(prefix["matched_sessions"], 0);
    assert!(recover_session("dev_*", Some(state_dir.clone())).is_err());
    let recovered = recover_session("dev_example_target", Some(state_dir.clone())).unwrap();
    assert_eq!(recovered["retired_sessions"], 1);
    assert_eq!(
        recovered["recoveries"][0]["session_id"],
        "dev_example_target"
    );
    let repeated = recover_session("dev_example_target", Some(state_dir.clone())).unwrap();
    assert_eq!(repeated["retired_sessions"], 0);
    assert_eq!(repeated["matched_sessions"], 0);
    assert_eq!(
        store.snapshot_dev_state().unwrap().sessions[0].session_id,
        "dev_example_other"
    );
    let refused = recover_session("dev_example_other", Some(state_dir)).unwrap();
    assert_eq!(refused["retention_reason"], "preflight-cleanup-pending");
}

#[test]
fn exact_recovery_refuses_pending_live_and_uncertain_evidence() {
    let mut cases = Vec::new();
    let mut preflight = cleanup_required_session();
    preflight.preflight_cleanup_pending = Some(true);
    cases.push((preflight, "preflight-cleanup-pending"));

    let mut supervisor = cleanup_required_session();
    supervisor.supervisor.pid = std::process::id();
    supervisor.supervisor.start_token = None;
    cases.push((supervisor, "supervisor-uncertain"));

    let mut spawn = cleanup_required_session();
    spawn.apps.push(DevSessionApp {
        name: "web".into(),
        hostname: None,
        target_host: "127.0.0.1".into(),
        target_port: Some(4000),
        spawn_state_tracked: true,
        spawn_pending: true,
        process: None,
    });
    cases.push((spawn, "app-spawn-pending"));

    let mut app = cleanup_required_session();
    app.apps.push(DevSessionApp {
        name: "web".into(),
        hostname: None,
        target_host: "127.0.0.1".into(),
        target_port: Some(4000),
        spawn_state_tracked: true,
        spawn_pending: false,
        process: Some(DevProcessIdentity {
            pid: std::process::id(),
            start_token: None,
        }),
    });
    cases.push((app, "app-uncertain"));

    if let Some(token) = crate::state::process_start_token(std::process::id()) {
        let mut live = cleanup_required_session();
        live.apps.push(DevSessionApp {
            name: "web".into(),
            hostname: None,
            target_host: "127.0.0.1".into(),
            target_port: Some(4000),
            spawn_state_tracked: true,
            spawn_pending: false,
            process: Some(DevProcessIdentity {
                pid: std::process::id(),
                start_token: Some(token),
            }),
        });
        cases.push((live, "app-alive"));
    }

    for (session, reason) in cases {
        let temp = tempdir().unwrap();
        let state_dir = temp.path().join("proxy-state");
        let store = StateStore::resolve(Some(state_dir.clone())).unwrap();
        store
            .mutate_dev_sessions(|sessions, _| {
                sessions.push(session);
                Ok(())
            })
            .unwrap();
        let refused = recover_session("dev_example", Some(state_dir)).unwrap();
        assert_eq!(refused["ok"], false, "{reason}");
        assert_eq!(refused["retention_reason"], reason);
        assert_eq!(store.snapshot_dev_state().unwrap().sessions.len(), 1);
    }
}

#[test]
fn exact_recovery_removes_only_selected_owned_process_route() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("proxy-state");
    let store = StateStore::resolve(Some(state_dir.clone())).unwrap();
    let mut target = cleanup_required_session();
    target.session_id = "dev_example_target".into();
    target.apps.push(DevSessionApp {
        name: "web".into(),
        hostname: Some("web.example.localhost".into()),
        target_host: "127.0.0.1".into(),
        target_port: Some(4000),
        spawn_state_tracked: true,
        spawn_pending: false,
        process: Some(DevProcessIdentity {
            pid: u32::MAX - 1,
            start_token: Some("retired-app".into()),
        }),
    });
    let mut other = cleanup_required_session();
    other.session_id = "dev_example_other".into();
    store
        .mutate_dev_state_interruptible(&|| false, |sessions, routes| {
            sessions.extend([target, other]);
            routes.extend([
                Route {
                    hostname: "web.example.localhost".into(),
                    target_host: "127.0.0.1".into(),
                    target_port: 4000,
                    owner_pid: Some(u32::MAX - 1),
                    owner_start_token: Some("retired-app".into()),
                    mode: crate::types::RouteMode::Process,
                    created_at_ms: 1,
                },
                Route {
                    hostname: "other.example.localhost".into(),
                    target_host: "127.0.0.1".into(),
                    target_port: 4001,
                    owner_pid: None,
                    owner_start_token: None,
                    mode: crate::types::RouteMode::Alias,
                    created_at_ms: 1,
                },
            ]);
            Ok(())
        })
        .unwrap();

    let recovered = recover_session("dev_example_target", Some(state_dir)).unwrap();
    assert_eq!(recovered["retired_sessions"], 1);
    let snapshot = store.snapshot_dev_state().unwrap();
    assert_eq!(snapshot.sessions.len(), 1);
    assert_eq!(snapshot.sessions[0].session_id, "dev_example_other");
    assert_eq!(snapshot.routes.len(), 1);
    assert_eq!(
        snapshot.routes[0].hostname.as_str(),
        "other.example.localhost"
    );
}

#[test]
fn missing_legacy_preflight_evidence_blocks_strict_recovery() {
    let mut session = cleanup_required_session();
    session.preflight_cleanup_pending = None;

    let strict = assess_with_observations(
        &session,
        AmbiguousOrphanPolicy::Retain,
        false,
        ProcessIdentityObservation::Absent,
        &[],
    )
    .recovery;
    assert_eq!(
        strict,
        OrphanRecoveryAssessment::Retain(OrphanRetentionReason::PreflightCleanupUnknown)
    );
    assert_eq!(
        forgotten_cleanup_ambiguities(&session, AmbiguousOrphanPolicy::Forget),
        vec![ForgottenCleanupAmbiguity::PreflightCleanup]
    );
    let status = session_status_from_observations(
        &session,
        &[],
        false,
        ProcessIdentityObservation::Absent,
        &[],
    );
    assert_eq!(status["recoverable"], false);
    assert_eq!(status["activity"], "possible");
    assert_eq!(status["retention_reason"], "preflight-cleanup-unknown");
    assert_eq!(status["preflight_cleanup_pending"], false);
    assert_eq!(status["preflight_cleanup_evidence"], "unknown");
}

#[test]
fn active_status_evidence_cannot_also_be_recoverable() {
    let session = cleanup_required_session();

    let control_active = session_status_from_observations(
        &session,
        &[],
        true,
        ProcessIdentityObservation::Absent,
        &[],
    );
    assert_eq!(control_active["status"], "running");
    assert_eq!(control_active["recoverable"], false);
    assert_eq!(control_active["activity"], "verified");
    assert_eq!(control_active["retention_reason"], "control-alive");

    let supervisor_active = session_status_from_observations(
        &session,
        &[],
        false,
        ProcessIdentityObservation::Alive,
        &[],
    );
    assert_eq!(supervisor_active["status"], "running");
    assert_eq!(supervisor_active["recoverable"], false);
    assert_eq!(supervisor_active["activity"], "verified");
    assert_eq!(supervisor_active["retention_reason"], "supervisor-alive");
}

#[test]
fn inactive_recovery_snapshot_is_reported_consistently() {
    let session = cleanup_required_session();

    let status = session_status_from_observations(
        &session,
        &[],
        false,
        ProcessIdentityObservation::Absent,
        &[],
    );

    assert_eq!(status["status"], "recoverable");
    assert_eq!(status["recoverable"], true);
    assert_eq!(status["activity"], "none");
    assert!(status["retention_reason"].is_null());
    assert_eq!(status["supervisor_alive"], false);
    assert_eq!(status["control_alive"], false);
}

#[test]
fn live_app_observation_cannot_also_be_recoverable() {
    let mut session = cleanup_required_session();
    session.apps.push(DevSessionApp {
        name: "web".into(),
        hostname: None,
        target_host: "127.0.0.1".into(),
        target_port: Some(4000),
        spawn_state_tracked: true,
        spawn_pending: false,
        process: Some(DevProcessIdentity {
            pid: u32::MAX - 1,
            start_token: Some("example-app".into()),
        }),
    });

    let status = session_status_from_observations(
        &session,
        &[],
        false,
        ProcessIdentityObservation::Absent,
        &[Some(ProcessIdentityObservation::Alive)],
    );

    assert_eq!(status["status"], "orphaned");
    assert_eq!(status["recoverable"], false);
    assert_eq!(status["activity"], "verified");
    assert_eq!(status["retention_reason"], "app-alive");
    assert_eq!(status["retention_app"], "web");
    assert_eq!(status["apps"][0]["alive"], true);
    assert_eq!(status["apps"][0]["identity_observation"], "alive");
}

#[test]
fn uncertain_supervisor_with_live_app_does_not_claim_the_supervisor_is_gone() {
    let mut session = cleanup_required_session();
    session.apps.push(DevSessionApp {
        name: "web".into(),
        hostname: None,
        target_host: "127.0.0.1".into(),
        target_port: Some(4000),
        spawn_state_tracked: true,
        spawn_pending: false,
        process: Some(DevProcessIdentity {
            pid: u32::MAX - 1,
            start_token: Some("example-app".into()),
        }),
    });
    let assessment = assess_with_observations(
        &session,
        AmbiguousOrphanPolicy::Retain,
        false,
        ProcessIdentityObservation::Uncertain,
        &[Some(ProcessIdentityObservation::Alive)],
    );
    assert_eq!(assessment.activity, ObservedActivity::Verified);
    let OrphanRecoveryAssessment::Retain(reason) = assessment.recovery else {
        panic!("uncertain supervisor must block recovery");
    };
    assert_eq!(reason, OrphanRetentionReason::SupervisorUncertain);
    let warning = retention_warning(&session, &reason).message;
    assert!(warning.contains("supervisor PID"));
    assert!(!warning.contains("supervisor is gone"));
}

#[test]
fn stopped_app_count_only_includes_targets_retired_during_control_phase() {
    let initially_maybe_live_apps =
        HashMap::from([("retired".to_owned(), 2), ("unretired".to_owned(), 3)]);
    let unretired_after_control_ids = HashSet::from(["unretired"]);

    assert_eq!(
        count_stopped_apps(initially_maybe_live_apps, &unretired_after_control_ids),
        2
    );
}

#[test]
fn failed_stop_outcome_reports_completed_progress() {
    let repo = CanonicalRepo {
        name: "ExampleProject".into(),
        root_display: "/tmp/example-project".into(),
        root_identity: "/tmp/example-project".into(),
    };
    let recovery = OrphanRecoveryNotice::from_session(&cleanup_required_session(), &[]);

    let output = stop_outcome_json(
        StopSessionOutcome::Failed {
            error: anyhow::anyhow!("later state read failed"),
            progress: StopProgress {
                recoveries: vec![recovery],
                warnings: StopWarnings {
                    control: vec![StopWarning {
                        session_id: "dev_other".into(),
                        message: "authenticated stop was unavailable".into(),
                    }],
                    lifecycle: vec![StopWarning {
                        session_id: "dev_example".into(),
                        message: "cleanup identity remained uncertain".into(),
                    }],
                },
            },
        },
        &repo,
        Path::new("/tmp/example-state"),
        2,
    )
    .unwrap();

    assert_eq!(output["ok"], false);
    assert_eq!(output["matched_sessions"], 2);
    assert_eq!(output["error"]["kind"], "command_failed");
    assert_eq!(output["error"]["message"], "later state read failed");
    assert_eq!(output["recoveries"].as_array().unwrap().len(), 1);
    assert_eq!(output["recoveries"][0]["session_id"], "dev_example");
    assert_eq!(
        output["warnings"],
        json!([
            "cleanup identity remained uncertain",
            "authenticated stop was unavailable"
        ])
    );
    assert!(output.get("stopped_sessions").is_none());
}

#[test]
fn completed_stop_filters_progress_warnings_to_remaining_sessions() {
    let mut progress = StopProgress::default();
    progress.record_control_warning("retired", "retired control warning".into());
    progress.record_control_warning("remaining", "remaining control warning".into());
    progress.record_lifecycle_warning(StopWarning {
        session_id: "remaining".into(),
        message: "remaining lifecycle warning".into(),
    });

    let (recoveries, warnings) = progress.into_report_parts(&HashSet::from(["remaining"]));

    assert!(recoveries.is_empty());
    assert_eq!(
        warnings,
        vec!["remaining lifecycle warning", "remaining control warning"]
    );
}
