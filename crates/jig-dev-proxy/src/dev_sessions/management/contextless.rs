use super::*;
use crate::session_id::is_valid_session_id;

pub(crate) fn status_all(state_dir: Option<PathBuf>) -> Result<Value> {
    contextless_status(None, state_dir)
}

pub(crate) fn status_session(session_id: &str, state_dir: Option<PathBuf>) -> Result<Value> {
    validate_exact_session_id(session_id)?;
    contextless_status(Some(session_id), state_dir)
}

fn contextless_status(session_id: Option<&str>, state_dir: Option<PathBuf>) -> Result<Value> {
    let configured = configured_state_dir(state_dir.clone())?;
    let Some(store) = StateStore::resolve_existing(state_dir)? else {
        return Ok(contextless_status_value(session_id, &configured, &[]));
    };
    let snapshot = store.snapshot_dev_state()?;
    let sessions = snapshot
        .sessions
        .iter()
        .filter(|session| session_id.is_none_or(|id| session.session_id == id))
        .map(|session| session_status(session, &snapshot.routes))
        .collect::<Vec<_>>();
    Ok(contextless_status_value(
        session_id,
        store.root(),
        &sessions,
    ))
}

fn contextless_status_value(
    session_id: Option<&str>,
    state_dir: &Path,
    sessions: &[Value],
) -> Value {
    let running = sessions
        .iter()
        .any(|session| !matches!(session["status"].as_str(), Some("stale" | "recoverable")));
    let activity = if sessions
        .iter()
        .any(|session| session["activity"] == "verified")
    {
        ObservedActivity::Verified
    } else if sessions
        .iter()
        .any(|session| session["activity"] == "possible")
    {
        ObservedActivity::Possible
    } else {
        ObservedActivity::None
    };
    json!({
        "ok": true,
        "command": "dev status",
        "scope": if session_id.is_some() { "session" } else { "all" },
        "selected_session_id": session_id,
        "state_dir": state_dir,
        "running": running,
        "activity": activity.label(),
        "cleanup_required": sessions.iter().any(|session| session["cleanup_required"] == true),
        "sessions": sessions,
    })
}

pub(crate) fn recover_session(session_id: &str, state_dir: Option<PathBuf>) -> Result<Value> {
    validate_exact_session_id(session_id)?;
    let configured = configured_state_dir(state_dir.clone())?;
    let Some(store) = StateStore::resolve_existing(state_dir)? else {
        return Ok(empty_exact_recovery(session_id, &configured));
    };
    let snapshot = store.snapshot_dev_state()?;
    let Some(session) = snapshot
        .sessions
        .iter()
        .find(|session| session.session_id == session_id)
    else {
        return Ok(empty_exact_recovery(session_id, store.root()));
    };
    let control_alive = ping(
        session.control.port,
        &session.session_id,
        &session.control.token,
    )
    .unwrap_or(false);
    if let OrphanRecoveryAssessment::Retain(reason) =
        assess_session(session, AmbiguousOrphanPolicy::Retain, control_alive).recovery
    {
        return Ok(retained_exact_recovery(session_id, store.root(), reason));
    }
    let outcome = retire_orphan(&store, session, AmbiguousOrphanPolicy::Retain, &|| false)?;
    match outcome {
        LockOutcome::Acquired(RetireDeadOrphanOutcome::Retired(notice)) => Ok(json!({
            "ok": true, "command": "dev recover", "session_id": session_id,
            "state_dir": store.root(), "matched_sessions": 1, "retired_sessions": 1,
            "recoveries": [notice],
        })),
        LockOutcome::Acquired(RetireDeadOrphanOutcome::AlreadyAbsent) => {
            Ok(empty_exact_recovery(session_id, store.root()))
        }
        LockOutcome::Acquired(RetireDeadOrphanOutcome::Retained(reason)) => {
            Ok(retained_exact_recovery(session_id, store.root(), reason))
        }
        LockOutcome::Cancelled => unreachable!("exact recovery is not cancellable"),
    }
}

fn empty_exact_recovery(session_id: &str, state_dir: &Path) -> Value {
    json!({"ok": true, "command": "dev recover", "session_id": session_id, "state_dir": state_dir,
        "matched_sessions": 0, "retired_sessions": 0, "recoveries": []})
}

fn retained_exact_recovery(
    session_id: &str,
    state_dir: &Path,
    reason: OrphanRetentionReason,
) -> Value {
    json!({"ok": false, "command": "dev recover", "session_id": session_id, "state_dir": state_dir,
        "matched_sessions": 1, "retired_sessions": 0, "retention_reason": reason.code(),
        "retention_app": reason.app(), "recoveries": []})
}

fn validate_exact_session_id(session_id: &str) -> Result<()> {
    if !is_valid_session_id(session_id) {
        bail!(
            "Invalid Jig dev session ID; provide the complete ID shown by `jig dev status --all`"
        );
    }
    Ok(())
}

pub(crate) fn stop_session(
    session_id: &str,
    state_dir: Option<PathBuf>,
    forget_ambiguous_orphans: bool,
) -> Result<Value> {
    validate_exact_session_id(session_id)?;
    let configured = configured_state_dir(state_dir.clone())?;
    let Some(store) = StateStore::resolve_existing(state_dir)? else {
        return Ok(empty_exact_stop(session_id, &configured));
    };
    let snapshot = store.snapshot_dev_state()?;
    let Some(session) = snapshot
        .sessions
        .iter()
        .find(|session| session.session_id == session_id)
    else {
        return Ok(empty_exact_stop(session_id, store.root()));
    };
    let repo = CanonicalRepo::from_record(session);
    let ids = BTreeSet::from([session_id.to_owned()]);
    let policy = if forget_ambiguous_orphans {
        AmbiguousOrphanPolicy::Forget
    } else {
        AmbiguousOrphanPolicy::Retain
    };
    let mut output = stop_outcome_json(
        stop_session_ids_interruptible_with_policy(
            &store,
            &repo,
            &ids,
            policy,
            Some(&session.supervisor),
            &|| false,
        ),
        &repo,
        store.root(),
        1,
    )?;
    output["selected_session_id"] = json!(session_id);
    Ok(output)
}

fn empty_exact_stop(session_id: &str, state_dir: &Path) -> Value {
    json!({"ok": true, "command": "dev stop", "selected_session_id": session_id,
        "state_dir": state_dir, "matched_sessions": 0, "stopped_sessions": 0,
        "stopped_apps": 0, "sessions": [], "recoveries": [], "warnings": []})
}
