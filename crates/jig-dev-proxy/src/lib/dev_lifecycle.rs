use std::path::Path;

use crate::test_tempdir as tempdir;
use serde_json::{Value, json};

use crate::state::StateStore;
use crate::{
    AppRunSpec, CommandSpec, DevStatusRequest, DevStopRequest, Route, RouteMode, dev_api,
    dev_sessions, dev_status, dev_stop, processes, state,
};

#[test]
fn interrupted_dev_result_is_structured_after_cleanup() {
    let reason = processes::TerminationReason::from_signal({
        #[cfg(unix)]
        {
            libc::SIGINT
        }
        #[cfg(not(unix))]
        {
            2
        }
    });
    let output = dev_api::normalize_dev_result(Err(processes::interruption_error(reason))).unwrap();

    assert_eq!(
        output,
        json!({
            "ok": false,
            "interrupted": true,
            "exit_status": reason.exit_status(),
            "exit_signal": reason.signal(),
            "termination_signal": reason.label(),
            "first_exit": null,
            "proxy_failed": false,
            "routes": [],
        })
    );
}

#[test]
fn dev_result_normalization_preserves_success_and_ordinary_errors() {
    let success = json!({ "ok": true, "routes": [] });
    assert_eq!(
        dev_api::normalize_dev_result(Ok(success.clone())).unwrap(),
        success
    );

    let error =
        dev_api::normalize_dev_result(Err(anyhow::anyhow!("ordinary failure"))).unwrap_err();
    assert_eq!(error.to_string(), "ordinary failure");
}

fn lifecycle_spec(root: &Path, name: &str, hostname: &str, proxy: bool) -> AppRunSpec {
    AppRunSpec::new(
        name,
        root.to_path_buf(),
        CommandSpec::Argv(vec!["unused-lifecycle-test-command".into()]),
        hostname,
    )
    .with_proxy(proxy)
}

#[test]
fn dev_lifecycle_commands_are_non_mutating_and_structured_without_state() {
    let temp = tempdir().unwrap();
    let repo_root = std::fs::canonicalize(temp.path()).unwrap();
    let state_dir = temp.path().join("missing-proxy-state");

    let status = dev_status(DevStatusRequest::new(
        "demo",
        temp.path().to_path_buf(),
        Some(state_dir.clone()),
    ))
    .unwrap();
    let stop = dev_stop(DevStopRequest::new(
        "demo",
        temp.path().to_path_buf(),
        Some(state_dir.clone()),
    ))
    .unwrap();

    assert_eq!(
        status,
        json!({
            "ok": true,
            "command": "dev status",
            "repo_name": "demo",
            "repo_root": repo_root,
            "state_dir": state_dir,
            "running": false,
            "sessions": [],
        })
    );
    assert_eq!(
        stop,
        json!({
            "ok": true,
            "command": "dev stop",
            "repo_name": "demo",
            "repo_root": repo_root,
            "state_dir": state_dir,
            "matched_sessions": 0,
            "stopped_sessions": 0,
            "stopped_apps": 0,
            "sessions": [],
            "recoveries": [],
            "warnings": [],
        })
    );
    assert!(
        !state_dir.exists(),
        "read-only lifecycle commands must not create missing state"
    );
}

