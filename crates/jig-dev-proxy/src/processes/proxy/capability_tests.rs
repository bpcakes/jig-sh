use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::thread;

use super::*;
use crate::state::StateStore;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::types::{AppRunSpec, CommandSpec};
use crate::types::{Route, RouteMode};

#[derive(Clone, Copy)]
enum GenerationChange {
    None,
    Token,
    Pid,
}

#[derive(Clone, Copy)]
enum CapabilityReply {
    Available(ProxyCapabilities),
    Unsupported,
    Unavailable,
    Invalid,
}

struct FakeProxy {
    _temp: tempfile::TempDir,
    store: StateStore,
    http_port: u16,
    https_port: Option<u16>,
    _https_listener: Option<TcpListener>,
    server: thread::JoinHandle<()>,
}

impl FakeProxy {
    fn start(
        capabilities: Option<ProxyCapabilities>,
        https_listener: bool,
        change: GenerationChange,
    ) -> Self {
        Self::start_with_reply(
            capabilities.map_or(CapabilityReply::Unsupported, CapabilityReply::Available),
            https_listener,
            change,
        )
    }

    fn start_with_reply(
        reply: CapabilityReply,
        https_listener: bool,
        change: GenerationChange,
    ) -> Self {
        let temp = crate::test_tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().join("isolated-proxy-state"))).unwrap();
        let token = store.ensure_health_token().unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let http_port = listener.local_addr().unwrap().port();
        store.write_http_port(http_port).unwrap();
        store.write_pid(std::process::id()).unwrap();
        let https_listener = https_listener.then(|| TcpListener::bind(("127.0.0.1", 0)).unwrap());
        let https_port = https_listener
            .as_ref()
            .map(|listener| listener.local_addr().unwrap().port());
        if let Some(port) = https_port {
            store.write_https_port(port).unwrap();
        }
        let server_store = store.clone();
        let server = thread::spawn(move || {
            for path in ["/__jig_proxy_health", crate::ports::CAPABILITIES_PATH] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut chunk = [0u8; 512];
                loop {
                    let count = stream.read(&mut chunk).unwrap();
                    assert!(count > 0, "probe request closed before headers");
                    request.extend_from_slice(&chunk[..count]);
                    if request.ends_with(b"\r\n\r\n") {
                        break;
                    }
                    assert!(request.len() <= 1024);
                }
                let request = String::from_utf8(request).unwrap();
                assert!(request.starts_with(&format!("GET {path} HTTP/1.1\r\n")));
                assert!(request.contains(&format!("x-jig-proxy-health-token: {token}\r\n")));
                if path == "/__jig_proxy_health" {
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nx-jig-proxy: 1\r\nx-jig-proxy-pid: {}\r\ncontent-length: 0\r\n\r\n",
                        std::process::id()
                    )
                    .unwrap();
                    continue;
                }
                match change {
                    GenerationChange::None => {}
                    GenerationChange::Token => std::fs::write(
                        server_store.root().join("proxy-health-token"),
                        "b".repeat(64),
                    )
                    .unwrap(),
                    GenerationChange::Pid => server_store.write_pid(u32::MAX).unwrap(),
                }
                match reply {
                    CapabilityReply::Available(capabilities) => {
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nx-jig-proxy: 1\r\nx-jig-proxy-pid: {}\r\nx-jig-proxy-capabilities-version: 1\r\nx-jig-proxy-lan: {}\r\nx-jig-proxy-https: {}\r\nx-jig-proxy-http2: {}\r\ncontent-length: 0\r\n\r\n",
                            capabilities.pid,
                            u8::from(capabilities.lan),
                            u8::from(capabilities.https),
                            u8::from(capabilities.http2),
                        )
                        .unwrap();
                    }
                    CapabilityReply::Unsupported => stream
                        .write_all(b"HTTP/1.1 404 Not Found\r\nx-jig-proxy: 1\r\ncontent-length: 0\r\n\r\n")
                        .unwrap(),
                    CapabilityReply::Unavailable => stream
                        .write_all(b"HTTP/1.1 503 Service Unavailable\r\nx-jig-proxy: 1\r\ncontent-length: 0\r\n\r\n")
                        .unwrap(),
                    CapabilityReply::Invalid => stream
                        .write_all(b"HTTP/1.1 200 OK\r\nx-jig-proxy: 1\r\ncontent-length: 0\r\n\r\n")
                        .unwrap(),
                }
            }
        });
        Self {
            _temp: temp,
            store,
            http_port,
            https_port,
            _https_listener: https_listener,
            server,
        }
    }

    fn settings(&self, lan: bool, https: bool, http2: bool) -> ProxySettings {
        ProxySettings {
            state_dir: Some(self.store.root().to_path_buf()),
            http_port: self.http_port,
            https_port: self.https_port,
            lan,
            https,
            http2,
            ..ProxySettings::default()
        }
    }

    fn finish(self) {
        self.server.join().unwrap();
    }
}

