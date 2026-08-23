use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

use crate::test_tempdir as tempdir;

use super::*;

mod dev_lifecycle;

const fn no_service_snapshot() -> Option<service::ServiceStatusSnapshot> {
    None
}

#[test]
fn interrupted_proxy_run_result_is_structured_after_cleanup() {
    let reason = processes::TerminationReason::from_signal({
        #[cfg(unix)]
        {
            libc::SIGTERM
        }
        #[cfg(not(unix))]
        {
            2
        }
    });
    let output = normalize_proxy_run_result(
        Err(processes::interruption_error(reason)),
        "web",
        "web.demo.localhost",
    )
    .unwrap();

    assert_eq!(
        output,
        json!({
            "ok": false,
            "interrupted": true,
            "exit_status": reason.exit_status(),
            "exit_signal": reason.signal(),
            "termination_signal": reason.label(),
            "app": "web",
            "hostname": "web.demo.localhost",
            "port": null,
        })
    );
}

#[test]
fn proxy_run_result_normalization_preserves_success_and_ordinary_errors() {
    let success = json!({ "ok": true, "app": "web", "exit_status": 0 });
    assert_eq!(
        normalize_proxy_run_result(Ok(success.clone()), "web", "web.demo.localhost").unwrap(),
        success
    );

    let error = normalize_proxy_run_result(
        Err(anyhow::anyhow!("ordinary failure")),
        "web",
        "web.demo.localhost",
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "ordinary failure");
}

#[test]
fn interrupted_proxy_start_result_is_structured_after_cleanup() {
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
    let output = normalize_proxy_start_result(Err(processes::interruption_error(reason)))
        .unwrap()
        .expect("interruption produces a structured result");

    assert_eq!(
        output,
        json!({
            "ok": false,
            "interrupted": true,
            "exit_status": reason.exit_status(),
            "exit_signal": reason.signal(),
            "termination_signal": reason.label(),
            "foreground": false,
            "http_port": null,
            "https_port": null,
        })
    );
}

#[test]
fn proxy_start_result_normalization_preserves_success_and_ordinary_errors() {
    assert_eq!(normalize_proxy_start_result(Ok(())).unwrap(), None);

    let error = normalize_proxy_start_result(Err(anyhow::anyhow!("ordinary failure"))).unwrap_err();
    assert_eq!(error.to_string(), "ordinary failure");
}

fn service_snapshot(
    value: Value,
    restart_risk: service::ServiceRestartRisk,
) -> service::ServiceStatusSnapshot {
    service::ServiceStatusSnapshot::from_parts(value, restart_risk)
}

fn keepalive_service_snapshot() -> service::ServiceStatusSnapshot {
    service_snapshot(
        json!({
            "ok": true,
            "may_restart_proxy": true,
        }),
        service::ServiceRestartRisk::MayRestart,
    )
}

fn inactive_service_snapshot(value: Value) -> service::ServiceStatusSnapshot {
    service_snapshot(value, service::ServiceRestartRisk::None)
}

#[test]
fn proxy_runtime_status_reports_missing_runtime_state() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

    let status = proxy_runtime_status(&store).unwrap();

    assert_eq!(status.pid, None);
    assert!(!status.pid_is_alive());
    assert!(!status.pid_may_be_alive());
    assert_eq!(status.pid_observation, None);
    assert_eq!(status.http_port, None);
    assert_eq!(status.health_pid, None);
    assert!(!status.handshake_ok);
    assert!(!status.pid_matches_proxy);
}

#[test]
fn uncertain_proxy_pid_is_not_reported_alive_but_remains_conservative() {
    let status = ProxyRuntimeStatus {
        pid: Some(123),
        pid_observation: Some(PidObservation::Uncertain),
        http_port: None,
        health_pid: None,
        handshake_ok: false,
        pid_matches_proxy: false,
    };

    assert!(!status.pid_is_alive());
    assert!(status.pid_may_be_alive());
    let warning = stop_warning(StopWarningStatus {
        pid: status.pid,
        health_pid: status.health_pid,
        pid_observation: status.pid_observation,
        handshake_ok: status.handshake_ok,
        pid_matches_proxy: status.pid_matches_proxy,
        stopped: false,
        service_keepalive_active: false,
        service_status_uncertain: false,
    })
    .unwrap();
    assert!(warning.contains("could not classify whether that PID is still alive"));
}