#[test]
fn dev_status_is_repo_scoped_and_does_not_expose_control_credentials() {
    let temp = tempdir().unwrap();
    let repo_a = temp.path().join("repo-a");
    let repo_b = temp.path().join("repo-b");
    let state_dir = temp.path().join("proxy-state");
    std::fs::create_dir_all(&repo_a).unwrap();
    std::fs::create_dir_all(&repo_b).unwrap();
    let store = StateStore::resolve(Some(state_dir.clone())).unwrap();
    let runtime = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "demo",
        &repo_a,
        &[lifecycle_spec(&repo_a, "web", "web.demo.localhost", false)],
        false,
    )
    .unwrap();
    let persisted = store.snapshot_dev_state().unwrap();
    let control_token = persisted.sessions[0].control.token.clone();

    let own = dev_status(DevStatusRequest::new(
        "demo",
        repo_a.clone(),
        Some(state_dir.clone()),
    ))
    .unwrap();
    #[cfg(unix)]
    let alias = {
        let alias_root = temp.path().join("repo-a-alias");
        std::os::unix::fs::symlink(&repo_a, &alias_root).unwrap();
        dev_status(DevStatusRequest::new(
            "demo",
            alias_root,
            Some(state_dir.clone()),
        ))
        .unwrap()
    };
    let other = dev_status(DevStatusRequest::new("demo", repo_b, Some(state_dir))).unwrap();

    assert_eq!(own["ok"], true);
    assert_eq!(own["command"], "dev status");
    assert_eq!(own["running"], true);
    assert_eq!(own["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(own["sessions"][0]["status"], "starting");
    assert_eq!(own["sessions"][0]["control_alive"], true);
    assert_eq!(own["sessions"][0]["apps"][0]["name"], "web");
    assert_eq!(own["sessions"][0]["apps"][0]["hostname"], Value::Null);
    #[cfg(unix)]
    assert_eq!(alias["sessions"], own["sessions"]);
    assert!(
        !serde_json::to_string(&own)
            .unwrap()
            .contains(&control_token),
        "status output must not disclose the private control token"
    );
    assert_eq!(other["running"], false);
    assert_eq!(other["sessions"], json!([]));

    drop(runtime);
    assert!(store.snapshot_dev_state().unwrap().sessions.is_empty());
}

#[test]
fn same_repo_conflict_recommends_dev_lifecycle_commands() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("proxy-state");
    let store = StateStore::resolve(Some(state_dir)).unwrap();
    let spec = lifecycle_spec(temp.path(), "web", "web.demo.localhost", false);
    let runtime = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        std::slice::from_ref(&spec),
        false,
    )
    .unwrap();

    let error =
        dev_sessions::DevSessionRuntime::start(store.clone(), "demo", temp.path(), &[spec], false)
            .err()
            .expect("overlapping same-repo session is rejected")
            .to_string();

    assert!(error.contains("from this repository"));
    assert!(error.contains("jig dev stop"));
    assert!(error.contains("jig dev --replace"));
    assert_eq!(store.snapshot_dev_state().unwrap().sessions.len(), 1);

    drop(runtime);
}

#[test]
fn disjoint_app_selections_from_the_same_repo_can_run_as_separate_sessions() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().join("proxy-state"))).unwrap();
    let web = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        &[lifecycle_spec(
            temp.path(),
            "web",
            "web.demo.localhost",
            false,
        )],
        false,
    )
    .unwrap();
    let worker = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        &[lifecycle_spec(
            temp.path(),
            "worker",
            "worker.demo.localhost",
            false,
        )],
        false,
    )
    .unwrap();

    let sessions = store.snapshot_dev_state().unwrap().sessions;
    assert_eq!(sessions.len(), 2);
    assert!(sessions.iter().any(|session| session.apps[0].name == "web"));
    assert!(
        sessions
            .iter()
            .any(|session| session.apps[0].name == "worker")
    );

    drop(worker);
    drop(web);
}

#[test]
fn same_app_name_in_different_repositories_does_not_conflict() {
    let temp = tempdir().unwrap();
    let repo_a = temp.path().join("repo-a");
    let repo_b = temp.path().join("repo-b");
    std::fs::create_dir_all(&repo_a).unwrap();
    std::fs::create_dir_all(&repo_b).unwrap();
    let store = StateStore::resolve(Some(temp.path().join("proxy-state"))).unwrap();
    let first = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "one",
        &repo_a,
        &[lifecycle_spec(
            &repo_a,
            "shared",
            "shared.one.localhost",
            false,
        )],
        false,
    )
    .unwrap();

    let second = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "two",
        &repo_b,
        &[lifecycle_spec(
            &repo_b,
            "shared",
            "shared.two.localhost",
            false,
        )],
        false,
    )
    .unwrap();

    assert_eq!(store.snapshot_dev_state().unwrap().sessions.len(), 2);

    drop(second);
    drop(first);
}

