use super::*;

pub(super) fn assert_unconfirmed_orphan(persisted: &state::DevStateSnapshot) {
    assert_eq!(persisted.sessions.len(), 1);
    assert!(persisted.sessions[0].cleanup_required);
    assert_eq!(
        persisted.sessions[0].phase,
        state::DevSessionPhase::Orphaned
    );
}

pub(super) fn configure_dead_orphan_routes(store: &StateStore) {
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
}

pub(super) fn assert_recoverable_status(status: &Value) {
    assert_eq!(status["running"], false);
    assert_eq!(status["sessions"][0]["status"], "recoverable");
    assert_eq!(status["sessions"][0]["cleanup_required"], true);
    assert_eq!(status["sessions"][0]["recoverable"], true);
}

pub(super) fn assert_dead_orphan_recovery(stopped: &Value) {
    assert_eq!(stopped["ok"], true);
    assert_eq!(stopped["matched_sessions"], 1);
    assert_eq!(stopped["stopped_sessions"], 1);
    assert_eq!(stopped["stopped_apps"], 0);
    assert_eq!(stopped["sessions"], json!([]));
    assert_eq!(stopped["warnings"], json!([]));
    let recovery = &stopped["recoveries"][0];
    assert_eq!(recovery["kind"], "dead-orphan-retired");
    assert_eq!(recovery["forgotten_ambiguities"], json!([]));
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
}

pub(super) fn assert_only_unowned_routes_remain(store: &StateStore) {
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