#[cfg(unix)]
#[test]
fn signal_esrch_is_treated_as_already_stopped() {
    assert_eq!(
        classify_signal_error(Some(libc::ESRCH)),
        SignalResult::NotFound
    );
}

#[test]
fn proxy_stop_keeps_runtime_files_for_live_unverified_pid() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    store.write_pid(std::process::id()).unwrap();
    store.write_http_port(closed_loopback_port()).unwrap();

    let output = proxy_stop_with_service_probe(ProxyStopRequest { settings }, |_, _| {
        Ok(no_service_snapshot())
    })
    .unwrap();

    assert_eq!(output["ok"].as_bool(), Some(false));
    assert_eq!(output["stopped"].as_bool(), Some(false));
    assert_eq!(output["runtime_files_cleared"].as_bool(), Some(false));
    assert!(
        output["warning"]
            .as_str()
            .unwrap()
            .contains("did not answer")
    );
    assert!(store.pid_path().exists());
    assert!(store.http_port_path().exists());
}

fn closed_loopback_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.local_addr().unwrap().port()
}

fn spawn_fake_health_responder(health_pid: u32) -> (u16, thread::JoinHandle<bool>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // Accepted sockets can inherit the listener's nonblocking
                    // mode on some platforms. The health client writes after
                    // connect, so make this fixture's request read blocking
                    // instead of racing it and intermittently seeing EAGAIN.
                    stream.set_nonblocking(false).unwrap();
                    let mut request = [0u8; 512];
                    let _ = stream.read(&mut request).unwrap();
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nx-jig-proxy: 1\r\nx-jig-proxy-pid: {health_pid}\r\ncontent-length: 11\r\n\r\n{{\"ok\":true}}",
                    )
                    .unwrap();
                    return true;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return false;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("fake health responder accept failed: {error}"),
            }
        }
    });
    (port, handle)
}

#[test]
fn proxy_stop_does_not_kill_when_health_pid_differs() {
    let temp = tempdir().unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 512];
        let _ = stream.read(&mut request).unwrap();
        stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nx-jig-proxy: 1\r\nx-jig-proxy-pid: 1\r\ncontent-length: 11\r\n\r\n{\"ok\":true}",
                )
                .unwrap();
    });

    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    store.write_pid(std::process::id()).unwrap();
    store.write_http_port(port).unwrap();
    store.ensure_health_token().unwrap();

    let output = proxy_stop_with_service_probe(ProxyStopRequest { settings }, |_, _| {
        Ok(no_service_snapshot())
    })
    .unwrap();
    handle.join().unwrap();

    assert_eq!(output["ok"].as_bool(), Some(false));
    assert_eq!(output["stopped"].as_bool(), Some(false));
    assert_eq!(output["handshake_ok"].as_bool(), Some(true));
    assert_eq!(output["pid_matches_proxy"].as_bool(), Some(false));
    assert_eq!(output["runtime_files_cleared"].as_bool(), Some(false));
    assert!(
        output["warning"]
            .as_str()
            .unwrap()
            .contains("PID file points")
    );
    assert!(store.pid_path().exists());
    assert!(store.http_port_path().exists());
}