#[test]
fn replace_refuses_cross_repo_route_ownership() {
    let temp = tempdir().unwrap();
    let repo_a = temp.path().join("repo-a");
    let repo_b = temp.path().join("repo-b");
    std::fs::create_dir_all(&repo_a).unwrap();
    std::fs::create_dir_all(&repo_b).unwrap();
    let store = StateStore::resolve(Some(temp.path().join("proxy-state"))).unwrap();
    let runtime = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "one",
        &repo_a,
        &[lifecycle_spec(&repo_a, "web", "shared.localhost", true)],
        false,
    )
    .unwrap();

    let error = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "two",
        &repo_b,
        &[lifecycle_spec(
            &repo_b,
            "frontend",
            "shared.localhost",
            true,
        )],
        true,
    )
    .err()
    .expect("cross-repository route replacement is rejected")
    .to_string();

    assert!(error.contains("live Jig dev session"));
    assert!(error.contains("cross-repository ownership"));
    assert!(error.contains("shared.localhost"));
    assert!(
        error.contains(
            &std::fs::canonicalize(&repo_a)
                .unwrap()
                .display()
                .to_string()
        )
    );
    assert_eq!(store.snapshot_dev_state().unwrap().sessions.len(), 1);

    drop(runtime);
}

#[test]
fn replace_refuses_an_unregistered_live_process_route() {
    if !state::process_start_tokens_supported() {
        return;
    }
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().join("proxy-state"))).unwrap();
    let owner_pid = std::process::id();
    let owner_start_token =
        state::process_start_token(owner_pid).expect("current process has a start token");
    store
        .add_route(Route {
            hostname: "web.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4005,
            owner_pid: Some(owner_pid),
            owner_start_token: Some(owner_start_token),
            mode: RouteMode::Process,
            created_at_ms: state::now_ms(),
        })
        .unwrap();

    let error = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        &[lifecycle_spec(
            temp.path(),
            "web",
            "web.demo.localhost",
            true,
        )],
        true,
    )
    .err()
    .expect("unmanaged process-route replacement is rejected")
    .to_string();

    assert!(error.contains("not attributable to a registered Jig dev session"));
    assert!(error.contains("will not terminate an unregistered or ad-hoc process"));
    assert!(error.contains(&owner_pid.to_string()));
    assert!(error.contains("127.0.0.1:4005"));
    assert_eq!(store.read_routes(false).unwrap().len(), 1);
    assert!(store.snapshot_dev_state().unwrap().sessions.is_empty());
}

#[test]
fn dev_stop_retires_a_stale_registered_session_and_is_idempotent() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("proxy-state");
    let store = StateStore::resolve(Some(state_dir.clone())).unwrap();
    let runtime = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        &[lifecycle_spec(
            temp.path(),
            "web",
            "web.demo.localhost",
            false,
        )],
        false,
    )
    .unwrap();
    let mut stale = store.snapshot_dev_state().unwrap().sessions.remove(0);
    drop(runtime);
    stale.phase = state::DevSessionPhase::Orphaned;
    stale.supervisor = state::DevProcessIdentity {
        pid: u32::MAX,
        start_token: Some("retired-supervisor".into()),
    };
    stale.apps[0].spawn_state_tracked = false;
    store
        .mutate_dev_sessions(|sessions, _| {
            sessions.push(stale);
            Ok(())
        })
        .unwrap();

    let request =
        || DevStopRequest::new("demo", temp.path().to_path_buf(), Some(state_dir.clone()));
    let stopped = dev_stop(request()).unwrap();
    let repeated = dev_stop(request()).unwrap();

    assert_eq!(stopped["ok"], true);
    assert_eq!(stopped["matched_sessions"], 1);
    assert_eq!(stopped["stopped_sessions"], 1);
    assert_eq!(stopped["stopped_apps"], 0);
    assert_eq!(stopped["sessions"], json!([]));
    assert_eq!(stopped["warnings"], json!([]));
    assert_eq!(stopped["recoveries"], json!([]));
    assert_eq!(repeated["ok"], true);
    assert_eq!(repeated["matched_sessions"], 0);
    assert_eq!(repeated["stopped_sessions"], 0);
    assert!(store.snapshot_dev_state().unwrap().sessions.is_empty());
}

