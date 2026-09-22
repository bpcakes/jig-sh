use super::*;
use std::sync::{Arc, Barrier};

fn retained_example_session(store: &StateStore, root: &Path, name: &str, hostname: &str) -> String {
    let runtime = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "ExampleProject",
        root,
        &[lifecycle_spec(root, name, hostname, false)],
        false,
    )
    .unwrap();
    let id = store.snapshot_dev_state().unwrap().sessions[0]
        .session_id
        .clone();
    let _cleanup_pending = runtime.arm_cleanup();
    drop(runtime);
    store
        .mutate_dev_sessions(|sessions, _| {
            let session = sessions
                .iter_mut()
                .find(|session| session.session_id == id)
                .unwrap();
            session.supervisor = state::DevProcessIdentity {
                pid: u32::MAX,
                start_token: Some("retired-example-supervisor".into()),
            };
            Ok(())
        })
        .unwrap();
    id
}

fn add_dead_owned_route(store: &StateStore, id: &str, hostname: &str) -> Route {
    let route = Route {
        hostname: hostname.into(),
        target_host: "127.0.0.1".into(),
        target_port: 4000,
        owner_pid: Some(u32::MAX),
        owner_start_token: Some("retired-example-app".into()),
        mode: RouteMode::Process,
        created_at_ms: 1,
    };
    store
        .mutate_dev_state_interruptible(&|| false, |sessions, routes| {
            let session = sessions
                .iter_mut()
                .find(|session| session.session_id == id)
                .unwrap();
            session.apps[0].hostname = Some(hostname.into());
            session.apps[0].target_port = Some(4000);
            session.apps[0].process = Some(state::DevProcessIdentity {
                pid: u32::MAX,
                start_token: Some("retired-example-app".into()),
            });
            routes.push(route.clone());
            Ok(())
        })
        .unwrap();
    route
}

#[test]
fn normal_launch_retires_dead_same_repo_claim_without_routes() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().join("isolated-proxy-state"))).unwrap();
    let old_id = retained_example_session(&store, temp.path(), "web", "web.example.localhost");
    assert!(store.snapshot_dev_state().unwrap().routes.is_empty());

    let next = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "ExampleProject",
        temp.path(),
        &[lifecycle_spec(
            temp.path(),
            "web",
            "web.example.localhost",
            false,
        )],
        false,
    )
    .unwrap();
    let snapshot = store.snapshot_dev_state().unwrap();
    assert_eq!(snapshot.sessions.len(), 1);
    assert_ne!(snapshot.sessions[0].session_id, old_id);
    assert_eq!(next.replacement_recoveries().len(), 1);
    let notice = &next.replacement_recoveries()[0];
    let notice_value = serde_json::to_value(notice).unwrap();
    assert_eq!(notice_value["session_id"], old_id);
    assert!(
        notice_value["message"]
            .as_str()
            .unwrap()
            .contains("without signaling persisted PIDs")
    );

    let failed = processes::finalize_claimed_dev_session_result(
        Err(anyhow::anyhow!("example app startup failed")),
        &next,
    );
    let output = dev_api::normalize_dev_result(failed).unwrap();
    assert_eq!(output["ok"], false);
    assert_eq!(output["recoveries"][0]["session_id"], old_id);

    let cancelled = processes::finalize_claimed_dev_session_result(
        Err(processes::interruption_error(
            processes::TerminationReason::requested_stop(),
        )),
        &next,
    );
    let output = dev_api::normalize_dev_result(cancelled).unwrap();
    assert_eq!(output["stopped"], true);
    assert_eq!(output["recoveries"][0]["session_id"], old_id);
}

