use super::{DevSessionRecord, OrphanRetentionReason, StopWarning};

pub(super) fn retention_warning(
    session: &DevSessionRecord,
    reason: &OrphanRetentionReason,
) -> StopWarning {
    let detail = match reason {
        OrphanRetentionReason::SupervisorAlive => format!(
            "supervisor PID {} remained live after the authenticated stop request",
            session.supervisor.pid
        ),
        OrphanRetentionReason::SupervisorUncertain => format!(
            "supervisor PID {} could not be classified safely",
            session.supervisor.pid
        ),
        OrphanRetentionReason::PreflightCleanupPending => {
            "development preflight cleanup was not confirmed".to_owned()
        }
        OrphanRetentionReason::AppAlive(app) => {
            let pid = session
                .apps
                .iter()
                .find(|entry| entry.name == *app)
                .and_then(|entry| entry.process.as_ref())
                .map(|identity| identity.pid);
            format!(
                "registered app '{app}' is still live (PID {})",
                pid.map_or_else(|| "unknown".into(), |pid| pid.to_string())
            )
        }
        OrphanRetentionReason::AppUncertain(app) => {
            format!("registered app '{app}' could not be classified safely")
        }
        OrphanRetentionReason::AppSpawnPending(app) => {
            format!("app '{app}' may have spawned before its process identity was durably recorded")
        }
        OrphanRetentionReason::AppSpawnUntracked(app) => format!(
            "legacy app '{app}' has no process identity and predates durable spawn-state tracking"
        ),
    };
    let repair = match reason {
        OrphanRetentionReason::PreflightCleanupPending
        | OrphanRetentionReason::AppSpawnPending(_)
        | OrphanRetentionReason::AppSpawnUntracked(_) => {
            "; after independently confirming that no unrecorded process remains, retry with `jig dev stop --forget-ambiguous-orphans`"
        }
        OrphanRetentionReason::AppAlive(_) | OrphanRetentionReason::AppUncertain(_) => {
            "; the owning supervisor is gone: inspect `jig dev status --json`, independently verify and stop surviving app processes, then retry `jig dev stop` or `jig dev --replace`; `--forget-ambiguous-orphans` cannot bypass live or uncertain process identities"
        }
        _ => "",
    };
    StopWarning {
        session_id: session.session_id.clone(),
        message: format!(
            "session '{}': {detail}; the registry entry was retained without signaling numeric PIDs{repair}",
            session.session_id
        ),
    }
}