#[test]
fn dead_unconfirmed_cleanup_retires_the_orphan_and_only_its_owned_routes() {
    if !state::process_start_tokens_supported() {
        return;
    }
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("proxy-state");
    let store = StateStore::resolve(Some(state_dir.clone())).unwrap();
    let runtime = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        &[
            lifecycle_spec(temp.path(), "web", "web.demo.localhost", true),
            lifecycle_spec(temp.path(), "admin", "admin.demo.localhost", true),
            lifecycle_spec(temp.path(), "docs", "docs.demo.localhost", true),
        ],
        false,
    )
    .unwrap();
    let _unconfirmed_cleanup = runtime.arm_cleanup();
    drop(runtime);

    let persisted = store.snapshot_dev_state().unwrap();
    assert_eq!(persisted.sessions.len(), 1);
    assert!(persisted.sessions[0].cleanup_required);
    assert_eq!(
        persisted.sessions[0].phase,
        state::DevSessionPhase::Orphaned
    );
    store
        .mutate_dev_sessions(|sessions, _| {
            sessions[0].supervisor = state::DevProcessIdentity {
                pid: u32::MAX,
                start_token: Some("retired-supervisor".into()),
            };
            sessions[0].apps[0].target_port = Some(4005);
            sessions[0].apps[0].spawn_state_tracked = false;
            sessions[0].apps[0].process = Some(state::DevProcessIdentity {
                pid: u32::MAX - 1,
                start_token: Some("retired-app".into()),
            });
            sessions[0].apps[1].target_port = Some(4006);
            sessions[0].apps[1].spawn_state_tracked = false;
            sessions[0].apps[1].process = Some(state::DevProcessIdentity {
                pid: u32::MAX - 2,
                start_token: Some("different-owner".into()),
            });
            sessions[0].apps[2].target_port = Some(4007);
            sessions[0].apps[2].spawn_state_tracked = false;
            sessions[0].apps[2].process = Some(state::DevProcessIdentity {
                pid: u32::MAX - 3,
                start_token: Some("alias-owner".into()),
            });
            Ok(())
        })
        .unwrap();
    let current_pid = std::process::id();
    let current_start_token =
        state::process_start_token(current_pid).expect("current process has a start token");
    let outcome = store
        .mutate_dev_state_interruptible(&|| false, |_, routes| {
            routes.extend([
                Route {
                    hostname: "unrelated.demo.localhost".into(),
                    target_host: "127.0.0.1".into(),
                    target_port: 4999,
                    owner_pid: None,
                    owner_start_token: None,
                    mode: RouteMode::Alias,
                    created_at_ms: state::now_ms(),
                },
                Route {
                    hostname: "docs.demo.localhost".into(),
                    target_host: "127.0.0.1".into(),
                    target_port: 4007,
                    owner_pid: Some(u32::MAX - 3),
                    owner_start_token: Some("alias-owner".into()),
                    mode: RouteMode::Alias,
                    created_at_ms: state::now_ms(),
                },
                Route {
                    hostname: "admin.demo.localhost".into(),
                    target_host: "127.0.0.1".into(),
                    target_port: 4006,
                    owner_pid: Some(current_pid),
                    owner_start_token: Some(current_start_token.clone()),
                    mode: RouteMode::Process,
                    created_at_ms: state::now_ms(),
                },
                Route {
                    hostname: "web.demo.localhost".into(),
                    target_host: "127.0.0.1".into(),
                    target_port: 4005,
                    owner_pid: Some(u32::MAX - 1),
                    owner_start_token: Some("retired-app".into()),
                    mode: RouteMode::Process,
                    created_at_ms: state::now_ms(),
                },
            ]);
            Ok(())
        })
        .unwrap();
    assert!(matches!(outcome, state::LockOutcome::Acquired(())));

    let status = dev_status(DevStatusRequest::new(
        "demo",
        temp.path().to_path_buf(),
        Some(state_dir.clone()),
    ))
    .unwrap();
    assert_eq!(status["running"], false);
    assert_eq!(status["sessions"][0]["status"], "recoverable");
    assert_eq!(status["sessions"][0]["cleanup_required"], true);
    assert_eq!(status["sessions"][0]["recoverable"], true);

    let stopped = dev_stop(DevStopRequest::new(
        "demo",
        temp.path().to_path_buf(),
        Some(state_dir),
    ))
    .unwrap();
    assert_eq!(stopped["ok"], true);
    assert_eq!(stopped["matched_sessions"], 1);
    assert_eq!(stopped["stopped_sessions"], 1);
    assert_eq!(stopped["stopped_apps"], 0);
    assert_eq!(stopped["sessions"], json!([]));
    assert_eq!(stopped["warnings"], json!([]));
    let recovery = &stopped["recoveries"][0];
    assert_eq!(recovery["kind"], "dead-orphan-retired");
    assert_eq!(recovery["forgot_ambiguous_spawn"], false);
    assert_eq!(recovery["apps"][0]["name"], "web");
    assert_eq!(recovery["apps"][0]["target_port"], 4005);
    assert_eq!(recovery["apps"][0]["pid"], u32::MAX - 1);
    assert_eq!(recovery["apps"][0]["spawn_state"], "registered");
    assert!(
        recovery["message"]
            .as_str()
            .is_some_and(|message| message.contains("web (target 127.0.0.1:4005")
                && message.contains(&format!("last PID {}", u32::MAX - 1)))
    );
    let snapshot = store.snapshot_dev_state().unwrap();
    assert!(snapshot.sessions.is_empty());
    let remaining_hosts = snapshot
        .routes
        .iter()
        .map(|route| route.hostname.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        remaining_hosts,
        std::collections::BTreeSet::from([
            "admin.demo.localhost",
            "docs.demo.localhost",
            "unrelated.demo.localhost",
        ])
    );
}