#[test]
fn normal_launch_retires_only_exact_owned_routes_and_preserves_unrelated_state() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().join("isolated-proxy-state"))).unwrap();
    let old_id = retained_example_session(&store, temp.path(), "web", "web.example.localhost");
    let old_route = add_dead_owned_route(&store, &old_id, "web.example.localhost");
    let unrelated = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "ExampleProject",
        temp.path(),
        &[lifecycle_spec(
            temp.path(),
            "worker",
            "worker.example.localhost",
            false,
        )],
        false,
    )
    .unwrap();
    let unrelated_id = store
        .snapshot_dev_state()
        .unwrap()
        .sessions
        .iter()
        .find(|session| session.session_id != old_id)
        .unwrap()
        .session_id
        .clone();
    let alias = Route {
        hostname: "alias.example.localhost".into(),
        target_host: "127.0.0.1".into(),
        target_port: 4001,
        owner_pid: None,
        owner_start_token: None,
        mode: RouteMode::Alias,
        created_at_ms: 2,
    };
    store.add_alias_route(alias.clone()).unwrap();
    let replacement_generation = Route {
        hostname: "other.example.localhost".into(),
        target_host: "127.0.0.1".into(),
        target_port: 4002,
        owner_pid: Some(u32::MAX),
        owner_start_token: Some("other-generation".into()),
        mode: RouteMode::Process,
        created_at_ms: 3,
    };
    store
        .mutate_dev_state_interruptible(&|| false, |_, routes| {
            routes.push(replacement_generation.clone());
            Ok(())
        })
        .unwrap();

    let next = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "ExampleProject",
        temp.path(),
        &[lifecycle_spec(
            temp.path(),
            "web",
            "web.example.localhost",
            false,
        )],
        false,
    )
    .unwrap();
    assert_eq!(next.replacement_recoveries().len(), 1);
    let snapshot = store.snapshot_dev_state().unwrap();
    assert_eq!(snapshot.sessions.len(), 2);
    assert!(
        !snapshot
            .sessions
            .iter()
            .any(|session| session.session_id == old_id)
    );
    assert!(
        snapshot
            .sessions
            .iter()
            .any(|session| session.session_id == unrelated_id)
    );
    assert!(!snapshot.routes.contains(&old_route));
    assert!(snapshot.routes.contains(&alias));
    assert!(snapshot.routes.contains(&replacement_generation));
    drop(unrelated);
}

#[test]
fn normal_launch_retries_after_route_or_session_write_failure() {
    for fail_session_write in [false, true] {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().join("isolated-proxy-state"))).unwrap();
        let old_id = retained_example_session(&store, temp.path(), "web", "web.example.localhost");
        let old_route = add_dead_owned_route(&store, &old_id, "web.example.localhost");
        if fail_session_write {
            state::fail_session_write_once();
        } else {
            state::fail_route_write_once();
        }
        let error = dev_sessions::DevSessionRuntime::start(
            store.clone(),
            "ExampleProject",
            temp.path(),
            &[lifecycle_spec(
                temp.path(),
                "web",
                "web.example.localhost",
                false,
            )],
            false,
        )
        .err()
        .expect("injected write failure must abort claim");
        assert!(format!("{error:#}").contains("injected"));
        let snapshot = store.snapshot_dev_state().unwrap();
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.sessions[0].session_id, old_id);
        assert_eq!(snapshot.routes.contains(&old_route), !fail_session_write);

        let next = dev_sessions::DevSessionRuntime::start(
            store.clone(),
            "ExampleProject",
            temp.path(),
            &[lifecycle_spec(
                temp.path(),
                "web",
                "web.example.localhost",
                false,
            )],
            false,
        )
        .unwrap();
        assert_eq!(next.replacement_recoveries().len(), 1);
        let snapshot = store.snapshot_dev_state().unwrap();
        assert_eq!(snapshot.sessions.len(), 1);
        assert_ne!(snapshot.sessions[0].session_id, old_id);
        assert!(snapshot.routes.is_empty());
    }
}

#[test]
fn normal_launch_treats_reused_pid_as_absent_without_signaling_it() {
    let Some(current_token) = state::process_start_token(std::process::id()) else {
        return;
    };
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().join("isolated-proxy-state"))).unwrap();
    let old_id = retained_example_session(&store, temp.path(), "web", "web.example.localhost");
    let reused_pid = std::process::id();
    store
        .mutate_dev_sessions(|sessions, _| {
            let session = sessions
                .iter_mut()
                .find(|session| session.session_id == old_id)
                .unwrap();
            session.supervisor = state::DevProcessIdentity {
                pid: reused_pid,
                start_token: Some("previous-example-generation".into()),
            };
            session.apps[0].process = Some(state::DevProcessIdentity {
                pid: reused_pid,
                start_token: Some("previous-example-generation".into()),
            });
            Ok(())
        })
        .unwrap();

    let next = dev_sessions::DevSessionRuntime::start(
        store,
        "ExampleProject",
        temp.path(),
        &[lifecycle_spec(
            temp.path(),
            "web",
            "web.example.localhost",
            false,
        )],
        false,
    )
    .unwrap();
    assert_eq!(next.replacement_recoveries().len(), 1);
    assert_eq!(state::process_start_token(reused_pid), Some(current_token));
}

