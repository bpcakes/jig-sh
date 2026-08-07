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
    assert_eq!(repeated["ok"], true);
    assert_eq!(repeated["matched_sessions"], 0);
    assert_eq!(repeated["stopped_sessions"], 0);
    assert!(store.snapshot_dev_state().unwrap().sessions.is_empty());
}

#[test]
fn unconfirmed_cleanup_stays_visible_and_stop_fails_closed() {
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
    assert_eq!(status["sessions"][0]["cleanup_required"], true);

    let stopped = dev_stop(DevStopRequest::new(
        "demo",
        temp.path().to_path_buf(),
        Some(state_dir),
    ))
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
                warning
                    .as_str()
                    .is_some_and(|warning| warning.contains("without signaling numeric PIDs"))
            })
    );
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