#[test]
fn proxy_stop_keeps_runtime_files_when_stale_pid_but_health_pid_cannot_stop() {
    let temp = tempdir().unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let health_pid = 4242_u32;
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 512];
        let _ = stream.read(&mut request).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nx-jig-proxy: 1\r\nx-jig-proxy-pid: {health_pid}\r\ncontent-length: 11\r\n\r\n{{\"ok\":true}}",
        )
        .unwrap();
    });

    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    store.write_pid(u32::MAX).unwrap();
    store.write_http_port(port).unwrap();
    store.ensure_health_token().unwrap();
    let attempted_stop = std::cell::Cell::new(None);

    let output = proxy_stop_with_service_probe_and_terminator(
        ProxyStopRequest { settings },
        |_, _| Ok(no_service_snapshot()),
        |pid| {
            attempted_stop.set(Some(pid));
            false
        },
    )
    .unwrap();
    handle.join().unwrap();

    assert_eq!(attempted_stop.get(), None);
    assert_eq!(output["ok"].as_bool(), Some(false));
    assert_eq!(output["stopped"].as_bool(), Some(false));
    assert_eq!(output["health_pid"].as_u64(), Some(u64::from(health_pid)));
    assert_eq!(output["pid_alive"].as_bool(), Some(false));
    assert_eq!(output["runtime_files_cleared"].as_bool(), Some(false));
    assert!(
        output["warning"]
            .as_str()
            .unwrap()
            .contains("authenticated proxy could not be stopped")
    );
    assert!(store.pid_path().exists());
    assert!(store.http_port_path().exists());
}

#[test]
fn proxy_stop_keeps_runtime_files_when_pid_file_missing_but_health_pid_answers() {
    let temp = tempdir().unwrap();
    let health_pid = 4242_u32;
    let (port, handle) = spawn_fake_health_responder(health_pid);

    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    store.write_http_port(port).unwrap();
    store.ensure_health_token().unwrap();
    let attempted_stop = std::cell::Cell::new(None);

    let output = proxy_stop_with_service_probe_and_terminator(
        ProxyStopRequest { settings },
        |_, _| Ok(no_service_snapshot()),
        |pid| {
            attempted_stop.set(Some(pid));
            true
        },
    )
    .unwrap();
    let responder_was_contacted = handle.join().unwrap();

    assert!(responder_was_contacted);
    assert_eq!(attempted_stop.get(), None);
    assert_eq!(output["ok"].as_bool(), Some(false));
    assert_eq!(output["stopped"].as_bool(), Some(false));
    assert_eq!(output["health_pid"].as_u64(), Some(u64::from(health_pid)));
    assert!(!output["pid_alive"].as_bool().unwrap());
    assert_eq!(output["runtime_files_cleared"].as_bool(), Some(false));
    assert!(
        output["warning"]
            .as_str()
            .unwrap()
            .contains("authenticated proxy could not be stopped")
    );
    assert!(!store.pid_path().exists());
    assert!(store.http_port_path().exists());
}

#[test]
fn proxy_stop_ignores_health_pid_without_health_token() {
    let temp = tempdir().unwrap();
    let fake_pid = 4242_u32;
    let (port, handle) = spawn_fake_health_responder(fake_pid);

    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    store.write_pid(u32::MAX).unwrap();
    store.write_http_port(port).unwrap();
    let attempted_stop = std::cell::Cell::new(None);

    let output = proxy_stop_with_service_probe_and_terminator(
        ProxyStopRequest { settings },
        |_, _| Ok(no_service_snapshot()),
        |pid| {
            attempted_stop.set(Some(pid));
            true
        },
    )
    .unwrap();
    let responder_was_contacted = handle.join().unwrap();

    assert_eq!(attempted_stop.get(), None);
    assert!(!responder_was_contacted);
    assert_eq!(output["ok"].as_bool(), Some(true));
    assert_eq!(output["stopped"].as_bool(), Some(false));
    assert!(output["health_pid"].is_null());
    assert_eq!(output["handshake_ok"].as_bool(), Some(false));
    assert_eq!(output["runtime_files_cleared"].as_bool(), Some(true));
    assert!(!store.pid_path().exists());
    assert!(!store.http_port_path().exists());
}

