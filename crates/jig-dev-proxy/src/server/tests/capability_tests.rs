use super::*;
use crate::ports::jig_proxy_capabilities;

#[test]
fn capabilities_require_loopback_addresses_host_and_token() {
    let token = "a".repeat(64);
    let caps = ProxyCapabilities {
        pid: std::process::id(),
        lan: true,
        https: true,
        http2: false,
    };
    let request = Request::builder()
        .uri(crate::ports::CAPABILITIES_PATH)
        .header(HOST, "localhost")
        .header("x-jig-proxy-health-token", &token)
        .body(())
        .unwrap();
    let loopback = "127.0.0.1".parse().unwrap();
    let remote = "192.0.2.10".parse().unwrap();
    let context = RequestContext {
        remote_addr: SocketAddr::new(loopback, 4000),
        proxy_port: 1355,
        tls: false,
        local_ip: loopback,
        lan_ip: None,
        health_token: Arc::from(token.as_str()),
        capabilities: caps,
    };

    let response = internal_probe_response(&request, &context).unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-jig-proxy-capabilities-version"], "1");
    assert_eq!(response.headers()["x-jig-proxy-lan"], "1");
    assert_eq!(response.headers()["x-jig-proxy-http2"], "0");
    let health = Request::builder()
        .uri("/__jig_proxy_health")
        .header(HOST, "localhost")
        .header("x-jig-proxy-health-token", &token)
        .body(())
        .unwrap();
    let legacy = internal_probe_response(&health, &context).unwrap();
    assert_eq!(legacy.status(), StatusCode::OK);
    assert_eq!(legacy.headers()["x-jig-proxy"], "1");
    assert!(
        legacy
            .headers()
            .get("x-jig-proxy-capabilities-version")
            .is_none()
    );

    let lan_context = RequestContext {
        remote_addr: SocketAddr::new(remote, 4000),
        ..context.clone()
    };
    let response = internal_probe_response(&request, &lan_context).unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(response.headers().get("x-jig-proxy-lan").is_none());
    let external_listener = RequestContext {
        local_ip: remote,
        ..context.clone()
    };
    assert_eq!(
        internal_probe_response(&request, &external_listener)
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let wrong_host = Request::builder()
        .uri(crate::ports::CAPABILITIES_PATH)
        .header(HOST, "web.example.localhost")
        .header("x-jig-proxy-health-token", &token)
        .body(())
        .unwrap();
    assert_eq!(
        internal_probe_response(&wrong_host, &context)
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let wrong_token = Request::builder()
        .uri(crate::ports::CAPABILITIES_PATH)
        .header(HOST, "localhost")
        .header("x-jig-proxy-health-token", "wrong")
        .body(())
        .unwrap();
    assert_eq!(
        internal_probe_response(&wrong_token, &context)
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serving_runtime_reports_effective_capabilities() {
    for (lan, https, http2) in [
        (false, false, true),
        (true, false, true),
        (false, true, false),
        (false, true, true),
    ] {
        let temp = tempdir().unwrap();
        let state_dir = temp.path().join("isolated-proxy-state");
        let store = StateStore::resolve(Some(state_dir.clone())).unwrap();
        let settings = ProxySettings {
            state_dir: Some(state_dir),
            http_port: 0,
            https_port: Some(0),
            https,
            http2,
            lan,
            ..ProxySettings::default()
        };
        let run = tokio::spawn(run_bound(
            settings,
            store.clone(),
            std::env::current_exe().unwrap(),
            Arc::new(AtomicBool::new(false)),
        ));
        let port = timeout(Duration::from_secs(10), async {
            loop {
                if let Some(port) = store.read_http_port().unwrap() {
                    break port;
                }
                assert!(
                    !run.is_finished(),
                    "proxy exited before publishing its port"
                );
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("isolated proxy became ready");
        let token = store.read_health_token().unwrap().unwrap();
        let health_token = token.clone();
        let health_pid = tokio::task::spawn_blocking(move || {
            crate::ports::jig_proxy_http_pid("127.0.0.1", port, Some(&health_token))
        })
        .await
        .unwrap();
        assert_eq!(health_pid, Some(std::process::id()));
        let actual =
            tokio::task::spawn_blocking(move || jig_proxy_capabilities("127.0.0.1", port, &token))
                .await
                .unwrap()
                .expect("authenticated capability response");
        assert_eq!(actual.pid, std::process::id());
        assert_eq!(actual.lan, lan);
        assert_eq!(actual.https, https);
        assert_eq!(actual.http2, https && http2);
        run.abort();
        let _ = run.await;
        store.clear_runtime_files();
    }
}

#[test]
fn health_request_requires_loopback_client_and_host() {
    let loopback = "127.0.0.1".parse().unwrap();
    let remote = "192.168.1.50".parse().unwrap();
    let token = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let localhost = Request::builder()
        .header(HOST, "localhost")
        .header("x-jig-proxy-health-token", token)
        .body(())
        .unwrap();
    let loopback_literal = Request::builder()
        .header(HOST, "127.0.0.1:1355")
        .header("x-jig-proxy-health-token", token)
        .body(())
        .unwrap();
    let ipv6_loopback = Request::builder()
        .header(HOST, "[::1]:1355")
        .header("x-jig-proxy-health-token", token)
        .body(())
        .unwrap();
    let malformed_ipv6_loopback = Request::builder()
        .header(HOST, "[::1]evil")
        .header("x-jig-proxy-health-token", token)
        .body(())
        .unwrap();
    let routed_host = Request::builder()
        .header(HOST, "web.demo.localhost")
        .header("x-jig-proxy-health-token", token)
        .body(())
        .unwrap();
    let wrong_token = Request::builder()
        .header(HOST, "localhost")
        .header("x-jig-proxy-health-token", "wrong")
        .body(())
        .unwrap();
    let missing_token = Request::builder()
        .header(HOST, "localhost")
        .body(())
        .unwrap();

    assert!(health_request_allowed(
        &localhost, loopback, loopback, token
    ));
    assert!(health_request_allowed(
        &loopback_literal,
        loopback,
        loopback,
        token
    ));
    assert!(health_request_allowed(
        &ipv6_loopback,
        "::1".parse().unwrap(),
        "::1".parse().unwrap(),
        token
    ));
    assert!(health_request_allowed(
        &localhost,
        "::ffff:127.0.0.1".parse().unwrap(),
        "::ffff:127.0.0.1".parse().unwrap(),
        token
    ));
    assert!(!health_request_allowed(&localhost, remote, loopback, token));
    assert!(!health_request_allowed(&localhost, loopback, remote, token));
    assert!(!health_request_allowed(
        &malformed_ipv6_loopback,
        loopback,
        loopback,
        token
    ));
    assert!(!health_request_allowed(
        &routed_host,
        loopback,
        loopback,
        token
    ));
    assert!(!health_request_allowed(
        &wrong_token,
        loopback,
        loopback,
        token
    ));
    assert!(!health_request_allowed(
        &missing_token,
        loopback,
        loopback,
        token
    ));
    assert!(!constant_time_ascii_eq("health-token-prefix", token));
    assert!(!constant_time_ascii_eq("short", "short"));
    assert!(constant_time_ascii_eq(token, token));
}
