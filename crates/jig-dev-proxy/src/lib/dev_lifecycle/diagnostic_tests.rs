use super::*;

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
    assert!(error.contains("jig dev stop --state-dir PATH"));
    assert!(error.contains("jig dev --replace"));
    let session_id = &store.snapshot_dev_state().unwrap().sessions[0].session_id;
    assert!(error.contains(session_id));
    assert!(error.contains(&store.root().display().to_string()));
    assert_eq!(store.snapshot_dev_state().unwrap().sessions.len(), 1);

    drop(runtime);
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

    let session_id = &store.snapshot_dev_state().unwrap().sessions[0].session_id;
    assert!(error.contains(session_id));
    assert!(error.contains("activity verified"));
    assert!(error.contains(&store.root().display().to_string()));
    assert!(error.contains("Cross-repository ownership"));
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
fn cross_repo_conflict_reports_every_overlapping_session() {
    let temp = tempdir().unwrap();
    let roots = ["ExampleProjectA", "ExampleProjectB", "ExampleProjectC"]
        .map(|name| temp.path().join(name));
    for root in &roots {
        std::fs::create_dir_all(root).unwrap();
    }
    let store = StateStore::resolve(Some(temp.path().join("proxy-state"))).unwrap();
    let first = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "ExampleProjectA",
        &roots[0],
        &[lifecycle_spec(
            &roots[0],
            "web",
            "web.example.localhost",
            true,
        )],
        false,
    )
    .unwrap();
    let second = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "ExampleProjectB",
        &roots[1],
        &[lifecycle_spec(
            &roots[1],
            "api",
            "api.example.localhost",
            true,
        )],
        false,
    )
    .unwrap();
    let ids = store
        .snapshot_dev_state()
        .unwrap()
        .sessions
        .into_iter()
        .map(|session| session.session_id)
        .collect::<Vec<_>>();

    let error = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "ExampleProjectC",
        &roots[2],
        &[
            lifecycle_spec(&roots[2], "web", "web.example.localhost", true),
            lifecycle_spec(&roots[2], "api", "api.example.localhost", true),
        ],
        true,
    )
    .err()
    .expect("both cross-repository claims block replacement")
    .to_string();

    for id in &ids {
        assert!(error.contains(id), "missing claimant {id}: {error}");
    }
    assert!(error.contains("web.example.localhost"));
    assert!(error.contains("api.example.localhost"));
    assert!(error.contains("Additional cross-repository claims"));
    assert!(error.contains(&store.root().display().to_string()));
    assert_eq!(store.snapshot_dev_state().unwrap().sessions.len(), 2);
    drop(second);
    drop(first);
}

#[test]
fn dead_cross_repo_claim_reports_exact_cleanup_without_changing_state() {
    let temp = tempdir().unwrap();
    let repo_a = temp.path().join("ExampleProject");
    let repo_b = temp.path().join("ExampleOtherProject");
    std::fs::create_dir_all(&repo_a).unwrap();
    std::fs::create_dir_all(&repo_b).unwrap();
    let state_dir = temp.path().join("proxy-state");
    let store = StateStore::resolve(Some(state_dir.clone())).unwrap();
    let runtime = dev_sessions::DevSessionRuntime::start(
        store.clone(),
        "ExampleProject",
        &repo_a,
        &[lifecycle_spec(&repo_a, "web", "shared.localhost", true)],
        false,
    )
    .unwrap();
    let _cleanup = runtime.arm_cleanup();
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
    let recorded = store.snapshot_dev_state().unwrap().sessions.remove(0);
    assert!(store.snapshot_dev_state().unwrap().routes.is_empty());
    let session_bytes = std::fs::read(state_dir.join("dev-sessions.json")).unwrap();
    let route_bytes = std::fs::read(state_dir.join("routes.json")).ok();

    let status = dev_status(DevStatusRequest::new(
        "ExampleProject",
        repo_a,
        Some(state_dir.clone()),
    ))
    .unwrap();
    assert_eq!(status["activity"], "none");
    assert_eq!(status["cleanup_required"], true);
    assert_eq!(status["sessions"][0]["session_id"], recorded.session_id);
    assert_eq!(status["sessions"][0]["retention_reason"], Value::Null);
    assert_eq!(
        std::fs::read(state_dir.join("dev-sessions.json")).unwrap(),
        session_bytes
    );
    assert_eq!(
        std::fs::read(state_dir.join("routes.json")).ok(),
        route_bytes
    );

    let error = dev_sessions::DevSessionRuntime::start(
        store,
        "ExampleOtherProject",
        &repo_b,
        &[lifecycle_spec(&repo_b, "web", "shared.localhost", true)],
        true,
    )
    .err()
    .expect("cross-repository cleanup obligation remains reserved")
    .to_string();
    assert!(error.contains(&recorded.session_id));
    assert!(error.contains("Development hostname 'shared.localhost'"));
    assert!(error.contains("activity none"));
    assert!(error.contains("cleanup required true"));
    assert!(error.contains(&state_dir.display().to_string()));
    assert!(!error.contains("live Jig dev session"));
    assert!(!error.contains("Development route"));
    assert!(!error.contains(&recorded.control.token));
    assert_eq!(
        std::fs::read(state_dir.join("dev-sessions.json")).unwrap(),
        session_bytes
    );
    assert_eq!(
        std::fs::read(state_dir.join("routes.json")).ok(),
        route_bytes
    );
}
