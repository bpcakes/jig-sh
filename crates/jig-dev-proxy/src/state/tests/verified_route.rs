use super::*;
use crate::test_tempdir as tempdir;

#[test]
fn verified_route_rolls_back_if_post_write_verification_fails() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let mut calls = 0usize;
    store
        .add_route(Route {
            hostname: "web.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 3999,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        })
        .unwrap();

    let error = store
        .add_verified_route(
            Route {
                hostname: "web.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 4000,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            },
            |_| {
                calls += 1;
                if calls == 2 {
                    Err(anyhow::anyhow!("listener changed after publish"))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err()
        .to_string();

    assert!(error.contains("listener changed"));
    assert_eq!(calls, 2);
    let routes = store.read_routes(false).unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].target_port, 3999);
}