#[cfg(unix)]
#[test]
fn exited_child_pid_is_observed_absent_and_retires_a_dead_orphan() {
    if !state::process_start_tokens_supported() {
        return;
    }
    let mut child = std::process::Command::new("sh")
        .args(["-c", "exit 0"])
        .spawn()
        .unwrap();
    let exited_pid = child.id();
    assert!(child.wait().unwrap().success());
    assert!(i32::try_from(exited_pid).is_ok());
    assert_eq!(
        state::observe_pid(exited_pid),
        state::PidObservation::Absent
    );

    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("proxy-state");
    let store = StateStore::resolve(Some(state_dir.clone())).unwrap();
    let runtime = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        &[lifecycle_spec(
            temp.path(),
            "web",
            "web.demo.localhost",
            false,
        )],
        false,
    )
    .unwrap();
    let _unconfirmed_cleanup = runtime.arm_cleanup();
    drop(runtime);

    store
        .mutate_dev_sessions(|sessions, _| {
            let exited_identity = state::DevProcessIdentity {
                pid: exited_pid,
                start_token: Some("exited-child".into()),
            };
            sessions[0].supervisor = exited_identity.clone();
            sessions[0].apps[0].process = Some(exited_identity);
            Ok(())
        })
        .unwrap();

    let status = dev_status(DevStatusRequest::new(
        "demo",
        temp.path().to_path_buf(),
        Some(state_dir.clone()),
    ))
    .unwrap();
    assert_eq!(status["running"], false);
    assert_eq!(status["sessions"][0]["status"], "recoverable");

    let stopped = dev_stop(DevStopRequest::new(
        "demo",
        temp.path().to_path_buf(),
        Some(state_dir),
    ))
    .unwrap();
    assert_eq!(stopped["ok"], true);
    assert_eq!(stopped["stopped_sessions"], 1);
    assert_eq!(stopped["recoveries"][0]["kind"], "dead-orphan-retired");
    assert!(store.snapshot_dev_state().unwrap().sessions.is_empty());
}

#[test]
fn replace_recovers_a_dead_orphan_before_claiming_the_same_app() {
    if !state::process_start_tokens_supported() {
        return;
    }
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().join("proxy-state"))).unwrap();
    let spec = lifecycle_spec(temp.path(), "web", "web.demo.localhost", true);
    let orphan = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        std::slice::from_ref(&spec),
        false,
    )
    .unwrap();
    let _unconfirmed_cleanup = orphan.arm_cleanup();
    let orphan_id = store.snapshot_dev_state().unwrap().sessions[0]
        .session_id
        .clone();
    drop(orphan);
    store
        .mutate_dev_sessions(|sessions, _| {
            sessions[0].supervisor = state::DevProcessIdentity {
                pid: u32::MAX,
                start_token: Some("retired-supervisor".into()),
            };
            sessions[0].apps[0].target_port = Some(4005);
            sessions[0].apps[0].process = Some(state::DevProcessIdentity {
                pid: u32::MAX - 1,
                start_token: Some("retired-app".into()),
            });
            Ok(())
        })
        .unwrap();
    store
        .add_route(Route {
            hostname: "web.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4005,
            owner_pid: Some(u32::MAX - 1),
            owner_start_token: Some("retired-app".into()),
            mode: RouteMode::Process,
            created_at_ms: state::now_ms(),
        })
        .unwrap();

    let replacement =
        dev_sessions::DevSessionRuntime::start(store.clone(), "demo", temp.path(), &[spec], true)
            .unwrap();

    let snapshot = store.snapshot_dev_state().unwrap();
    assert_eq!(snapshot.sessions.len(), 1);
    assert_ne!(snapshot.sessions[0].session_id, orphan_id);
    assert!(snapshot.routes.is_empty());
    drop(replacement);
}

