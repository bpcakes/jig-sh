use super::*;
use crate::state::{DevProcessIdentity, DevSessionControl};

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
            token: "example-control-token".into(),
        },
        apps: Vec::new(),
    }
}

#[test]
fn missing_legacy_preflight_evidence_blocks_strict_recovery() {
    let mut session = cleanup_required_session();
    session.preflight_cleanup_pending = None;

    let strict = orphan_recovery_assessment_with_observations(
        &session,
        AmbiguousOrphanPolicy::Retain,
        ProcessIdentityObservation::Absent,
        |_, _| None,
    );
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

    let supervisor_active = session_status_from_observations(
        &session,
        &[],
        false,
        ProcessIdentityObservation::Alive,
        &[],
    );
    assert_eq!(supervisor_active["status"], "running");
    assert_eq!(supervisor_active["recoverable"], false);
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
    assert_eq!(status["apps"][0]["alive"], true);
    assert_eq!(status["apps"][0]["identity_observation"], "alive");
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
