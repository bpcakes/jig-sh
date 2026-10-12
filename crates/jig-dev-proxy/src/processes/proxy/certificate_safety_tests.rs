use super::*;
use crate::types::{AppRunSpec, CommandSpec};

fn certificate_snapshot(store: &StateStore) -> Vec<Vec<u8>> {
    [
        store.ca_path(),
        store.ca_key_path(),
        store.leaf_path(),
        store.leaf_key_path(),
        store.leaf_hosts_path(),
    ]
    .into_iter()
    .map(|path| fs::read(path).unwrap())
    .collect()
}

#[test]
fn incomplete_runtime_blocks_start_even_with_unused_requested_ports() {
    for name in [
        "proxy.pid",
        "proxy-exe.txt",
        "proxy-http.port",
        "proxy-https.port",
        "proxy-health-token",
    ] {
        let temp = crate::test_tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().join("isolated-proxy-state"))).unwrap();
        let path = store.root().join(name);
        fs::write(&path, b"incomplete-example-runtime").unwrap();
        let settings = ProxySettings {
            state_dir: Some(store.root().into()),
            http_port: 0,
            ..ProxySettings::default()
        };
        let error = ensure_proxy_running_interruptible(
            &store,
            &settings,
            Path::new("unused-example-proxy-executable"),
            &|| false,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("readiness") && error.contains("unconfirmed"),
            "{name}: {error}"
        );
        assert_eq!(fs::read(path).unwrap(), b"incomplete-example-runtime");
        assert!(!store.log_path().exists(), "no proxy startup was attempted");
    }
}

#[test]
fn both_launch_paths_preserve_certificates_when_health_is_inconclusive() {
    for dev_session in [false, true] {
        let temp = crate::test_tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().join("isolated-proxy-state"))).unwrap();
        // A bound listener that never answers health is live, but its listener
        // settings cannot be verified. It must never authorize TLS changes.
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let settings = ProxySettings {
            state_dir: Some(store.root().into()),
            http_port: listener.local_addr().unwrap().port(),
            https: true,
            ..ProxySettings::default()
        };
        crate::certs::ensure_for_hosts(&settings, &["existing.example.localhost".into()]).unwrap();
        let before = certificate_snapshot(&store);
        store.write_http_port(settings.http_port).unwrap();
        store.write_pid(std::process::id()).unwrap();
        store.ensure_health_token().unwrap();
        let marker = temp.path().join("example-app-spawn-marker");
        let spec = AppRunSpec::new(
            "web",
            temp.path().into(),
            CommandSpec::Argv(vec!["touch".into(), marker.display().to_string()]),
            "web.example.localhost",
        )
        .with_proxy(true);
        let requested = ProxySettings {
            lan: true,
            ..settings
        };
        let executable = Path::new("unused-example-proxy-executable");
        let result = if dev_session {
            super::super::run_apps_with_interrupt_probe(vec![spec], &requested, executable, || None)
        } else {
            super::super::run_app_with_interrupt_probe(spec, &requested, executable, || None)
        };
        let error = format!("{:#}", result.unwrap_err());
        assert!(
            error.contains("readiness") && error.contains("unconfirmed"),
            "{error}"
        );
        assert_eq!(certificate_snapshot(&store), before);
        assert!(!marker.exists());
        assert!(store.read_routes(false).unwrap().is_empty());
        assert!(!store.log_path().exists());
    }
}

#[test]
fn certificate_preparation_rechecks_readiness_before_any_write() {
    let temp = crate::test_tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().join("isolated-proxy-state"))).unwrap();
    let settings = ProxySettings {
        state_dir: Some(store.root().into()),
        https: true,
        ..ProxySettings::default()
    };
    crate::certs::ensure_for_hosts(&settings, &["existing.example.localhost".into()]).unwrap();
    let before = certificate_snapshot(&store);
    let error = super::super::prepare_certs_for_hosts_interruptible(
        &settings,
        &["new.example.localhost".into()],
        &|| None,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("readiness"));
    assert_eq!(certificate_snapshot(&store), before);
}