#[test]
fn unconfirmed_cleanup_with_a_live_registered_app_stays_visible_and_fails_closed() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("proxy-state");
    let store = StateStore::resolve(Some(state_dir.clone())).unwrap();
    let runtime = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        &[lifecycle_spec(
            temp.path(),
            "web",
            "web.demo.localhost",
            false,
        )],
        false,
    )
    .unwrap();
    let _unconfirmed_cleanup = runtime.arm_cleanup();
    drop(runtime);

    store
        .mutate_dev_sessions(|sessions, _| {
            sessions[0].supervisor = state::DevProcessIdentity {
                pid: u32::MAX,
                start_token: Some("retired-supervisor".into()),
            };
            sessions[0].apps[0].process = Some(state::DevProcessIdentity {
                pid: std::process::id(),
                start_token: state::process_start_token(std::process::id()),
            });
            Ok(())
        })
        .unwrap();

    let stopped = dev_stop(
        DevStopRequest::new("demo", temp.path().to_path_buf(), Some(state_dir))
            .with_forget_ambiguous_orphans(true),
    )
    .unwrap();
    assert_eq!(stopped["ok"], false);
    assert_eq!(stopped["matched_sessions"], 1);
    assert_eq!(stopped["stopped_sessions"], 0);
    assert_eq!(stopped["stopped_apps"], 0);
    assert_eq!(stopped["sessions"].as_array().unwrap().len(), 1);
    assert!(
        stopped["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| {
                warning.as_str().is_some_and(|warning| {
                    warning.contains("registered app 'web'")
                        && (warning.contains("is still live")
                            || warning.contains("could not be classified safely"))
                        && warning.contains("without signaling numeric PIDs")
                })
            })
    );
}

#[test]
fn unconfirmed_spawn_window_is_retained_without_trusting_missing_process_identity() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("proxy-state");
    let store = StateStore::resolve(Some(state_dir.clone())).unwrap();
    let runtime = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        &[lifecycle_spec(
            temp.path(),
            "web",
            "web.demo.localhost",
            false,
        )],
        false,
    )
    .unwrap();
    runtime.prepare_app_spawn("web", 4005).unwrap();
    let _unconfirmed_cleanup = runtime.arm_cleanup();
    drop(runtime);

    store
        .mutate_dev_sessions(|sessions, _| {
            sessions[0].supervisor = state::DevProcessIdentity {
                pid: u32::MAX,
                start_token: Some("retired-supervisor".into()),
            };
            Ok(())
        })
        .unwrap();

    let status = dev_status(DevStatusRequest::new(
        "demo",
        temp.path().to_path_buf(),
        Some(state_dir.clone()),
    ))
    .unwrap();
    assert_eq!(status["running"], true);
    assert_eq!(status["sessions"][0]["status"], "orphaned");
    assert_eq!(status["sessions"][0]["recoverable"], false);
    assert_eq!(status["sessions"][0]["apps"][0]["spawn_pending"], true);

    let stopped = dev_stop(DevStopRequest::new(
        "demo",
        temp.path().to_path_buf(),
        Some(state_dir.clone()),
    ))
    .unwrap();
    assert_eq!(stopped["ok"], false);
    assert_eq!(stopped["stopped_sessions"], 0);
    assert_eq!(stopped["sessions"].as_array().unwrap().len(), 1);
    assert!(
        stopped["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().is_some_and(|warning| {
                warning.contains("may have spawned")
                    && warning.contains("process identity was durably recorded")
            }))
    );

    let forgotten = dev_stop(
        DevStopRequest::new("demo", temp.path().to_path_buf(), Some(state_dir))
            .with_forget_ambiguous_orphans(true),
    )
    .unwrap();
    assert_eq!(forgotten["ok"], true);
    assert_eq!(forgotten["stopped_sessions"], 1);
    assert!(forgotten["sessions"].as_array().unwrap().is_empty());
    assert_eq!(forgotten["warnings"], json!([]));
    let recovery = &forgotten["recoveries"][0];
    assert_eq!(recovery["kind"], "ambiguous-orphan-forgotten");
    assert_eq!(recovery["forgot_ambiguous_spawn"], true);
    assert_eq!(recovery["apps"][0]["name"], "web");
    assert_eq!(recovery["apps"][0]["target_port"], 4005);
    assert_eq!(recovery["apps"][0]["pid"], json!(null));
    assert_eq!(recovery["apps"][0]["spawn_state"], "pending");
    assert!(recovery["message"].as_str().is_some_and(
        |message| message.contains("explicitly forgot")
            && message.contains("ambiguous spawn history")
            && message.contains("may still be running")
    ));
}

