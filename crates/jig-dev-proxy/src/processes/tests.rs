use std::io::{Read, Write};
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::thread;

use super::*;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use tempfile::tempdir;

#[test]
fn injects_vite_port_and_host_flags() {
    let mut argv = vec!["vite".to_string()];
    inject_framework_flags(&mut argv, &AppKind::EnvPort, 4210);
    assert!(argv.contains(&"--port".to_string()));
    assert!(argv.contains(&"4210".to_string()));
    assert!(argv.contains(&"--host".to_string()));
    assert!(argv.contains(&"--strictPort".to_string()));
}

#[test]
fn ensure_not_interrupted_reports_pending_signal() {
    let error = ensure_not_interrupted_with(|| true)
        .unwrap_err()
        .to_string();

    assert_eq!(error, "Interrupted");
}

#[test]
fn does_not_duplicate_existing_flags() {
    let mut argv = vec![
        "vite".to_string(),
        "--port".to_string(),
        "3000".to_string(),
        "--host=0.0.0.0".to_string(),
    ];
    inject_framework_flags(&mut argv, &AppKind::Vite, 4210);
    assert_eq!(argv.iter().filter(|arg| *arg == "--port").count(), 1);
    assert!(!argv.contains(&"4210".to_string()));
    assert!(argv.contains(&"--strictPort".to_string()));
}

