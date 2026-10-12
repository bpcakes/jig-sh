use super::*;

pub(super) fn session_status(session: &DevSessionRecord, routes: &[Route]) -> Value {
    let control_alive = ping(
        session.control.port,
        &session.session_id,
        &session.control.token,
    )
    .unwrap_or(false);
    let supervisor_observation = observe_process_identity(&session.supervisor);
    let app_observations = session
        .apps
        .iter()
        .map(|app| app.process.as_ref().map(observe_process_identity))
        .collect::<Vec<_>>();
    session_status_from_observations(
        session,
        routes,
        control_alive,
        supervisor_observation,
        &app_observations,
    )
}

pub(super) fn session_status_from_observations(
    session: &DevSessionRecord,
    routes: &[Route],
    control_alive: bool,
    supervisor_observation: ProcessIdentityObservation,
    app_observations: &[Option<ProcessIdentityObservation>],
) -> Value {
    assert_eq!(
        session.apps.len(),
        app_observations.len(),
        "every development app must have one status observation"
    );
    let supervisor_verified = supervisor_observation.is_verified_alive();
    let supervisor_alive = supervisor_observation.may_be_alive();
    let apps = session
        .apps
        .iter()
        .zip(app_observations.iter().copied())
        .map(|(app, process_observation)| {
            let process_alive =
                process_observation.is_some_and(|observation| observation.may_be_alive());
            let process_identity_verified =
                process_observation.is_some_and(|observation| observation.is_verified_alive());
            let route_present = app.hostname.as_deref().is_some_and(|hostname| {
                routes.iter().any(|route| {
                    route.hostname.as_str() == hostname && session_owns_route(session, route)
                })
            });
            json!({
                "name": app.name,
                "hostname": app.hostname,
                "target_host": app.target_host,
                "target_port": app.target_port,
                "spawn_state_tracked": app.spawn_state_tracked,
                "spawn_pending": app.spawn_pending,
                "pid": app.process.as_ref().map(|process| process.pid),
                "alive": process_alive,
                "identity_verified": process_identity_verified,
                "identity_observation": process_observation.map(ProcessIdentityObservation::label),
                "route_present": route_present,
            })
        })
        .collect::<Vec<_>>();
    let assessment = assess_with_observations(
        session,
        AmbiguousOrphanPolicy::Retain,
        control_alive,
        supervisor_observation,
        app_observations,
    );
    let activity = assessment.activity.label();
    let (retention_reason, retention_app) = match &assessment.recovery {
        OrphanRecoveryAssessment::Retain(reason) => (Some(reason.code()), reason.app()),
        OrphanRecoveryAssessment::Retirable => (None, None),
    };
    let recovery_assessment = assessment.recovery.clone();
    let supervisor_active = control_alive || supervisor_observation.may_be_alive();
    let recoverable = !supervisor_active
        && session.cleanup_required
        && recovery_assessment == OrphanRecoveryAssessment::Retirable;
    let status = if session.phase != DevSessionPhase::Orphaned && supervisor_active {
        match session.phase {
            DevSessionPhase::Starting => "starting",
            DevSessionPhase::Stopping => "stopping",
            DevSessionPhase::Running => "running",
            DevSessionPhase::Orphaned => unreachable!("orphaned phase handled above"),
        }
    } else if recoverable {
        "recoverable"
    } else if matches!(recovery_assessment, OrphanRecoveryAssessment::Retain(_)) {
        "orphaned"
    } else {
        "stale"
    };
    json!({
        "session_id": session.session_id,
        "repo_name": session.repo_name,
        "repo_root": session.repo_root_display,
        "status": status,
        "phase": session.phase,
        "started_at_ms": session.started_at_ms,
        "updated_at_ms": session.updated_at_ms,
        "cleanup_required": session.cleanup_required,
        "activity": activity,
        "retention_reason": retention_reason,
        "retention_app": retention_app,
        "preflight_cleanup_pending": session.preflight_cleanup_pending.unwrap_or(false),
        "preflight_cleanup_evidence": match session.preflight_cleanup_pending {
            Some(true) => "pending",
            Some(false) => "clear",
            None => "unknown",
        },
        "recoverable": recoverable,
        "supervisor_pid": session.supervisor.pid,
        "supervisor_alive": supervisor_alive,
        "supervisor_identity_verified": supervisor_verified,
        "supervisor_observation": supervisor_observation.label(),
        "control_alive": control_alive,
        "apps": apps,
    })
}

pub(super) fn empty_status(repo: &CanonicalRepo, state_dir: PathBuf) -> Value {
    json!({
        "ok": true,
        "command": "dev status",
        "repo_name": repo.name,
        "repo_root": repo.root_display,
        "state_dir": state_dir,
        "running": false,
        "activity": "none",
        "cleanup_required": false,
        "sessions": [],
    })
}