#[test]
fn confirmed_failed_spawn_does_not_poison_later_orphan_recovery() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("proxy-state");
    let store = StateStore::resolve(Some(state_dir.clone())).unwrap();
    let runtime = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        &[
            lifecycle_spec(temp.path(), "web", "web.demo.localhost", false),
            lifecycle_spec(temp.path(), "admin", "admin.demo.localhost", false),
        ],
        false,
    )
    .unwrap();
    runtime.prepare_app_spawn("web", 4005).unwrap();
    assert!(matches!(
        runtime
            .record_app_process_interruptible(
                "web",
                4005,
                state::DevProcessIdentity {
                    pid: u32::MAX - 1,
                    start_token: Some("retired-app".into()),
                },
                &|| false,
            )
            .unwrap(),
        state::LockOutcome::Acquired(())
    ));
    let _unconfirmed_web_cleanup = runtime.arm_cleanup();
    runtime.prepare_app_spawn("admin", 4006).unwrap();
    let mut admin_cleanup = runtime.arm_cleanup();
    assert!(
        runtime
            .confirm_app_spawn_absent_cleanup_cancelable("admin", &|| false)
            .unwrap()
            .is_some()
    );
    admin_cleanup.confirm();
    drop(runtime);

    store
        .mutate_dev_sessions(|sessions, _| {
            sessions[0].supervisor = state::DevProcessIdentity {
                pid: u32::MAX,
                start_token: Some("retired-supervisor".into()),
            };
            Ok(())
        })
        .unwrap();

    let persisted = store.snapshot_dev_state().unwrap();
    assert!(!persisted.sessions[0].apps[1].spawn_pending);
    let stopped = dev_stop(DevStopRequest::new(
        "demo",
        temp.path().to_path_buf(),
        Some(state_dir),
    ))
    .unwrap();
    assert_eq!(stopped["ok"], true);
    assert_eq!(stopped["stopped_sessions"], 1);
    assert!(store.snapshot_dev_state().unwrap().sessions.is_empty());
}

#[test]
fn uncertain_live_identities_remain_alive_without_claiming_verification() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("proxy-state");
    let store = StateStore::resolve(Some(state_dir.clone())).unwrap();
    let runtime = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        &[lifecycle_spec(
            temp.path(),
            "web",
            "web.demo.localhost",
            false,
        )],
        false,
    )
    .unwrap();
    runtime.prepare_app_spawn("web", 4005).unwrap();
    assert!(matches!(
        runtime
            .record_app_process_interruptible(
                "web",
                4005,
                state::DevProcessIdentity {
                    pid: std::process::id(),
                    start_token: None,
                },
                &|| false,
            )
            .unwrap(),
        state::LockOutcome::Acquired(())
    ));
    let original_supervisor = store.snapshot_dev_state().unwrap().sessions[0]
        .supervisor
        .clone();
    store
        .mutate_dev_sessions(|sessions, _| {
            sessions[0].supervisor.start_token = None;
            sessions[0].control.port = 9;
            Ok(())
        })
        .unwrap();

    let status = dev_status(DevStatusRequest::new(
        "demo",
        temp.path().to_path_buf(),
        Some(state_dir),
    ))
    .unwrap();
    let session = &status["sessions"][0];
    assert_eq!(status["running"], true);
    assert_eq!(session["status"], "starting");
    assert_eq!(session["control_alive"], false);
    assert_eq!(session["supervisor_alive"], true);
    assert_eq!(session["supervisor_identity_verified"], false);
    assert_eq!(session["supervisor_observation"], "uncertain");
    assert_eq!(session["apps"][0]["alive"], true);
    assert_eq!(session["apps"][0]["identity_verified"], false);
    assert_eq!(session["apps"][0]["identity_observation"], "uncertain");

    store
        .mutate_dev_sessions(|sessions, _| {
            sessions[0].supervisor = original_supervisor;
            Ok(())
        })
        .unwrap();
    drop(runtime);
}