#[cfg(target_os = "linux")]
#[test]
fn listener_matching_accepts_unspecified_listener_for_same_family() {
    let target_addrs = ["127.0.0.1:4000".parse().unwrap()];

    assert!(listen_ip_matches_targets(
        "0.0.0.0".parse().unwrap(),
        &target_addrs
    ));
    assert!(!listen_ip_matches_targets(
        "::".parse().unwrap(),
        &target_addrs
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn linux_tcp_ip_parser_uses_proc_native_endian_words() {
    let ipv4_loopback = if cfg!(target_endian = "little") {
        "0100007F"
    } else {
        "7F000001"
    };
    let ipv6_loopback = if cfg!(target_endian = "little") {
        "00000000000000000000000001000000"
    } else {
        "00000000000000000000000000000001"
    };

    assert_eq!(
        parse_linux_tcp_ip(ipv4_loopback),
        Some("127.0.0.1".parse::<std::net::IpAddr>().unwrap())
    );
    assert_eq!(
        parse_linux_tcp_ip(ipv6_loopback),
        Some("::1".parse::<std::net::IpAddr>().unwrap())
    );
    assert_eq!(parse_linux_tcp_ip("not-hex"), None);
}

#[cfg(target_os = "linux")]
#[test]
fn terminate_child_kills_process_group_grandchild() {
    let temp = tempdir().unwrap();
    let grandchild_pid_path = temp.path().join("grandchild.pid");
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg("sleep 60 & echo $! > \"$1\"; wait")
        .arg("sh")
        .arg(&grandchild_pid_path);
    configure_app_child_process_group(&mut command);
    let mut child = command.spawn().unwrap();
    let mut grandchild_pid = None;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if let Ok(text) = fs::read_to_string(&grandchild_pid_path) {
            if let Ok(pid) = text.trim().parse::<u32>() {
                grandchild_pid = Some(pid);
                break;
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    let Some(grandchild_pid) = grandchild_pid else {
        terminate_child(&mut child);
        let _ = child.wait();
        panic!("grandchild PID was not written");
    };

    terminate_child(&mut child);
    let _ = child.wait();

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !crate::state::pid_is_alive(grandchild_pid) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("grandchild process {grandchild_pid} survived process-group termination");
}

#[test]
fn does_not_duplicate_vite_port_shorthand() {
    let mut argv = vec!["vite".to_string(), "-p".to_string(), "3000".to_string()];
    inject_framework_flags(&mut argv, &AppKind::Vite, 4210);

    assert!(!argv.contains(&"--port".to_string()));
    assert!(!argv.contains(&"4210".to_string()));
    assert!(argv.contains(&"--strictPort".to_string()));
}

#[test]
fn does_not_duplicate_vite_compact_port_shorthand() {
    let mut argv = vec!["vite".to_string(), "-p3000".to_string()];
    inject_framework_flags(&mut argv, &AppKind::Vite, 4210);

    assert!(!argv.contains(&"--port".to_string()));
    assert!(!argv.contains(&"4210".to_string()));
    assert!(argv.contains(&"--strictPort".to_string()));
}

#[test]
fn vite_argv_rejects_port_flags_that_do_not_match_assigned_port() {
    let command = CommandSpec::Argv(vec![
        "vite".to_string(),
        "-p".to_string(),
        "3000".to_string(),
    ]);

    let error = command_argv(&command, &AppKind::Vite, 4210)
        .unwrap_err()
        .to_string();

    assert!(error.contains("already sets port 3000"));
    assert!(command_argv(&command, &AppKind::Vite, 3000).is_ok());
}

#[test]
fn vite_argv_rejects_compact_port_flags_that_do_not_match_assigned_port() {
    let command = CommandSpec::Argv(vec!["vite".to_string(), "-p3000".to_string()]);

    let error = command_argv(&command, &AppKind::Vite, 4210)
        .unwrap_err()
        .to_string();

    assert!(error.contains("already sets port 3000"));
    assert!(command_argv(&command, &AppKind::Vite, 3000).is_ok());
}

#[test]
fn vite_argv_rejects_port_flags_without_numeric_values() {
    let missing = CommandSpec::Argv(vec!["vite".to_string(), "--port".to_string()]);
    let non_numeric = CommandSpec::Argv(vec!["vite".to_string(), "--port=abc".to_string()]);

    assert!(
        command_argv(&missing, &AppKind::Vite, 4210)
            .unwrap_err()
            .to_string()
            .contains("must include")
    );
    assert!(
        command_argv(&non_numeric, &AppKind::Vite, 4210)
            .unwrap_err()
            .to_string()
            .contains("non-numeric")
    );
}

#[test]
fn inserts_separator_for_package_manager_vite_commands() {
    let mut argv = vec!["pnpm".to_string(), "run".to_string(), "dev".to_string()];
    inject_framework_flags(&mut argv, &AppKind::Vite, 4210);

    assert_eq!(
        argv,
        vec![
            "pnpm",
            "run",
            "dev",
            "--",
            "--port",
            "4210",
            "--strictPort",
            "--host",
            "127.0.0.1"
        ]
    );
}

#[test]
fn inserts_package_manager_separator_before_existing_script_args() {
    let mut argv = vec![
        "pnpm".to_string(),
        "run".to_string(),
        "dev".to_string(),
        "--mode".to_string(),
        "local".to_string(),
    ];
    inject_framework_flags(&mut argv, &AppKind::Vite, 4210);

    assert_eq!(&argv[..6], ["pnpm", "run", "dev", "--", "--mode", "local"]);
    assert!(argv.contains(&"--port".to_string()));
}

#[test]
fn inserts_package_manager_separator_for_exec_vite_commands() {
    let mut argv = vec![
        "pnpm".to_string(),
        "exec".to_string(),
        "vite".to_string(),
        "--base".to_string(),
        "/x".to_string(),
    ];
    inject_framework_flags(&mut argv, &AppKind::EnvPort, 4210);

    assert_eq!(&argv[..6], ["pnpm", "exec", "vite", "--", "--base", "/x"]);
    assert!(argv.contains(&"--port".to_string()));
}

#[test]
fn yarn_direct_commands_do_not_receive_run_separator() {
    let mut argv = vec![
        "yarn".to_string(),
        "vite".to_string(),
        "--mode".to_string(),
        "dev".to_string(),
    ];
    inject_framework_flags(&mut argv, &AppKind::Vite, 4210);

    assert!(!argv.contains(&"--".to_string()));
    assert!(argv.contains(&"--port".to_string()));
}

#[test]
fn vite_exec_wrappers_receive_flags_without_run_separator() {
    for command in [
        vec!["npx".to_string(), "vite".to_string()],
        vec!["bunx".to_string(), "vite".to_string()],
    ] {
        let mut argv = command.clone();
        inject_framework_flags(&mut argv, &AppKind::EnvPort, 4210);

        assert!(!argv.contains(&"--".to_string()));
        assert_eq!(&argv[..command.len()], &command[..]);
        assert!(argv.contains(&"--port".to_string()));
        assert!(argv.contains(&"4210".to_string()));
        assert!(argv.contains(&"--host".to_string()));
    }
}

#[test]
fn vite_detection_ignores_unrelated_arguments_named_vite() {
    let mut argv = vec![
        "node".to_string(),
        "scripts/build.js".to_string(),
        "--target".to_string(),
        "vite".to_string(),
    ];

    inject_framework_flags(&mut argv, &AppKind::EnvPort, 4210);

    assert!(!argv.contains(&"--port".to_string()));
    assert!(!argv.contains(&"--strictPort".to_string()));
}

#[test]
fn shell_vite_commands_are_rejected() {
    let error = command_argv(
        &CommandSpec::Shell("bun run dev".into()),
        &AppKind::Vite,
        4210,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("must use argv"));
}

#[test]
fn shell_commands_reject_nul_and_line_breaks() {
    for command in ["npm run dev\nnpm test", "npm run dev\r", "npm\0run dev"] {
        let error = command_argv(&CommandSpec::Shell(command.into()), &AppKind::EnvPort, 4210)
            .unwrap_err()
            .to_string();

        assert!(error.contains("single-line"));
    }
}

#[cfg(unix)]
#[test]
fn prepare_certs_for_hosts_records_host_before_route_registration() {
    let temp = tempfile::tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        https: true,
        ..ProxySettings::default()
    };

    prepare_certs_for_hosts(&settings, &["web.demo.localhost".into()]).unwrap();

    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let leaf_hosts = fs::read_to_string(store.leaf_hosts_path()).unwrap();
    assert!(leaf_hosts.contains("web.demo.localhost"));
    assert!(store.read_routes(false).unwrap().is_empty());
}

#[test]
fn shell_vite_detection_handles_exec_wrappers() {
    assert!(shell_command_looks_like_vite("bunx vite"));
    assert!(shell_command_looks_like_vite("npx vite"));
    assert!(shell_command_looks_like_vite("pnpm exec vite"));
    assert!(shell_command_looks_like_vite("npx vite@latest"));
    assert!(!shell_command_looks_like_vite("vite build && echo done"));
    assert!(!shell_command_looks_like_vite("vite preview"));
}

#[test]
fn lan_process_routes_reject_non_loopback_targets() {
    let spec = AppRunSpec {
        name: "web".into(),
        dir: Path::new(".").to_path_buf(),
        command: CommandSpec::Argv(vec!["unused".into()]),
        kind: AppKind::EnvPort,
        hostname: "web.demo.localhost".into(),
        target_host: "10.0.0.5".into(),
        explicit_port: None,
        proxy: true,
    };
    let settings = ProxySettings {
        lan: true,
        ..ProxySettings::default()
    };

    let error = process_route_parts(&settings, &spec)
        .unwrap_err()
        .to_string();

    assert!(error.contains("loopback"));
}

#[test]
fn process_routes_require_ip_literal_targets() {
    let spec = AppRunSpec {
        name: "web".into(),
        dir: Path::new(".").to_path_buf(),
        command: CommandSpec::Argv(vec!["unused".into()]),
        kind: AppKind::EnvPort,
        hostname: "web.demo.localhost".into(),
        target_host: "localhost".into(),
        explicit_port: None,
        proxy: true,
    };

    let error = process_route_parts(&ProxySettings::default(), &spec)
        .unwrap_err()
        .to_string();

    assert!(error.contains("must be an IP literal"));
}

#[test]
fn process_routes_require_routed_hostnames_before_launch() {
    let spec = AppRunSpec {
        name: "web".into(),
        dir: Path::new(".").to_path_buf(),
        command: CommandSpec::Argv(vec!["unused".into()]),
        kind: AppKind::EnvPort,
        hostname: "example.com".into(),
        target_host: "127.0.0.1".into(),
        explicit_port: None,
        proxy: true,
    };

    let error = process_route_parts(&ProxySettings::default(), &spec)
        .unwrap_err()
        .to_string();

    assert!(error.contains("private/local suffix"));
}

#[test]
fn proxy_health_requires_consecutive_misses_before_failure() {
    let mut misses = 0;

    assert!(!proxy_health_failed(&mut misses, false));
    assert_eq!(misses, 1);
    assert!(!proxy_health_failed(&mut misses, true));
    assert_eq!(misses, 0);
    assert!(!proxy_health_failed(&mut misses, false));
    assert!(!proxy_health_failed(&mut misses, false));
    assert!(proxy_health_failed(&mut misses, false));
}

#[test]
fn vite_allowed_hosts_uses_configured_tld() {
    let spec = AppRunSpec {
        name: "web".into(),
        dir: Path::new(".").to_path_buf(),
        command: CommandSpec::Argv(vec!["vite".into()]),
        kind: AppKind::Vite,
        hostname: "web.demo.test".into(),
        target_host: "127.0.0.1".into(),
        explicit_port: None,
        proxy: true,
    };
    let settings = ProxySettings {
        tld: "test".into(),
        ..ProxySettings::default()
    };

    assert_eq!(
        vite_allowed_hosts(&spec, &settings).unwrap(),
        "web.demo.test,.test"
    );
}

#[test]
fn vite_allowed_hosts_omits_empty_tld_wildcard() {
    let spec = AppRunSpec {
        name: "web".into(),
        dir: Path::new(".").to_path_buf(),
        command: CommandSpec::Argv(vec!["vite".into()]),
        kind: AppKind::Vite,
        hostname: "web.demo.localhost".into(),
        target_host: "127.0.0.1".into(),
        explicit_port: None,
        proxy: true,
    };
    let settings = ProxySettings {
        tld: " ".into(),
        ..ProxySettings::default()
    };

    assert_eq!(
        vite_allowed_hosts(&spec, &settings).unwrap(),
        "web.demo.localhost"
    );
}

#[test]
fn vite_allowed_hosts_revalidates_env_tokens() {
    let spec = AppRunSpec {
        name: "web".into(),
        dir: Path::new(".").to_path_buf(),
        command: CommandSpec::Argv(vec!["vite".into()]),
        kind: AppKind::Vite,
        hostname: "web,demo.localhost".into(),
        target_host: "127.0.0.1".into(),
        explicit_port: None,
        proxy: true,
    };

    let error = vite_allowed_hosts(&spec, &ProxySettings::default())
        .unwrap_err()
        .to_string();

    assert!(error.contains("invalid"));
}

#[test]
fn dev_table_has_header_and_app_rows() {
    let lines = format_dev_table(&[
        AppDisplay {
            name: "web".into(),
            url: "http://web.demo.localhost:1355".into(),
            pid: 12345,
            lan_note: None,
        },
        AppDisplay {
            name: "api".into(),
            url: "http://api.demo.localhost:1355".into(),
            pid: 12346,
            lan_note: None,
        },
    ]);

    assert!(lines[0].contains("APP"));
    assert!(lines[0].contains("URL"));
    assert!(lines[0].contains("STATUS"));
    assert!(lines[0].contains("PID"));
    assert!(lines[1].contains("web"));
    assert!(lines[1].contains("http://web.demo.localhost:1355"));
    assert!(lines[1].contains("running"));
    assert!(lines[1].ends_with("12345"));
    assert!(!lines[1].contains("pid 12345"));
}

#[test]
fn dev_table_keeps_header_for_single_app() {
    let lines = format_dev_table(&[AppDisplay {
        name: "web".into(),
        url: "http://web.demo.localhost:1355".into(),
        pid: 12345,
        lan_note: None,
    }]);

    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("APP"));
    assert!(lines[1].contains("web"));
}

#[test]
fn dev_app_environment_exports_assigned_app_origins() {
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let api = AppRunSpec::new(
        "api",
        temp.path().to_path_buf(),
        CommandSpec::Shell("cargo run -p demo-api".into()),
        "api.demo.localhost",
    )
    .with_proxy(false);
    let web = AppRunSpec::new(
        "web-app",
        temp.path().to_path_buf(),
        CommandSpec::Argv(vec!["vite".into()]),
        "web-app.demo.localhost",
    )
    .with_kind(AppKind::Vite);

    let env = dev_app_environment(
        [(&api, 41001), (&web, 41002)],
        &ProxySettings::default(),
        &store,
    )
    .unwrap();

    assert!(env.contains(&("JIG_DEV_API_PORT".into(), "41001".into())));
    assert!(env.contains(&("JIG_DEV_API_ORIGIN".into(), "http://127.0.0.1:41001".into())));
    assert!(env.contains(&("JIG_DEV_WEB_APP_PORT".into(), "41002".into())));
    assert!(env.contains(&(
        "JIG_DEV_WEB_APP_ORIGIN".into(),
        "http://web-app.demo.localhost:1355".into()
    )));
}

#[test]
fn dev_app_environment_rejects_duplicate_env_prefixes() {
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let first = AppRunSpec::new(
        "web-app",
        temp.path().to_path_buf(),
        CommandSpec::Argv(vec!["vite".into()]),
        "web-app.demo.localhost",
    )
    .with_proxy(false);
    let second = AppRunSpec::new(
        "web_app",
        temp.path().to_path_buf(),
        CommandSpec::Argv(vec!["vite".into()]),
        "web-app.demo.localhost",
    )
    .with_proxy(false);

    let error = dev_app_environment(
        [(&first, 41001), (&second, 41002)],
        &ProxySettings::default(),
        &store,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("share derived environment prefix JIG_DEV_WEB_APP"));
}

#[test]
fn proxied_app_origin_prefers_https_port_without_http_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    store.write_https_port(1443).unwrap();
    fs::write(store.http_port_path(), "not-a-port").unwrap();
    let settings = ProxySettings {
        https: true,
        ..ProxySettings::default()
    };
    let spec = AppRunSpec::new(
        "web",
        temp.path().to_path_buf(),
        CommandSpec::Argv(vec!["vite".into()]),
        "web.demo.localhost",
    );

    let origin = app_origin(&spec, &settings, 41002, &store).unwrap();
    let display = app_display(&spec, &settings, 41002, 12345, &store).unwrap();

    assert_eq!(origin, "https://web.demo.localhost:1443");
    assert_eq!(display.url, "https://web.demo.localhost:1443");
}

#[test]
fn open_proxy_log_rotates_existing_large_log() {
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    fs::write(
        store.log_path(),
        vec![b'x'; (MAX_PROXY_LOG_BYTES + 1) as usize],
    )
    .unwrap();

    let log = open_proxy_log(&store).unwrap();

    assert!(store.root().join("proxy.log.1").exists());
    assert_eq!(fs::metadata(store.log_path()).unwrap().len(), 0);
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(store.log_path()).unwrap().permissions().mode() & 0o777,
        0o600
    );

    drop(log);
    let rotated = store.root().join("proxy.log.1");
    fs::write(&rotated, b"stale backup").unwrap();
    fs::write(
        store.log_path(),
        vec![b'y'; (MAX_PROXY_LOG_BYTES + 1) as usize],
    )
    .unwrap();

    let _log = open_proxy_log(&store).unwrap();

    assert_eq!(
        fs::metadata(&rotated).unwrap().len(),
        MAX_PROXY_LOG_BYTES + 1
    );
    assert!(!store.root().join("proxy.log.2").exists());
}

#[cfg(unix)]
#[test]
fn open_proxy_log_rejects_hardlinked_log_file() {
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    fs::write(store.log_path(), b"log").unwrap();
    fs::hard_link(store.log_path(), store.root().join("linked-proxy.log")).unwrap();

    let error = open_proxy_log(&store).unwrap_err().to_string();

    assert!(error.contains("hardlinks"));
}

#[test]
fn spawn_child_errors_preserve_io_source() {
    let temp = tempfile::tempdir().unwrap();
    let settings = ProxySettings::default();
    let spec = AppRunSpec {
        name: "missing".into(),
        dir: temp.path().to_path_buf(),
        command: CommandSpec::Argv(Vec::new()),
        kind: AppKind::EnvPort,
        hostname: "missing.example.localhost".into(),
        target_host: "127.0.0.1".into(),
        explicit_port: None,
        proxy: false,
    };
    let argv = ["jig-dev-proxy-definitely-missing-test-command".to_string()];

    let error = spawn_child(&spec, &argv, 4321, &settings, &[]).unwrap_err();

    assert!(error.to_string().contains("executable was not found"));
    assert!(
        error
            .chain()
            .any(|cause| cause.downcast_ref::<std::io::Error>().is_some())
    );
}

#[test]
fn remove_route_best_effort_tolerates_cleanup_failure() {
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    fs::write(store.root().join("routes.json"), b"{not json").unwrap();

    remove_route_best_effort(&store, "app.example.localhost", "app");
}

#[cfg(not(windows))]
#[test]
fn run_apps_launches_non_proxied_apps_without_routes() {
    let temp = tempfile::tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        http_port: 0,
        ..ProxySettings::default()
    };
    let output = run_apps(
        vec![AppRunSpec {
            name: "direct".into(),
            dir: temp.path().to_path_buf(),
            command: CommandSpec::Argv(vec!["sh".into(), "-c".into(), "exit 0".into()]),
            kind: AppKind::EnvPort,
            hostname: "not a route hostname".into(),
            target_host: "localhost".into(),
            // The chosen port is not part of this route-storage assertion.
            // Let run_apps assign it so parallel listener tests cannot steal a
            // preselected explicit port between probe and launch.
            explicit_port: None,
            proxy: false,
        }],
        &settings,
        Path::new("unused-jig"),
    )
    .unwrap();

    assert_eq!(output["ok"].as_bool(), Some(true));
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    assert!(store.read_http_port().unwrap().is_none());
    assert!(store.read_routes(false).unwrap().is_empty());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn app_readiness_wait_returns_when_child_owns_listener() {
    let port = find_free_app_port_excluding("127.0.0.1", &HashSet::new()).unwrap();
    let mut child = spawn_python_listener(port);

    let owner_token = wait_for_app_ready_with_timeout(
        "ready",
        "127.0.0.1",
        port,
        &mut child,
        Duration::from_secs(2),
    )
    .unwrap();

    assert!(owner_token.is_some());
    terminate_child(&mut child);
    let _ = child.wait();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn run_apps_preflights_live_process_route_before_spawning() {
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
    let spec = AppRunSpec::new(
        "api",
        temp.path().to_path_buf(),
        CommandSpec::Argv(vec!["definitely-not-run-by-jig-test".into()]),
        "api.demo.localhost",
    );

    let error = run_apps(vec![spec], &settings, Path::new("/definitely/not/jig"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("would replace a live process route"));
    assert!(error.contains(&std::process::id().to_string()));
    assert!(error.contains("127.0.0.1:4000"));
    assert!(
        !store.pid_path().exists(),
        "preflight should run before proxy startup"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn run_apps_preflights_duplicate_process_route_hostnames_before_spawning() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let first = AppRunSpec::new(
        "api",
        temp.path().to_path_buf(),
        CommandSpec::Argv(vec!["definitely-not-run-by-jig-test".into()]),
        "api.demo.localhost",
    );
    let second = AppRunSpec::new(
        "admin",
        temp.path().to_path_buf(),
        CommandSpec::Argv(vec!["definitely-not-run-by-jig-test".into()]),
        "API.demo.localhost",
    );

    let error = run_apps(
        vec![first, second],
        &settings,
        Path::new("/definitely/not/jig"),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Multiple proxied development apps requested hostname"));
    assert!(error.contains("api"));
    assert!(error.contains("admin"));
    assert!(
        !store.pid_path().exists(),
        "preflight should run before proxy startup"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn app_readiness_wait_errors_when_child_exits_first() {
    let target_host = "127.0.0.1";
    let listener = TcpListener::bind((target_host, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut child = Command::new("sh")
        .args(["-c", "sleep 1; exit 7"])
        .spawn()
        .unwrap();

    let error = wait_for_app_ready_with_timeout(
        "dead",
        target_host,
        port,
        &mut child,
        Duration::from_secs(3),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("exited before listening"));
    let _ = child.wait();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn app_listener_owner_rejects_external_listener() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut command = Command::new("sh");
    command.args(["-c", "sleep 5"]);
    configure_app_child_process_group(&mut command);
    let mut child = command.spawn().unwrap();

    let error = verify_app_listener_owner("external", "127.0.0.1", port, child.id())
        .unwrap_err()
        .to_string();

    assert!(error.contains("refusing to publish process route"));
    terminate_child(&mut child);
    let _ = child.wait();
    drop(listener);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn app_readiness_wait_rejects_port_owned_by_other_process() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut command = Command::new("sh");
    command.args(["-c", "sleep 5"]);
    configure_app_child_process_group(&mut command);
    let mut child = command.spawn().unwrap();

    let error = wait_for_app_ready_with_timeout(
        "raced",
        "127.0.0.1",
        port,
        &mut child,
        Duration::from_secs(2),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("refusing to publish process route"));
    terminate_child(&mut child);
    let _ = child.wait();
    drop(listener);
}

#[cfg(target_os = "linux")]
#[test]
fn app_readiness_wait_rejects_listener_in_different_process_group() {
    let temp = tempdir().unwrap();
    let pid_path = temp.path().join("listener.pid");
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut command = Command::new("python3");
    command.arg("-c").arg(
        "import os, socket, sys, time\n\
             port = int(sys.argv[1])\n\
             pid_path = sys.argv[2]\n\
             pid = os.fork()\n\
             if pid == 0:\n\
                 os.setsid()\n\
                 sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)\n\
                 sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)\n\
                 sock.bind(('127.0.0.1', port))\n\
                 sock.listen()\n\
                 time.sleep(5)\n\
             else:\n\
                 open(pid_path, 'w').write(str(pid))\n\
                 time.sleep(5)\n",
    );
    command.arg(port.to_string()).arg(&pid_path);
    configure_app_child_process_group(&mut command);
    let mut child = command.spawn().unwrap();

    let error = wait_for_app_ready_with_timeout(
        "forked",
        "127.0.0.1",
        port,
        &mut child,
        Duration::from_secs(3),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("refusing to publish process route"));
    if let Ok(pid) = fs::read_to_string(&pid_path)
        .unwrap_or_default()
        .trim()
        .parse::<u32>()
    {
        terminate_pid(pid);
    }
    terminate_child(&mut child);
    let _ = child.wait();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn spawn_python_listener(port: u16) -> Child {
    let mut command = Command::new("python3");
    command.arg("-c").arg(
        "import socket, sys, time\n\
             sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)\n\
             sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)\n\
             sock.bind(('127.0.0.1', int(sys.argv[1])))\n\
             sock.listen()\n\
             time.sleep(5)\n",
    );
    command.arg(port.to_string());
    configure_app_child_process_group(&mut command);
    command.spawn().unwrap()
}

#[test]
fn choose_app_port_rejects_duplicate_explicit_ports() {
    let mut assigned = HashSet::new();
    let mut excluded = HashSet::new();
    let port = (0..10)
        .find_map(|_| {
            let port = find_free_app_port_excluding("127.0.0.1", &excluded).ok()?;
            match choose_app_port(Some(port), "127.0.0.1", &mut assigned) {
                Ok(port) => Some(port),
                Err(_) => {
                    excluded.insert(port);
                    None
                }
            }
        })
        .expect("could not reserve a free port for duplicate-port test");

    let error = choose_app_port(Some(port), "127.0.0.1", &mut assigned)
        .unwrap_err()
        .to_string();
    assert!(error.contains(&format!("Multiple development apps requested port {port}")));
}

#[test]
fn choose_app_port_rejects_zero_explicit_port() {
    let error = choose_app_port(Some(0), "127.0.0.1", &mut HashSet::new())
        .unwrap_err()
        .to_string();
    assert!(error.contains("must be greater than 0"));
    assert!(error.contains("Likely fix"));
}

#[test]
fn ensure_requested_https_rejects_http_only_proxy() {
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        https: true,
        ..ProxySettings::default()
    };

    let error = ensure_requested_https(&store, &settings)
        .unwrap_err()
        .to_string();

    assert!(error.contains("without the requested HTTPS listener"));
    assert!(error.contains("Likely fix"));
    assert!(error.contains(temp.path().to_string_lossy().as_ref()));
}

#[test]
fn proxy_ready_rejects_registered_proxy_on_different_http_port() {
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let token = store.ensure_health_token().unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let actual_port = listener.local_addr().unwrap().port();
    let handle = spawn_proxy_health_response(listener);
    store.write_http_port(actual_port).unwrap();
    store.write_pid(std::process::id()).unwrap();
    let requested_port = if actual_port == 1355 { 1356 } else { 1355 };
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        http_port: requested_port,
        ..ProxySettings::default()
    };

    let error = proxy_ready(&store, &settings).unwrap_err().to_string();
    handle.join().unwrap();

    assert!(error.contains("requested HTTP port"));
    assert!(error.contains(&actual_port.to_string()));
    assert!(error.contains(&requested_port.to_string()));
    assert_eq!(store.read_health_token().unwrap(), Some(token));
}

#[test]
fn proxy_ready_rejects_registered_proxy_on_different_https_port() {
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    store.ensure_health_token().unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let http_port = listener.local_addr().unwrap().port();
    let handle = spawn_proxy_health_response(listener);
    let actual_https_port = 1443;
    let requested_https_port = 1556;
    store.write_http_port(http_port).unwrap();
    store.write_https_port(actual_https_port).unwrap();
    store.write_pid(std::process::id()).unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        http_port,
        https: true,
        https_port: Some(requested_https_port),
        ..ProxySettings::default()
    };

    let error = proxy_ready(&store, &settings).unwrap_err().to_string();
    handle.join().unwrap();

    assert!(error.contains("requested HTTPS port"));
    assert!(error.contains(&actual_https_port.to_string()));
    assert!(error.contains(&requested_https_port.to_string()));
}

fn spawn_proxy_health_response(listener: TcpListener) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 512];
        let _ = stream.read(&mut request).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nx-jig-proxy: 1\r\nx-jig-proxy-pid: {}\r\ncontent-length: 0\r\n\r\n",
            std::process::id()
        )
        .unwrap();
    })
}

#[test]
fn ensure_proxy_running_rejects_proxy_from_other_state_dir() {
    let temp = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 512];
        let _ = stream.read(&mut request).unwrap();
        stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nx-jig-proxy: 1\r\nx-jig-proxy-pid: 123\r\ncontent-length: 11\r\n\r\n{\"ok\":true}",
                )
                .unwrap();
    });
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        http_port: port,
        ..ProxySettings::default()
    };

    let error = ensure_proxy_running(&settings, Path::new("unused-jig"))
        .unwrap_err()
        .to_string();
    handle.join().unwrap();

    assert!(error.contains("already running on HTTP port"));
    assert!(error.contains(temp.path().to_string_lossy().as_ref()));
}

#[test]
fn ensure_proxy_running_identifies_foreign_jig_proxy_without_health_token() {
    let temp = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 512];
        let _ = stream.read(&mut request).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 403 Forbidden\r\nx-jig-proxy: 1\r\ncontent-length: 9\r\n\r\nForbidden",
            )
            .unwrap();
    });
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        http_port: port,
        ..ProxySettings::default()
    };

    let error = ensure_proxy_running(&settings, Path::new("unused-jig"))
        .unwrap_err()
        .to_string();
    handle.join().unwrap();

    assert!(error.contains("cannot authenticate"));
    assert!(error.contains(temp.path().to_string_lossy().as_ref()));
}