#[test]
fn proxy_stop_warns_when_service_degrades_ok_without_pid() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    StateStore::resolve(settings.state_dir.clone()).unwrap();

    let output = proxy_stop_with_service_probe(ProxyStopRequest { settings }, |_, _| {
        Ok(Some(keepalive_service_snapshot()))
    })
    .unwrap();

    assert_eq!(output["ok"].as_bool(), Some(false));
    assert_eq!(output["service_keepalive_active"].as_bool(), Some(true));
    assert!(
        output["warning"]
            .as_str()
            .unwrap()
            .contains("No Jig proxy PID file")
    );
}

#[test]
fn proxy_stop_keeps_runtime_files_when_service_keepalive_active() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    store.write_pid(u32::MAX).unwrap();
    store.write_http_port(12345).unwrap();

    let output = proxy_stop_with_service_probe(ProxyStopRequest { settings }, |_, _| {
        Ok(Some(keepalive_service_snapshot()))
    })
    .unwrap();

    assert_eq!(output["ok"].as_bool(), Some(false));
    assert_eq!(output["stopped"].as_bool(), Some(false));
    assert_eq!(output["runtime_files_cleared"].as_bool(), Some(false));
    assert_eq!(output["service_keepalive_active"].as_bool(), Some(true));
    assert!(store.pid_path().exists());
    assert!(store.http_port_path().exists());
}

#[test]
fn proxy_stop_keeps_runtime_files_when_service_status_uncertain() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    store.write_pid(u32::MAX).unwrap();
    store.write_http_port(12345).unwrap();

    let output = proxy_stop_with_service_probe(ProxyStopRequest { settings }, |_, _| {
        Ok(Some(service::ServiceStatusSnapshot::uncertain(
            anyhow::anyhow!("service status timed out"),
        )))
    })
    .unwrap();

    assert_eq!(output["ok"].as_bool(), Some(false));
    assert_eq!(output["stopped"].as_bool(), Some(false));
    assert_eq!(output["runtime_files_cleared"].as_bool(), Some(false));
    assert_eq!(output["service_status_uncertain"].as_bool(), Some(true));
    assert!(store.pid_path().exists());
    assert!(store.http_port_path().exists());
}

#[test]
fn stop_warning_reports_service_keepalive_after_stopped_pid() {
    let warning = stop_warning(StopWarningStatus {
        pid: Some(123),
        health_pid: Some(123),
        pid_observation: Some(PidObservation::Alive),
        handshake_ok: true,
        pid_matches_proxy: true,
        stopped: true,
        service_keepalive_active: true,
        service_status_uncertain: false,
    })
    .unwrap();

    assert!(warning.contains("may restart"));
    assert!(warning.contains("proxy service uninstall"));
}

#[test]
fn stop_warning_reports_uncertain_service_status_after_stopped_pid() {
    let warning = stop_warning(StopWarningStatus {
        pid: Some(123),
        health_pid: Some(123),
        pid_observation: Some(PidObservation::Alive),
        handshake_ok: true,
        pid_matches_proxy: true,
        stopped: true,
        service_keepalive_active: false,
        service_status_uncertain: true,
    })
    .unwrap();

    assert!(warning.contains("could not inspect"));
    assert!(warning.contains("proxy service status"));
}

#[test]
fn stop_warning_reports_service_keepalive_without_pid() {
    let warning = stop_warning(StopWarningStatus {
        pid: None,
        health_pid: None,
        pid_observation: None,
        handshake_ok: false,
        pid_matches_proxy: false,
        stopped: false,
        service_keepalive_active: true,
        service_status_uncertain: false,
    })
    .unwrap();

    assert!(warning.contains("No Jig proxy PID file"));
    assert!(warning.contains("proxy service uninstall"));
}

#[test]
fn stop_warning_reports_uncertain_service_status_without_pid() {
    let warning = stop_warning(StopWarningStatus {
        pid: None,
        health_pid: None,
        pid_observation: None,
        handshake_ok: false,
        pid_matches_proxy: false,
        stopped: false,
        service_keepalive_active: false,
        service_status_uncertain: true,
    })
    .unwrap();

    assert!(warning.contains("No Jig proxy PID file"));
    assert!(warning.contains("proxy service status"));
}