fn caps(lan: bool, https: bool, http2: bool) -> ProxyCapabilities {
    ProxyCapabilities {
        pid: std::process::id(),
        lan,
        https,
        http2,
    }
}

#[test]
fn reuse_rejects_both_lan_mismatch_directions_without_mutation() {
    for (actual_lan, requested_lan) in [(false, true), (true, false)] {
        let proxy = FakeProxy::start(
            Some(caps(actual_lan, false, false)),
            false,
            GenerationChange::None,
        );
        let alias = Route {
            hostname: "alias.example.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: 1,
        };
        proxy.store.add_alias_route(alias).unwrap();
        let before = proxy.store.snapshot_dev_state().unwrap();
        let error = ensure_proxy_running_interruptible(
            &proxy.store,
            &proxy.settings(requested_lan, false, true),
            Path::new("unused-example-proxy-executable"),
            &|| false,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains(&format!("LAN={actual_lan}")), "{error}");
        assert!(error.contains(&format!("LAN={requested_lan}")), "{error}");
        assert!(error.contains("No proxy was restarted"), "{error}");
        assert!(error.contains(&proxy.store.root().display().to_string()));
        let after = proxy.store.snapshot_dev_state().unwrap();
        assert_eq!(after.sessions, before.sessions);
        assert_eq!(after.routes, before.routes);
        proxy.finish();
    }
}

#[test]
fn reuse_rejects_both_https_http2_mismatch_directions() {
    for (actual_http2, requested_http2) in [(false, true), (true, false)] {
        let proxy = FakeProxy::start(
            Some(caps(false, true, actual_http2)),
            true,
            GenerationChange::None,
        );
        let error = ensure_proxy_running_interruptible(
            &proxy.store,
            &proxy.settings(false, true, requested_http2),
            Path::new("unused-example-proxy-executable"),
            &|| false,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains(&format!("HTTP2={actual_http2}")), "{error}");
        assert!(
            error.contains(&format!("HTTP2={requested_http2}")),
            "{error}"
        );
        proxy.finish();
    }
}

#[test]
fn runtime_https_absence_overrides_stale_port_file_and_unrelated_listener() {
    let proxy = FakeProxy::start(
        Some(caps(false, false, false)),
        true,
        GenerationChange::None,
    );
    let error = ensure_proxy_running_interruptible(
        &proxy.store,
        &proxy.settings(false, true, false),
        Path::new("unused-example-proxy-executable"),
        &|| false,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("HTTPS=false"), "{error}");
    assert!(error.contains("HTTPS=true"), "{error}");
    proxy.finish();
}

#[test]
fn matching_reuse_and_http_only_request_accept_extra_https_listener() {
    for (requested_https, requested_http2) in [(true, true), (false, false)] {
        let proxy = FakeProxy::start(Some(caps(false, true, true)), true, GenerationChange::None);
        let result = ensure_proxy_running_interruptible(
            &proxy.store,
            &proxy.settings(false, requested_https, requested_http2),
            Path::new("unused-example-proxy-executable"),
            &|| false,
        )
        .unwrap();
        assert_eq!(result, LockOutcome::Acquired(()));
        proxy.finish();
    }
}

#[test]
fn compatible_running_proxy_can_add_app_certificate_hosts() {
    let proxy = FakeProxy::start(Some(caps(false, true, true)), true, GenerationChange::None);
    super::super::prepare_certs_for_hosts_interruptible(
        &proxy.settings(false, true, true),
        &["web.example.localhost".into()],
        &|| None,
    )
    .unwrap();
    let hosts = fs::read_to_string(proxy.store.leaf_hosts_path()).unwrap();
    assert!(hosts.contains("web.example.localhost"));
    proxy.finish();
}