#[test]
fn legacy_missing_process_identity_requires_explicit_forget_after_spawn_tracking_upgrade() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("proxy-state");
    let store = StateStore::resolve(Some(state_dir.clone())).unwrap();
    let runtime = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        &[lifecycle_spec(
            temp.path(),
            "web",
            "web.demo.localhost",
            false,
        )],
        false,
    )
    .unwrap();
    let _unconfirmed_cleanup = runtime.arm_cleanup();
    drop(runtime);

    store
        .mutate_dev_sessions(|sessions, _| {
            sessions[0].supervisor = state::DevProcessIdentity {
                pid: u32::MAX,
                start_token: Some("retired-supervisor".into()),
            };
            sessions[0].apps[0].spawn_state_tracked = false;
            Ok(())
        })
        .unwrap();

    let stopped = dev_stop(DevStopRequest::new(
        "demo",
        temp.path().to_path_buf(),
        Some(state_dir.clone()),
    ))
    .unwrap();
    assert_eq!(stopped["ok"], false);
    assert_eq!(stopped["stopped_sessions"], 0);
    assert!(
        stopped["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().is_some_and(|warning| {
                warning.contains("predates durable spawn-state tracking")
            }))
    );

    let forgotten = dev_stop(
        DevStopRequest::new("demo", temp.path().to_path_buf(), Some(state_dir))
            .with_forget_ambiguous_orphans(true),
    )
    .unwrap();
    assert_eq!(forgotten["ok"], true);
    assert_eq!(forgotten["stopped_sessions"], 1);
    assert!(forgotten["sessions"].as_array().unwrap().is_empty());
    assert_eq!(forgotten["warnings"], json!([]));
    assert!(
        forgotten["recoveries"][0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("explicitly forgot")
                && message.contains("ambiguous spawn history"))
    );
}

#[test]
fn partial_stop_separates_successful_recoveries_from_blocking_warnings() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("proxy-state");
    let store = StateStore::resolve(Some(state_dir.clone())).unwrap();
    let recoverable = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        &[lifecycle_spec(
            temp.path(),
            "web",
            "web.demo.localhost",
            false,
        )],
        false,
    )
    .unwrap();
    let _recoverable_cleanup = recoverable.arm_cleanup();
    drop(recoverable);

    let blocked = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        &[lifecycle_spec(
            temp.path(),
            "admin",
            "admin.demo.localhost",
            false,
        )],
        false,
    )
    .unwrap();
    let _blocked_cleanup = blocked.arm_cleanup();
    drop(blocked);

    store
        .mutate_dev_sessions(|sessions, _| {
            for session in sessions {
                session.supervisor = state::DevProcessIdentity {
                    pid: u32::MAX,
                    start_token: Some("retired-supervisor".into()),
                };
                if let Some(app) = session.apps.iter_mut().find(|app| app.name == "admin") {
                    app.process = Some(state::DevProcessIdentity {
                        pid: std::process::id(),
                        start_token: state::process_start_token(std::process::id()),
                    });
                }
            }
            Ok(())
        })
        .unwrap();

    let stopped = dev_stop(DevStopRequest::new(
        "demo",
        temp.path().to_path_buf(),
        Some(state_dir),
    ))
    .unwrap();

    assert_eq!(stopped["ok"], false);
    assert_eq!(stopped["matched_sessions"], 2);
    assert_eq!(stopped["stopped_sessions"], 1);
    assert_eq!(stopped["recoveries"].as_array().unwrap().len(), 1);
    assert_eq!(stopped["recoveries"][0]["apps"][0]["name"], "web");
    let warnings = stopped["warnings"].as_array().unwrap();
    assert!(warnings.iter().any(|warning| {
        warning
            .as_str()
            .is_some_and(|warning| warning.contains("registered app 'admin'"))
    }));
    assert!(warnings.iter().all(|warning| {
        warning
            .as_str()
            .is_some_and(|warning| !warning.contains("retired a dead orphan"))
    }));
}

#[test]
fn confirmed_cleanup_retires_the_exact_session() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().join("proxy-state"))).unwrap();
    let runtime = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "demo",
        temp.path(),
        &[lifecycle_spec(
            temp.path(),
            "web",
            "web.demo.localhost",
            false,
        )],
        false,
    )
    .unwrap();
    let mut cleanup = runtime.arm_cleanup();
    cleanup.confirm();
    drop(runtime);

    assert!(store.snapshot_dev_state().unwrap().sessions.is_empty());
}