#[test]
fn service_keepalive_status_degrades_stop_ok() {
    let snapshot = keepalive_service_snapshot();

    assert!(snapshot.keepalive_active());
    assert!(snapshot.degrades_stop_ok());
}

#[test]
fn uncertain_service_status_degrades_stop_ok() {
    let snapshot = service::ServiceStatusSnapshot::uncertain(anyhow::anyhow!("launchctl failed"));

    assert!(snapshot.is_uncertain());
    assert!(snapshot.degrades_stop_ok());
}

#[test]
fn typed_uncertain_service_status_degrades_stop_ok() {
    let snapshot = service_snapshot(
        json!({
            "ok": false,
            "service_state_dir_error": "could not inspect service file"
        }),
        service::ServiceRestartRisk::Uncertain,
    );

    assert!(snapshot.is_uncertain());
    assert!(snapshot.degrades_stop_ok());
}

#[test]
fn proxy_alias_rejects_zero_port() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };

    let error = proxy_alias(ProxyAliasRequest {
        name: "api".into(),
        target_host: "127.0.0.1".into(),
        target_port: 0,
        repo_name: "demo".into(),
        accept_non_loopback_target: false,
        settings,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("must be greater than 0"));
}

#[test]
fn proxy_alias_rejects_invalid_target_host() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };

    let error = proxy_alias(ProxyAliasRequest {
        name: "api".into(),
        target_host: "bad host".into(),
        target_port: 5000,
        repo_name: "demo".into(),
        accept_non_loopback_target: false,
        settings,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("must be an IP literal"));
}

#[test]
fn proxy_alias_rejects_hostname_target_host() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };

    let error = proxy_alias(ProxyAliasRequest {
        name: "api".into(),
        target_host: "example.com".into(),
        target_port: 5000,
        repo_name: "demo".into(),
        accept_non_loopback_target: false,
        settings,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("must be an IP literal"));
}

#[test]
fn proxy_alias_lan_rejects_non_loopback_target_host() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        lan: true,
        ..ProxySettings::default()
    };

    let error = proxy_alias(ProxyAliasRequest {
        name: "api".into(),
        target_host: "10.0.0.5".into(),
        target_port: 5000,
        repo_name: "demo".into(),
        accept_non_loopback_target: false,
        settings,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("loopback"));
}

#[test]
fn proxy_alias_requires_ack_for_non_loopback_target_host() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };

    let error = proxy_alias(ProxyAliasRequest {
        name: "api".into(),
        target_host: "10.0.0.5".into(),
        target_port: 5000,
        repo_name: "demo".into(),
        accept_non_loopback_target: false,
        settings,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("--accept-non-loopback-target"));
}

#[test]
fn proxy_alias_allows_acknowledged_non_loopback_target_host() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };

    let output = proxy_alias(ProxyAliasRequest {
        name: "api".into(),
        target_host: "10.0.0.5".into(),
        target_port: 5000,
        repo_name: "demo".into(),
        accept_non_loopback_target: true,
        settings,
    })
    .unwrap();

    assert_eq!(output["ok"].as_bool(), Some(true));
}