#[test]
fn older_proxy_without_capabilities_requires_explicit_upgrade() {
    let proxy = FakeProxy::start(None, false, GenerationChange::None);
    let error = ensure_proxy_running_interruptible(
        &proxy.store,
        &proxy.settings(false, false, true),
        Path::new("unused-example-proxy-executable"),
        &|| false,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("cannot report authenticated"));
    assert!(error.contains("proxy stop --state-dir PATH"));
    assert!(error.contains(&proxy.store.root().display().to_string()));
    proxy.finish();
}

#[test]
fn capability_generation_mismatch_fails_closed() {
    let wrong_pid = ProxyCapabilities {
        pid: u32::MAX,
        ..caps(false, false, false)
    };
    for (reported, change, expected) in [
        (wrong_pid, GenerationChange::None, "generation changed"),
        (
            caps(false, false, false),
            GenerationChange::Token,
            "runtime generation changed",
        ),
        (
            caps(false, false, false),
            GenerationChange::Pid,
            "runtime generation changed",
        ),
    ] {
        let proxy = FakeProxy::start(Some(reported), false, change);
        let error = proxy_ready(&proxy.store, &proxy.settings(false, false, true))
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
        proxy.finish();
    }
}

#[test]
fn transient_capability_evidence_uses_monitor_health_miss_budget() {
    for reply in [CapabilityReply::Unavailable, CapabilityReply::Invalid] {
        let proxy = FakeProxy::start_with_reply(reply, false, GenerationChange::None);
        let result = proxy_ready_for_monitor_interruptible(
            &proxy.store,
            &proxy.settings(false, false, true),
            &|| false,
        )
        .unwrap();
        assert_eq!(result, LockOutcome::Acquired(false));
        proxy.finish();
    }
    for change in [GenerationChange::Token, GenerationChange::Pid] {
        let proxy = FakeProxy::start(Some(caps(false, false, false)), false, change);
        let result = proxy_ready_for_monitor_interruptible(
            &proxy.store,
            &proxy.settings(false, false, true),
            &|| false,
        )
        .unwrap();
        assert_eq!(result, LockOutcome::Acquired(false));
        proxy.finish();
    }
}

#[test]
fn transient_probe_does_not_recommend_restarting_shared_proxy() {
    let proxy =
        FakeProxy::start_with_reply(CapabilityReply::Unavailable, false, GenerationChange::None);
    let error = ensure_proxy_running_interruptible(
        &proxy.store,
        &proxy.settings(false, false, true),
        Path::new("unused-example-proxy-executable"),
        &|| false,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("temporarily unavailable"), "{error}");
    assert!(!error.contains("proxy stop"), "{error}");
    proxy.finish();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn incompatible_https_reuse_preserves_shared_certificate_material() {
    let proxy = FakeProxy::start(Some(caps(false, true, true)), true, GenerationChange::None);
    let spec = AppRunSpec::new(
        "web",
        proxy._temp.path().to_path_buf(),
        CommandSpec::Argv(vec!["true".into()]),
        "web.example.localhost",
    )
    .with_proxy(true);
    let settings = proxy.settings(true, true, true);
    let error = super::super::run_app_with_interrupt_probe(
        spec,
        &settings,
        Path::new("unused-example-proxy-executable"),
        || None,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("LAN=false"), "{error}");
    assert!(!proxy.store.ca_path().exists());
    assert!(!proxy.store.leaf_path().exists());
    proxy.finish();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn incompatible_reuse_returns_before_app_spawn_or_route_publication() {
    let proxy = FakeProxy::start(
        Some(caps(false, false, false)),
        false,
        GenerationChange::None,
    );
    let marker = proxy._temp.path().join("example-app-spawn-marker");
    let spec = AppRunSpec::new(
        "web",
        proxy._temp.path().to_path_buf(),
        CommandSpec::Argv(vec!["touch".into(), marker.display().to_string()]),
        "web.example.localhost",
    )
    .with_proxy(true);
    let before = proxy.store.snapshot_dev_state().unwrap();
    let error = super::super::run_app_with_interrupt_probe(
        spec,
        &proxy.settings(true, false, true),
        Path::new("unused-example-proxy-executable"),
        || None,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("LAN=false"), "{error}");
    assert!(!marker.exists());
    let after = proxy.store.snapshot_dev_state().unwrap();
    assert_eq!(after.sessions, before.sessions);
    assert_eq!(after.routes, before.routes);
    proxy.finish();
}
