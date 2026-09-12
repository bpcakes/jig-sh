use super::*;

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
                        && warning.contains("jig dev status --json")
                        && warning.contains("independently verify and stop surviving app processes")
                })
            })
    );
}