#[test]
fn proxy_alias_rejects_live_process_route_replacement() {
    if !crate::state::process_start_tokens_supported() {
        return;
    }
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    store
        .add_route(Route {
            hostname: "api.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: Some(std::process::id()),
            owner_start_token: crate::state::process_start_token(std::process::id()),
            mode: RouteMode::Process,
            created_at_ms: now_ms(),
        })
        .unwrap();

    let error = proxy_alias(ProxyAliasRequest {
        name: "api".into(),
        target_host: "127.0.0.1".into(),
        target_port: 5000,
        repo_name: "demo".into(),
        accept_non_loopback_target: false,
        settings,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("would replace a live process route"));
}

#[cfg(unix)]
#[test]
fn proxy_alias_registers_route_and_refreshes_https_certificate() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        https: true,
        ..ProxySettings::default()
    };
    let https_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let health_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let token = store.ensure_health_token().unwrap();
    store.write_pid(std::process::id()).unwrap();
    store
        .write_http_port(health_listener.local_addr().unwrap().port())
        .unwrap();
    store
        .write_https_port(https_listener.local_addr().unwrap().port())
        .unwrap();
    let health = thread::spawn(move || {
        let (mut stream, _) = health_listener.accept().unwrap();
        let mut request = [0u8; 512];
        let count = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..count]);
        assert!(request.contains(&format!("x-jig-proxy-health-token: {token}\r\n")));
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nx-jig-proxy: 1\r\nx-jig-proxy-pid: {}\r\ncontent-length: 0\r\n\r\n",
            std::process::id()
        )
        .unwrap();
    });

    let output = proxy_alias(ProxyAliasRequest {
        name: "api".into(),
        target_host: "127.0.0.1".into(),
        target_port: 5000,
        repo_name: "demo".into(),
        accept_non_loopback_target: false,
        settings,
    })
    .unwrap();
    health.join().unwrap();

    assert_eq!(output["ok"].as_bool(), Some(true));
    assert_eq!(output["hostname"].as_str(), Some("api.demo.localhost"));
    let routes = store.read_routes(false).unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].hostname, "api.demo.localhost");
    assert_eq!(routes[0].target_port, 5000);
    assert_eq!(routes[0].mode, RouteMode::Alias);
    assert!(store.leaf_path().exists());
    let leaf_hosts = std::fs::read_to_string(store.leaf_hosts_path()).unwrap();
    assert!(leaf_hosts.contains("api.demo.localhost"));
}

#[cfg(unix)]
#[test]
fn proxy_alias_defers_https_certificate_without_running_https_proxy() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        https: true,
        ..ProxySettings::default()
    };

    proxy_alias(ProxyAliasRequest {
        name: "api".into(),
        target_host: "127.0.0.1".into(),
        target_port: 5000,
        repo_name: "demo".into(),
        accept_non_loopback_target: false,
        settings: settings.clone(),
    })
    .unwrap();

    let store = StateStore::resolve(settings.state_dir).unwrap();
    assert!(!store.leaf_path().exists());
}

#[test]
fn proxy_stop_list_and_prune_noop_when_state_dir_is_missing() {
    let temp = tempdir().unwrap();
    let missing = temp.path().join("missing-state");
    let settings = ProxySettings {
        state_dir: Some(missing.clone()),
        ..ProxySettings::default()
    };

    let stop = proxy_stop_with_service_probe(
        ProxyStopRequest {
            settings: settings.clone(),
        },
        |_, _| Ok(no_service_snapshot()),
    )
    .unwrap();
    let list = proxy_list_with_service_probe(
        ProxyListRequest {
            settings: settings.clone(),
            raw: false,
        },
        |_, _| Ok(no_service_snapshot()),
    )
    .unwrap();
    let prune = proxy_prune(ProxyPruneRequest { settings }).unwrap();

    assert_eq!(stop["ok"].as_bool(), Some(true));
    assert_eq!(stop["stopped"].as_bool(), Some(false));
    assert!(list["routes"].as_array().unwrap().is_empty());
    assert!(prune["routes"].as_array().unwrap().is_empty());
    assert!(!missing.exists());
}

#[test]
fn proxy_stop_list_report_service_when_state_dir_is_missing() {
    let temp = tempdir().unwrap();
    let missing = temp.path().join("missing-state");
    let settings = ProxySettings {
        state_dir: Some(missing.clone()),
        ..ProxySettings::default()
    };

    let stop = proxy_stop_with_service_probe(
        ProxyStopRequest {
            settings: settings.clone(),
        },
        |_, state_dir| {
            assert_eq!(state_dir, missing.as_path());
            Ok(Some(keepalive_service_snapshot()))
        },
    )
    .unwrap();
    let list = proxy_list_with_service_probe(
        ProxyListRequest {
            settings,
            raw: false,
        },
        |_, state_dir| {
            assert_eq!(state_dir, missing.as_path());
            Ok(Some(keepalive_service_snapshot()))
        },
    )
    .unwrap();

    assert_eq!(stop["ok"].as_bool(), Some(false));
    assert_eq!(stop["service_keepalive_active"].as_bool(), Some(true));
    assert!(stop["warning"].as_str().unwrap().contains("state dir"));
    assert_eq!(list["service_keepalive_active"].as_bool(), Some(true));
    assert!(!missing.exists());
}