#[test]
fn concurrent_normal_launch_has_one_owner() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().join("isolated-proxy-state"))).unwrap();
    retained_example_session(&store, temp.path(), "web", "web.example.localhost");
    let start = Arc::new(Barrier::new(2));
    let finish = Arc::new(Barrier::new(2));
    let root = temp.path().to_path_buf();
    let outcomes = std::thread::scope(|scope| {
        let workers = (0..2)
            .map(|_| {
                let store = store.clone();
                let root = root.clone();
                let start = start.clone();
                let finish = finish.clone();
                scope.spawn(move || {
                    start.wait();
                    let result = dev_sessions::DevSessionRuntime::start(
                        store,
                        "ExampleProject",
                        &root,
                        &[lifecycle_spec(&root, "web", "web.example.localhost", false)],
                        false,
                    );
                    let outcome = result
                        .as_ref()
                        .map(|runtime| runtime.replacement_recoveries().len())
                        .map_err(|error| error.to_string());
                    finish.wait();
                    (outcome, result.ok())
                })
            })
            .collect::<Vec<_>>();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        outcomes
            .iter()
            .filter(|(outcome, _)| outcome.is_ok())
            .count(),
        1
    );
    assert!(outcomes.iter().any(|(outcome, _)| *outcome == Ok(1)));
    let snapshot = store.snapshot_dev_state().unwrap();
    assert_eq!(snapshot.sessions.len(), 1);
}

#[test]
fn normal_launch_retains_incomplete_and_live_claims() {
    let cases = [
        ("preflight", "preflight-cleanup-pending"),
        ("spawn", "app-spawn-pending"),
        ("untracked", "app-spawn-untracked"),
        ("supervisor-uncertain", "supervisor-uncertain"),
        ("app-uncertain", "app-uncertain"),
        ("supervisor-alive", "supervisor-alive"),
        ("app-alive", "app-alive"),
    ];
    for (case, reason) in cases {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().join("isolated-proxy-state"))).unwrap();
        let old_id = retained_example_session(&store, temp.path(), "web", "web.example.localhost");
        let own_token = state::process_start_token(std::process::id());
        if matches!(case, "supervisor-alive" | "app-alive") && own_token.is_none() {
            continue;
        }
        store
            .mutate_dev_sessions(|sessions, _| {
                let session = sessions
                    .iter_mut()
                    .find(|session| session.session_id == old_id)
                    .unwrap();
                match case {
                    "preflight" => session.preflight_cleanup_pending = Some(true),
                    "spawn" => {
                        session.apps[0].target_port = Some(4000);
                        session.apps[0].spawn_pending = true;
                    }
                    "untracked" => session.apps[0].spawn_state_tracked = false,
                    "supervisor-uncertain" => {
                        session.supervisor = state::DevProcessIdentity {
                            pid: std::process::id(),
                            start_token: None,
                        }
                    }
                    "supervisor-alive" => {
                        session.supervisor = state::DevProcessIdentity {
                            pid: std::process::id(),
                            start_token: own_token.clone(),
                        }
                    }
                    "app-uncertain" | "app-alive" => {
                        session.apps[0].target_port = Some(4000);
                        session.apps[0].process = Some(state::DevProcessIdentity {
                            pid: std::process::id(),
                            start_token: (case == "app-alive").then(|| own_token.clone()).flatten(),
                        });
                    }
                    _ => unreachable!(),
                }
                Ok(())
            })
            .unwrap();

        let error = dev_sessions::DevSessionRuntime::start(
            store.clone(),
            "ExampleProject",
            temp.path(),
            &[lifecycle_spec(
                temp.path(),
                "web",
                "web.example.localhost",
                false,
            )],
            false,
        )
        .err()
        .expect("incomplete or live claim must block normal launch")
        .to_string();
        assert!(error.contains(reason), "{case}: {error}");
        let snapshot = store.snapshot_dev_state().unwrap();
        assert_eq!(snapshot.sessions.len(), 1, "{case}");
        assert_eq!(snapshot.sessions[0].session_id, old_id, "{case}");
    }
}
