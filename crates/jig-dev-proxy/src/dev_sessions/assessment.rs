use super::process_identity::{ProcessIdentityObservation, observe_process_identity};
use crate::state::{DevSessionAppSpawnEvidence, DevSessionRecord};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AmbiguousOrphanPolicy {
    Retain,
    Forget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum OrphanRetentionReason {
    ControlAlive,
    SupervisorAlive,
    SupervisorUncertain,
    PreflightCleanupPending,
    PreflightCleanupUnknown,
    AppAlive(String),
    AppUncertain(String),
    AppSpawnPending(String),
    AppSpawnUntracked(String),
}

impl OrphanRetentionReason {
    pub(super) const fn code(&self) -> &'static str {
        match self {
            Self::ControlAlive => "control-alive",
            Self::SupervisorAlive => "supervisor-alive",
            Self::SupervisorUncertain => "supervisor-uncertain",
            Self::PreflightCleanupPending => "preflight-cleanup-pending",
            Self::PreflightCleanupUnknown => "preflight-cleanup-unknown",
            Self::AppAlive(_) => "app-alive",
            Self::AppUncertain(_) => "app-uncertain",
            Self::AppSpawnPending(_) => "app-spawn-pending",
            Self::AppSpawnUntracked(_) => "app-spawn-untracked",
        }
    }

    pub(super) fn app(&self) -> Option<&str> {
        match self {
            Self::AppAlive(app)
            | Self::AppUncertain(app)
            | Self::AppSpawnPending(app)
            | Self::AppSpawnUntracked(app) => Some(app),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum OrphanRecoveryAssessment {
    Retirable,
    Retain(OrphanRetentionReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ObservedActivity {
    Verified,
    Possible,
    None,
}

impl ObservedActivity {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Possible => "possible",
            Self::None => "none",
        }
    }
}

pub(super) struct SessionAssessment {
    pub(super) activity: ObservedActivity,
    pub(super) recovery: OrphanRecoveryAssessment,
}

/// Observe only process identities. The caller supplies an authenticated
/// control-channel result when one has been probed outside the state lock.
pub(super) fn assess_session(
    session: &DevSessionRecord,
    policy: AmbiguousOrphanPolicy,
    control_alive: bool,
) -> SessionAssessment {
    let supervisor = observe_process_identity(&session.supervisor);
    let apps = session
        .apps
        .iter()
        .map(|app| app.process.as_ref().map(observe_process_identity))
        .collect::<Vec<_>>();
    assess_with_observations(session, policy, control_alive, supervisor, &apps)
}

pub(super) fn assess_with_observations(
    session: &DevSessionRecord,
    policy: AmbiguousOrphanPolicy,
    control_alive: bool,
    supervisor: ProcessIdentityObservation,
    apps: &[Option<ProcessIdentityObservation>],
) -> SessionAssessment {
    assert_eq!(session.apps.len(), apps.len());
    let app_alive = session.apps.iter().zip(apps).find_map(|(app, observed)| {
        (*observed == Some(ProcessIdentityObservation::Alive)).then(|| app.name.clone())
    });
    let app_uncertain = session.apps.iter().zip(apps).find_map(|(app, observed)| {
        (*observed == Some(ProcessIdentityObservation::Uncertain)).then(|| app.name.clone())
    });
    let untracked = session.apps.iter().find_map(|app| {
        matches!(app.spawn_evidence(), DevSessionAppSpawnEvidence::Untracked)
            .then(|| app.name.clone())
    });
    let pending = session.apps.iter().find_map(|app| {
        matches!(app.spawn_evidence(), DevSessionAppSpawnEvidence::Pending)
            .then(|| app.name.clone())
    });
    let activity = if control_alive
        || supervisor == ProcessIdentityObservation::Alive
        || app_alive.is_some()
    {
        ObservedActivity::Verified
    } else if supervisor == ProcessIdentityObservation::Uncertain
        || app_uncertain.is_some()
        || (session.cleanup_required
            && (session.preflight_cleanup_pending != Some(false)
                || pending.is_some()
                || untracked.is_some()))
    {
        ObservedActivity::Possible
    } else {
        ObservedActivity::None
    };
    let reason = if control_alive {
        Some(OrphanRetentionReason::ControlAlive)
    } else if supervisor == ProcessIdentityObservation::Alive {
        Some(OrphanRetentionReason::SupervisorAlive)
    } else if supervisor == ProcessIdentityObservation::Uncertain {
        Some(OrphanRetentionReason::SupervisorUncertain)
    } else if let Some(app) = app_alive {
        Some(OrphanRetentionReason::AppAlive(app))
    } else if let Some(app) = app_uncertain {
        Some(OrphanRetentionReason::AppUncertain(app))
    } else if !session.cleanup_required || policy == AmbiguousOrphanPolicy::Forget {
        None
    } else if session.preflight_cleanup_pending == Some(true) {
        Some(OrphanRetentionReason::PreflightCleanupPending)
    } else if session.preflight_cleanup_pending.is_none() {
        Some(OrphanRetentionReason::PreflightCleanupUnknown)
    } else if let Some(app) = pending {
        Some(OrphanRetentionReason::AppSpawnPending(app))
    } else {
        untracked.map(OrphanRetentionReason::AppSpawnUntracked)
    };
    SessionAssessment {
        activity,
        recovery: reason.map_or(
            OrphanRecoveryAssessment::Retirable,
            OrphanRecoveryAssessment::Retain,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{DevProcessIdentity, DevSessionApp, DevSessionControl, DevSessionPhase};

    fn example_session() -> DevSessionRecord {
        DevSessionRecord {
            session_id: "dev_example".into(),
            repo_name: "ExampleProject".into(),
            repo_root_display: "/tmp/ExampleProject".into(),
            repo_root_identity: "/tmp/ExampleProject".into(),
            phase: DevSessionPhase::Orphaned,
            started_at_ms: 1,
            updated_at_ms: 1,
            cleanup_required: true,
            preflight_cleanup_pending: Some(false),
            supervisor: DevProcessIdentity {
                pid: u32::MAX,
                start_token: Some("retired-supervisor".into()),
            },
            control: DevSessionControl {
                port: 1,
                token: "example-control-token".into(),
            },
            apps: vec![DevSessionApp {
                name: "web".into(),
                hostname: Some("web.example.localhost".into()),
                target_host: "127.0.0.1".into(),
                target_port: None,
                spawn_state_tracked: true,
                spawn_pending: false,
                process: None,
            }],
        }
    }

    #[test]
    fn activity_and_retention_are_distinct_across_cleanup_evidence() {
        use ProcessIdentityObservation::{Absent, Alive, Uncertain};

        let cases = [
            (
                "clear",
                Some(false),
                true,
                false,
                None,
                Absent,
                None,
                false,
                ObservedActivity::None,
                None,
            ),
            (
                "preflight pending",
                Some(true),
                true,
                false,
                None,
                Absent,
                None,
                false,
                ObservedActivity::Possible,
                Some(OrphanRetentionReason::PreflightCleanupPending),
            ),
            (
                "legacy preflight unknown",
                None,
                true,
                false,
                None,
                Absent,
                None,
                false,
                ObservedActivity::Possible,
                Some(OrphanRetentionReason::PreflightCleanupUnknown),
            ),
            (
                "spawn pending",
                Some(false),
                true,
                true,
                None,
                Absent,
                None,
                false,
                ObservedActivity::Possible,
                Some(OrphanRetentionReason::AppSpawnPending("web".into())),
            ),
            (
                "spawn untracked",
                Some(false),
                false,
                false,
                None,
                Absent,
                None,
                false,
                ObservedActivity::Possible,
                Some(OrphanRetentionReason::AppSpawnUntracked("web".into())),
            ),
            (
                "supervisor uncertain",
                Some(false),
                true,
                false,
                None,
                Uncertain,
                None,
                false,
                ObservedActivity::Possible,
                Some(OrphanRetentionReason::SupervisorUncertain),
            ),
            (
                "app uncertain",
                Some(false),
                true,
                false,
                Some(Uncertain),
                Absent,
                Some(Uncertain),
                false,
                ObservedActivity::Possible,
                Some(OrphanRetentionReason::AppUncertain("web".into())),
            ),
            (
                "app alive",
                Some(false),
                true,
                false,
                Some(Alive),
                Absent,
                Some(Alive),
                false,
                ObservedActivity::Verified,
                Some(OrphanRetentionReason::AppAlive("web".into())),
            ),
            (
                "app alive with uncertain supervisor",
                Some(false),
                true,
                false,
                Some(Alive),
                Uncertain,
                Some(Alive),
                false,
                ObservedActivity::Verified,
                Some(OrphanRetentionReason::SupervisorUncertain),
            ),
            (
                "supervisor alive",
                Some(false),
                true,
                false,
                None,
                Alive,
                None,
                false,
                ObservedActivity::Verified,
                Some(OrphanRetentionReason::SupervisorAlive),
            ),
            (
                "control alive",
                Some(false),
                true,
                false,
                None,
                Absent,
                None,
                true,
                ObservedActivity::Verified,
                Some(OrphanRetentionReason::ControlAlive),
            ),
        ];
        for (
            name,
            preflight,
            tracked,
            pending,
            process,
            supervisor,
            app,
            control,
            expected_activity,
            expected_reason,
        ) in cases
        {
            let mut session = example_session();
            session.preflight_cleanup_pending = preflight;
            session.apps[0].spawn_state_tracked = tracked;
            session.apps[0].spawn_pending = pending;
            session.apps[0].process = process.map(|_| DevProcessIdentity {
                pid: u32::MAX - 1,
                start_token: Some("example-app".into()),
            });
            let assessment = assess_with_observations(
                &session,
                AmbiguousOrphanPolicy::Retain,
                control,
                supervisor,
                &[app],
            );
            assert_eq!(assessment.activity, expected_activity, "{name}");
            assert_eq!(
                assessment.recovery,
                expected_reason.map_or(
                    OrphanRecoveryAssessment::Retirable,
                    OrphanRecoveryAssessment::Retain,
                ),
                "{name}"
            );
        }
    }
}