#[test]
fn proxy_stop_does_not_fail_for_service_bound_to_different_state_dir() {
    let temp = tempdir().unwrap();
    let other_state_dir = temp.path().join("other-state");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("current-state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();

    let stop = proxy_stop_with_service_probe(ProxyStopRequest { settings }, |_, state_dir| {
        assert_eq!(state_dir, store.root());
        Ok(Some(inactive_service_snapshot(json!({
            "ok": true,
            "installed": false,
            "may_restart_proxy": false,
            "service_state_dir": other_state_dir,
            "service_state_dir_matches": false,
        }))))
    })
    .unwrap();

    assert_eq!(stop["ok"].as_bool(), Some(true));
    assert_eq!(stop["service_keepalive_active"].as_bool(), Some(false));
    assert_eq!(stop["service_status_uncertain"].as_bool(), Some(false));
    assert_eq!(stop["service"]["installed"].as_bool(), Some(false));
    assert_eq!(stop["warning"], Value::Null);
}

#[test]
fn proxy_stop_reports_uncertain_service_when_state_dir_is_missing() {
    let temp = tempdir().unwrap();
    let missing = temp.path().join("missing-state");
    let settings = ProxySettings {
        state_dir: Some(missing.clone()),
        ..ProxySettings::default()
    };

    let stop = proxy_stop_with_service_probe(ProxyStopRequest { settings }, |_, _| {
        Ok(Some(service::ServiceStatusSnapshot::uncertain(
            anyhow::anyhow!("launchctl failed"),
        )))
    })
    .unwrap();

    assert_eq!(stop["ok"].as_bool(), Some(false));
    assert_eq!(stop["service_status_uncertain"].as_bool(), Some(true));
    assert!(
        stop["warning"]
            .as_str()
            .unwrap()
            .contains("could not inspect")
    );
    assert!(!missing.exists());
}

#[cfg(unix)]
#[test]
fn missing_state_dir_propagates_stat_errors() {
    let temp = tempdir().unwrap();
    let loop_path = temp.path().join("loop-state");
    std::os::unix::fs::symlink(&loop_path, &loop_path).unwrap();
    let settings = ProxySettings {
        state_dir: Some(loop_path.clone()),
        ..ProxySettings::default()
    };

    let error = missing_state_dir(&settings).unwrap_err().to_string();

    assert!(error.contains("Failed to inspect Jig proxy state dir"));
    assert!(error.contains(&loop_path.display().to_string()));
}

#[test]
fn proxy_stop_list_report_uncertain_service_when_probe_errors() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    StateStore::resolve(settings.state_dir.clone()).unwrap();

    let stop = proxy_stop_with_service_probe(
        ProxyStopRequest {
            settings: settings.clone(),
        },
        |_, _| Err(anyhow::anyhow!("service file stat failed")),
    )
    .unwrap();
    let list = proxy_list_with_service_probe(
        ProxyListRequest {
            settings,
            raw: false,
        },
        |_, _| Err(anyhow::anyhow!("service file stat failed")),
    )
    .unwrap();

    assert_eq!(stop["ok"].as_bool(), Some(false));
    assert_eq!(stop["runtime_files_cleared"].as_bool(), Some(false));
    assert_eq!(stop["service_status_uncertain"].as_bool(), Some(true));
    assert_eq!(stop["service"]["ok"].as_bool(), Some(false));
    assert!(
        stop["service"]["error"]
            .as_str()
            .unwrap()
            .contains("service file stat failed")
    );
    assert_eq!(list["service_status_uncertain"].as_bool(), Some(true));
    assert_eq!(list["service"]["ok"].as_bool(), Some(false));
}

