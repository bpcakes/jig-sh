#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::test_tempdir as tempdir;

use crate::DevPreflightError;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::dev_api;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::dev_sessions::DevSessionRuntime;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::state::StateStore;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::types::{AppRunSpec, CommandSpec};

use super::super::{
    TerminationReason, interruption_reason, is_interruption, normalize_preflight_result,
};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::super::{finalize_claimed_dev_session_result, finish_preflight_cleanup};

#[test]
fn typed_preflight_cancellation_requires_a_pending_termination_reason() {
    let reason = TerminationReason::from_signal({
        #[cfg(unix)]
        {
            libc::SIGTERM
        }
        #[cfg(not(unix))]
        {
            2
        }
    });

    let interrupted =
        normalize_preflight_result(Err(DevPreflightError::cancelled()), Some(reason)).unwrap_err();
    assert_eq!(interruption_reason(&interrupted), Some(reason));

    let unconfirmed =
        normalize_preflight_result(Err(DevPreflightError::cancelled()), None).unwrap_err();
    assert!(!is_interruption(&unconfirmed));
    assert!(unconfirmed.to_string().contains("without a pending"));
}

#[test]
fn preflight_failure_survives_even_when_termination_is_pending() {
    let reason = TerminationReason::from_signal({
        #[cfg(unix)]
        {
            libc::SIGINT
        }
        #[cfg(not(unix))]
        {
            2
        }
    });
    let failure = anyhow::anyhow!("preflight cleanup failure sentinel");

    let error = normalize_preflight_result(Err(DevPreflightError::failed(failure)), Some(reason))
        .unwrap_err();

    assert!(!is_interruption(&error));
    assert_eq!(error.to_string(), "preflight cleanup failure sentinel");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn confirmed_preflight_cleanup_clears_the_durable_obligation() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().join("proxy-state"))).unwrap();
    let spec = AppRunSpec::new(
        "web",
        temp.path().to_path_buf(),
        CommandSpec::Argv(vec!["unused-preflight-command".into()]),
        "web.demo.localhost",
    )
    .with_proxy(false);
    let session = DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        std::slice::from_ref(&spec),
        false,
    )
    .unwrap();
    let mut cleanup = session.begin_preflight_cleanup().unwrap();

    finish_preflight_cleanup(&session, &mut cleanup, Ok(()), &|| None).unwrap();

    assert!(session.cleanup_is_confirmed());
    let sessions = store.snapshot_dev_state().unwrap().sessions;
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].cleanup_required);
    assert!(!sessions[0].preflight_cleanup_pending);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn confirmed_preflight_cleanup_is_persisted_before_pending_interruption() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().join("proxy-state"))).unwrap();
    let spec = AppRunSpec::new(
        "web",
        temp.path().to_path_buf(),
        CommandSpec::Argv(vec!["unused-preflight-command".into()]),
        "web.demo.localhost",
    )
    .with_proxy(false);
    let session = DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        std::slice::from_ref(&spec),
        false,
    )
    .unwrap();
    let mut cleanup = session.begin_preflight_cleanup().unwrap();
    let reason = TerminationReason::requested_stop();

    finish_preflight_cleanup(&session, &mut cleanup, Ok(()), &|| Some(reason)).unwrap();

    assert!(session.cleanup_is_confirmed());
    let sessions = store.snapshot_dev_state().unwrap().sessions;
    assert_eq!(sessions.len(), 1);
    assert!(!sessions[0].preflight_cleanup_pending);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn requested_stop_with_unconfirmed_cleanup_is_a_structured_failure() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().join("proxy-state"))).unwrap();
    let spec = AppRunSpec::new(
        "web",
        temp.path().to_path_buf(),
        CommandSpec::Argv(vec!["unused-preflight-command".into()]),
        "web.demo.localhost",
    )
    .with_proxy(false);
    let session = DevSessionRuntime::start(
        store,
        "demo",
        temp.path(),
        std::slice::from_ref(&spec),
        false,
    )
    .unwrap();
    let _cleanup = session.arm_cleanup();
    let reason = TerminationReason::requested_stop();

    let output = dev_api::normalize_dev_result(finalize_claimed_dev_session_result(
        Err(super::super::interruption_error(reason)),
        &session,
    ))
    .unwrap();

    assert_eq!(output["ok"], false);
    assert_eq!(output["stopped"], false);
    assert_eq!(output["cleanup_unconfirmed"], true);
    assert_eq!(output["stop_reason"], reason.label());
    assert_eq!(
        output["error"],
        super::super::UNCONFIRMED_DEV_CLEANUP_MESSAGE
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn unconfirmed_preflight_cleanup_retains_the_registered_session() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().join("proxy-state"))).unwrap();
    let spec = AppRunSpec::new(
        "web",
        temp.path().to_path_buf(),
        CommandSpec::Argv(vec!["unused-preflight-command".into()]),
        "web.demo.localhost",
    )
    .with_proxy(false);
    let session = DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        std::slice::from_ref(&spec),
        false,
    )
    .unwrap();
    let mut cleanup = session.begin_preflight_cleanup().unwrap();
    assert!(store.snapshot_dev_state().unwrap().sessions[0].preflight_cleanup_pending);

    let error = finish_preflight_cleanup(
        &session,
        &mut cleanup,
        Err(DevPreflightError::cleanup_unconfirmed(anyhow::anyhow!(
            "preflight cleanup was not confirmed"
        ))),
        &|| None,
    )
    .unwrap_err();

    assert_eq!(error.to_string(), "preflight cleanup was not confirmed");
    assert!(!session.cleanup_is_confirmed());
    drop(cleanup);
    drop(session);
    let sessions = store.snapshot_dev_state().unwrap().sessions;
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].cleanup_required);
    assert!(sessions[0].preflight_cleanup_pending);
    assert_eq!(sessions[0].phase, crate::state::DevSessionPhase::Orphaned);
}