#[test]
fn dev_reports_unknown_selected_app_names() {
    let temp = tempdir().unwrap();
    let error = dev(DevRequest {
        repo_name: "demo".into(),
        root: temp.path().to_path_buf(),
        package_manager: "npm".into(),
        settings: ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        },
        apps: vec![AppRunSpec {
            name: "web".into(),
            dir: temp.path().to_path_buf(),
            command: CommandSpec::Argv(vec!["unused".into()]),
            kind: AppKind::EnvPort,
            hostname: "web.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            explicit_port: None,
            proxy: false,
        }],
        selected_apps: vec!["api".into()],
        discover_workspace: false,
        no_proxy: false,
        replace: false,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("No development apps matched"));
    assert!(error.contains("Available apps: web"));
}

#[test]
fn dev_rejects_unknown_selected_app_names_even_when_another_filter_matches() {
    let temp = tempdir().unwrap();
    let error = dev(DevRequest {
        repo_name: "demo".into(),
        root: temp.path().to_path_buf(),
        package_manager: "npm".into(),
        settings: ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        },
        apps: vec![AppRunSpec {
            name: "web".into(),
            dir: temp.path().to_path_buf(),
            command: CommandSpec::Argv(vec!["unused".into()]),
            kind: AppKind::EnvPort,
            hostname: "web.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            explicit_port: None,
            proxy: false,
        }],
        selected_apps: vec!["web".into(), "api".into()],
        discover_workspace: false,
        no_proxy: false,
        replace: false,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("No development apps matched --app filter 'api'"));
    assert!(error.contains("Available apps: web"));
}

#[test]
fn dev_reports_empty_app_configuration_before_launch() {
    let temp = tempdir().unwrap();
    let error = dev(DevRequest {
        repo_name: "demo".into(),
        root: temp.path().to_path_buf(),
        package_manager: "npm".into(),
        settings: ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        },
        apps: Vec::new(),
        selected_apps: Vec::new(),
        discover_workspace: false,
        no_proxy: false,
        replace: false,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("No development apps were configured or discovered"));
}

#[test]
fn duplicate_hostname_error_includes_source_dirs() {
    let temp = tempdir().unwrap();
    let web_dir = temp.path().join("web");
    let api_dir = temp.path().join("api");
    let specs = vec![
        AppRunSpec {
            name: "web".into(),
            dir: web_dir.clone(),
            command: CommandSpec::Argv(vec!["unused".into()]),
            kind: AppKind::EnvPort,
            hostname: "app.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            explicit_port: None,
            proxy: true,
        },
        AppRunSpec {
            name: "api".into(),
            dir: api_dir.clone(),
            command: CommandSpec::Argv(vec!["unused".into()]),
            kind: AppKind::EnvPort,
            hostname: "app.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            explicit_port: None,
            proxy: true,
        },
    ];

    let error = ensure_unique_specs(&specs).unwrap_err().to_string();

    assert!(error.contains("Duplicate development app hostname"));
    assert!(error.contains(&web_dir.display().to_string()));
    assert!(error.contains(&api_dir.display().to_string()));
}

#[test]
fn resolved_dev_request_keeps_the_canonical_directory_for_preflight() {
    let temp = tempdir().unwrap();
    let app_dir = temp.path().join("web");
    std::fs::create_dir(&app_dir).unwrap();
    let request = DevRequest::new(
        "demo",
        temp.path().to_path_buf(),
        "npm",
        ProxySettings::default(),
    )
    .with_apps(vec![AppRunSpec::new(
        "web",
        Path::new("web").to_path_buf(),
        CommandSpec::Argv(vec!["pnpm".into(), "dev".into()]),
        "web.demo.localhost",
    )]);

    let resolved = resolve_dev_request(request).unwrap();
    let resolved_dir = &resolved.apps()[0].dir;

    assert_eq!(resolved_dir, &std::fs::canonicalize(app_dir).unwrap());
}
