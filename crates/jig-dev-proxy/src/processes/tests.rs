use std::io::{Read, Write};
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::PathBuf;
use std::thread;

use super::*;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use tempfile::tempdir;

#[cfg(target_os = "linux")]
fn process_identity_is_alive(pid: u32, start_token: &str) -> bool {
    crate::state::pid_is_alive(pid)
        && crate::state::process_start_token(pid).as_deref() == Some(start_token)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
struct TestStopFile(PathBuf);

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl TestStopFile {
    fn stop(&self) {
        fs::write(&self.0, b"stop\n").unwrap();
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Drop for TestStopFile {
    fn drop(&mut self) {
        let _ = fs::write(&self.0, b"stop\n");
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn write_process_test_marker(path: &Path, contents: &str) {
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&temporary, contents).unwrap();
    fs::rename(temporary, path).unwrap();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn wait_for_process_test_marker(path: &Path, child: &mut Child, label: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(contents) = fs::read_to_string(path) {
            if !contents.trim().is_empty() {
                return contents;
            }
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("{label} exited before publishing its marker: {status}");
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("{label} did not publish its marker");
}

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
    let reason = TerminationReason::from_signal({
        #[cfg(unix)]
        {
            libc::SIGINT
        }
        #[cfg(not(unix))]
        {
            2
        }
    });
    let error = ensure_not_interrupted_with(|| Some(reason)).unwrap_err();

    assert!(error.to_string().starts_with("Interrupted by "));
    assert!(is_interruption(&error));
    assert_eq!(interruption_reason(&error), Some(reason));
}

#[test]
fn typed_preflight_cancellation_requires_a_pending_termination_reason() {
    let reason = TerminationReason::from_signal({
        #[cfg(unix)]
        {
            libc::SIGTERM
        }
        #[cfg(not(unix))]
        {
            2
        }
    });

    let interrupted =
        normalize_preflight_result(Err(DevPreflightError::cancelled()), Some(reason)).unwrap_err();
    assert_eq!(interruption_reason(&interrupted), Some(reason));

    let unconfirmed =
        normalize_preflight_result(Err(DevPreflightError::cancelled()), None).unwrap_err();
    assert!(!is_interruption(&unconfirmed));
    assert!(unconfirmed.to_string().contains("without a pending"));
}

#[test]
fn preflight_failure_survives_even_when_termination_is_pending() {
    let reason = TerminationReason::from_signal({
        #[cfg(unix)]
        {
            libc::SIGINT
        }
        #[cfg(not(unix))]
        {
            2
        }
    });
    let failure = anyhow::anyhow!("preflight cleanup failure sentinel");

    let error = normalize_preflight_result(Err(DevPreflightError::failed(failure)), Some(reason))
        .unwrap_err();

    assert!(!is_interruption(&error));
    assert_eq!(error.to_string(), "preflight cleanup failure sentinel");
}

#[cfg(unix)]
#[test]
fn child_exit_status_preserves_shell_signal_statuses() {
    for (signal_name, expected) in [("HUP", 129), ("INT", 130), ("TERM", 143)] {
        let status = Command::new("sh")
            .args(["-c", &format!("kill -{signal_name} $$")])
            .status()
            .unwrap();

        assert_eq!(status.code(), None);
        assert_eq!(child_exit_status(&status), expected);
    }
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

#[test]
fn termination_test_serialization_recovers_a_poisoned_mutex() {
    let mutex = std::sync::Mutex::new(());
    let panic = std::panic::catch_unwind(|| {
        let _guard = mutex.lock().unwrap();
        panic!("deliberately poison the local test mutex");
    });

    assert!(panic.is_err());
    assert!(mutex.is_poisoned());
    drop(recover_test_mutex_guard(&mutex));
    drop(recover_test_mutex_guard(&mutex));
}

#[cfg(target_os = "linux")]
#[test]
fn terminate_child_kills_process_group_grandchild() {
    let _guard = termination_test_guard();
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
        terminate_child(&mut child).unwrap();
        let _ = child.wait();
        panic!("grandchild PID was not written");
    };
    let Some(grandchild_start_token) = crate::state::process_start_token(grandchild_pid) else {
        terminate_child(&mut child).unwrap();
        let _ = child.wait();
        panic!("grandchild process {grandchild_pid} had no start token");
    };

    terminate_child(&mut child).unwrap();
    let _ = child.wait();

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !process_identity_is_alive(grandchild_pid, &grandchild_start_token) {
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
fn dev_child_environment_replaces_only_runtime_owned_app_coordinates() {
    let inherited = [
        ("JIG_DEV_API_ORIGIN", "http://stale.invalid"),
        ("JIG_DEV_OLD_APP_PORT", "4999"),
        ("API_ORIGIN", "http://remote.example"),
        ("JIG_DEV_BIN", "/tmp/jig"),
        ("JIG_DEV_ALLOW_WORKSPACE_DISCOVERY", "1"),
        ("JIG_DEV_API_ORIGIN_EXTRA", "keep-me"),
    ];
    let mut command = Command::new("unused");
    for (key, value) in inherited {
        command.env(key, value);
    }
    apply_dev_app_environment(
        &mut command,
        inherited.into_iter().map(|(key, _)| OsString::from(key)),
        &[
            ("JIG_DEV_API_ORIGIN".into(), "http://127.0.0.1:41001".into()),
            ("JIG_DEV_WEB_PORT".into(), "41002".into()),
        ],
    );

    let configured = command
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect::<HashMap<_, _>>();
    assert_eq!(
        configured.get("JIG_DEV_API_ORIGIN"),
        Some(&Some("http://127.0.0.1:41001".into()))
    );
    assert_eq!(
        configured.get("JIG_DEV_WEB_PORT"),
        Some(&Some("41002".into()))
    );
    assert_eq!(configured.get("JIG_DEV_OLD_APP_PORT"), Some(&None));
    assert_eq!(
        configured.get("API_ORIGIN"),
        Some(&Some("http://remote.example".into()))
    );
    assert_eq!(
        configured.get("JIG_DEV_BIN"),
        Some(&Some("/tmp/jig".into()))
    );
    assert_eq!(
        configured.get("JIG_DEV_ALLOW_WORKSPACE_DISCOVERY"),
        Some(&Some("1".into()))
    );
    assert_eq!(
        configured.get("JIG_DEV_API_ORIGIN_EXTRA"),
        Some(&Some("keep-me".into()))
    );
}

#[test]
fn generated_npm_dev_environment_removes_only_execution_shaping_config() {
    let argv = GENERATED_NPM_DEV_ARGV_PREFIX
        .into_iter()
        .map(str::to_owned)
        .chain([
            "--".into(),
            "--port".into(),
            "41002".into(),
            "--strictPort".into(),
            "--host".into(),
            "127.0.0.1".into(),
        ])
        .collect::<Vec<_>>();
    let inherited = [
        ("NPM_CONFIG_WORKSPACE", "missing"),
        ("npm_config_include_workspace_root", "false"),
        ("NpM_CoNfIg_LoCaTiOn", "global"),
        ("NPM_CONFIG_IF_PRESENT", "true"),
        ("NPM_CONFIG_OMIT", "dev"),
        ("NODE_ENV", "staging"),
        ("NPM_CONFIG_REGISTRY", "https://registry.example"),
        ("npm_config_install_strategy", "nested"),
        ("NPM_CONFIG_IGNORE_SCRIPTS", "true"),
        ("APP_FEATURE", "enabled"),
    ];
    let mut command = Command::new("unused");
    for (key, value) in inherited {
        command.env(key, value);
    }

    apply_dev_child_environment(
        &mut command,
        &argv,
        inherited.into_iter().map(|(key, _)| OsString::from(key)),
        &[],
    );

    let configured = command
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect::<HashMap<_, _>>();
    for removed in [
        "NPM_CONFIG_WORKSPACE",
        "npm_config_include_workspace_root",
        "NpM_CoNfIg_LoCaTiOn",
        "NPM_CONFIG_IF_PRESENT",
        "NPM_CONFIG_OMIT",
    ] {
        assert_eq!(configured.get(removed), Some(&None), "{removed}");
    }
    for (preserved, value) in [
        ("NODE_ENV", "staging"),
        ("NPM_CONFIG_REGISTRY", "https://registry.example"),
        ("npm_config_install_strategy", "nested"),
        ("NPM_CONFIG_IGNORE_SCRIPTS", "true"),
        ("APP_FEATURE", "enabled"),
    ] {
        assert_eq!(
            configured.get(preserved),
            Some(&Some(value.into())),
            "{preserved}"
        );
    }
}

#[test]
fn custom_npm_dev_environment_remains_project_owned() {
    let inherited = [
        ("NPM_CONFIG_WORKSPACE", "custom-workspace"),
        ("NPM_CONFIG_OMIT", "optional"),
        ("NODE_ENV", "production"),
    ];
    let mut command = Command::new("unused");
    for (key, value) in inherited {
        command.env(key, value);
    }

    apply_dev_child_environment(
        &mut command,
        &["npm".into(), "run".into(), "dev".into()],
        inherited.into_iter().map(|(key, _)| OsString::from(key)),
        &[],
    );

    let configured = command
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect::<HashMap<_, _>>();
    for (key, value) in inherited {
        assert_eq!(configured.get(key), Some(&Some(value.into())), "{key}");
    }
}

#[test]
fn canonical_generated_npm_dev_argv_accepts_only_jig_vite_suffix() {
    let mut argv = GENERATED_NPM_DEV_ARGV_PREFIX
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert!(is_generated_npm_dev_argv(&argv));

    inject_framework_flags(&mut argv, &AppKind::Vite, 4210);
    assert!(is_generated_npm_dev_argv(&argv));
    assert_eq!(
        &argv[GENERATED_NPM_DEV_ARGV_PREFIX.len()..],
        [
            "--",
            "--port",
            "4210",
            "--strictPort",
            "--host",
            "127.0.0.1"
        ]
    );

    let mut noncanonical = GENERATED_NPM_DEV_ARGV_PREFIX
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    noncanonical.push("--mode=custom".into());
    assert!(!is_generated_npm_dev_argv(&noncanonical));
    let mut forged_suffix = GENERATED_NPM_DEV_ARGV_PREFIX
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    forged_suffix.extend(["--".into(), "--mode=custom".into()]);
    assert!(!is_generated_npm_dev_argv(&forged_suffix));
    assert!(!is_generated_npm_dev_argv(&[
        "npm".into(),
        "run".into(),
        "dev".into()
    ]));
}

#[test]
fn managed_npm_run_environment_key_matching_is_case_insensitive_and_narrow() {
    for key in MANAGED_NPM_RUN_CONFIG_KEYS {
        let hyphenated = format!("npm_config_{key}");
        assert!(
            is_managed_npm_run_environment_key(OsStr::new(&hyphenated)),
            "{hyphenated}"
        );
        let key = format!("NpM_CoNfIg_{}", key.replace('-', "_"));
        assert!(
            is_managed_npm_run_environment_key(OsStr::new(&key)),
            "{key}"
        );
    }
    for key in [
        "NODE_ENV",
        "NPM_CONFIG_REGISTRY",
        "NPM_CONFIG_NODE_OPTIONS",
        "NPM_CONFIG_INSTALL_STRATEGY",
        "NPM_CONFIG_LEGACY_PEER_DEPS",
        "NPM_CONFIG_STRICT_PEER_DEPS",
        "NPM_CONFIG_IGNORE_SCRIPTS",
        "NPM_CONFIG_FOREGROUND_SCRIPTS",
        "NPM_CONFIG_SCRIPT_SHELL",
        "NPM_CONFIG_//REGISTRY.EXAMPLE/:_AUTH_TOKEN",
    ] {
        assert!(
            !is_managed_npm_run_environment_key(OsStr::new(key)),
            "{key}"
        );
    }
}

#[test]
fn runtime_owned_app_coordinate_key_matching_is_exact_and_case_insensitive() {
    for key in [
        "JIG_DEV_API_HOST",
        "JIG_DEV_API_PORT",
        "JIG_DEV_WEB_APP_ORIGIN",
        "JIG_DEV___PORT",
    ] {
        assert!(
            is_runtime_owned_dev_app_environment_key(OsStr::new(key)),
            "expected runtime-owned key: {key}"
        );
    }
    assert_eq!(
        is_runtime_owned_dev_app_environment_key(OsStr::new("jig_dev_web_app_url")),
        cfg!(windows),
        "environment key case sensitivity must match the target platform"
    );
    for key in [
        "API_ORIGIN",
        "JIG_DEV_BIN",
        "JIG_DEV_ALLOW_WORKSPACE_DISCOVERY",
        "JIG_DEV_API",
        "JIG_DEV__ORIGIN",
        "JIG_DEV_API_ORIGIN_EXTRA",
        "JIG_DEV_API_origin_suffix",
    ] {
        assert!(
            !is_runtime_owned_dev_app_environment_key(OsStr::new(key)),
            "unexpected runtime-owned key: {key}"
        );
    }
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

    let error = spawn_child(&spec, &argv, 4321, &settings, &[])
        .err()
        .expect("missing executable should fail to spawn");

    assert!(error.to_string().contains("executable was not found"));
    assert!(
        error
            .chain()
            .any(|cause| cause.downcast_ref::<std::io::Error>().is_some())
    );
}

#[cfg(not(windows))]
#[test]
fn spawn_child_captures_output_without_inheriting_the_terminal() {
    let temp = tempfile::tempdir().unwrap();
    let settings = ProxySettings::default();
    let spec = AppRunSpec {
        name: "captured".into(),
        dir: temp.path().to_path_buf(),
        command: CommandSpec::Argv(Vec::new()),
        kind: AppKind::EnvPort,
        hostname: "captured.example.localhost".into(),
        target_host: "127.0.0.1".into(),
        explicit_port: None,
        proxy: false,
    };
    let argv = [
        "sh".to_string(),
        "-c".to_string(),
        "printf 'captured stdout\\n'; printf 'captured stderr\\n' >&2".to_string(),
    ];

    let mut spawned = spawn_child(&spec, &argv, 4321, &settings, &[]).unwrap();
    assert!(spawned.child.wait().unwrap().success());
    let output = String::from_utf8(spawned.output.captured_bytes()).unwrap();

    assert!(output.contains("captured stdout"));
    assert!(output.contains("captured stderr"));
}

#[cfg(not(windows))]
#[test]
fn spawn_child_keeps_astro_in_the_supervised_foreground_group() {
    let temp = tempfile::tempdir().unwrap();
    let settings = ProxySettings::default();
    let spec = AppRunSpec {
        name: "astro".into(),
        dir: temp.path().to_path_buf(),
        command: CommandSpec::Argv(Vec::new()),
        kind: AppKind::EnvPort,
        hostname: "astro.example.localhost".into(),
        target_host: "127.0.0.1".into(),
        explicit_port: None,
        proxy: false,
    };
    let argv = [
        "sh".to_string(),
        "-c".to_string(),
        "printf '%s' \"$ASTRO_DEV_BACKGROUND\"".to_string(),
    ];
    let dev_env = [("ASTRO_DEV_BACKGROUND".to_string(), String::new())];

    let mut spawned = spawn_child(&spec, &argv, 4321, &settings, &dev_env).unwrap();
    assert!(spawned.child.wait().unwrap().success());

    assert_eq!(spawned.output.captured_bytes(), b"0");
}

#[test]
fn unsupported_app_supervision_fails_before_spawn() {
    let error = ensure_app_supervision_supported(false, false).unwrap_err();

    assert!(error.to_string().contains("supervision is unsupported"));
    assert!(error.to_string().contains("refusing to spawn"));
}

#[test]
fn publication_error_after_write_is_cleaned_by_exact_ownership() {
    if !process_start_tokens_supported() {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let hostname = RouteHostname::new("committed.localhost").unwrap();
    let owner_pid = std::process::id();
    let owner_start_token = crate::state::process_start_token(owner_pid).unwrap();
    let ownership =
        ProcessRouteOwnership::new(hostname.clone(), owner_pid, owner_start_token.clone());
    let error = store
        .add_route_then_fail_after_write(Route {
            hostname,
            target_host: "127.0.0.1".into(),
            target_port: 4321,
            owner_pid: Some(owner_pid),
            owner_start_token: Some(owner_start_token),
            mode: RouteMode::Process,
            created_at_ms: now_ms(),
        })
        .unwrap_err();
    assert!(error.to_string().contains("after durable write"));
    assert_eq!(store.read_routes(false).unwrap().len(), 1);

    remove_route_best_effort(&store, &ownership, "committed", true);

    assert!(store.read_routes(false).unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn captured_output_does_not_wait_for_grandchildren_holding_pipes_open() {
    let _guard = termination_test_guard();
    let temp = tempfile::tempdir().unwrap();
    let settings = ProxySettings::default();
    let spec = AppRunSpec {
        name: "captured".into(),
        dir: temp.path().to_path_buf(),
        command: CommandSpec::Argv(Vec::new()),
        kind: AppKind::EnvPort,
        hostname: "captured.example.localhost".into(),
        target_host: "127.0.0.1".into(),
        explicit_port: None,
        proxy: false,
    };
    let argv = [
        "sh".to_string(),
        "-c".to_string(),
        "sleep 5 & printf 'failure before wrapper exit\\n'; exit 1".to_string(),
    ];
    let mut spawned = spawn_child(&spec, &argv, 4321, &settings, &[]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        match try_wait_preserving_process_group(&mut spawned.child).unwrap() {
            Some(status) => break status,
            None if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            None => panic!("wrapper did not exit"),
        }
    };
    assert!(!status.success());
    assert!(process_group_alive(spawned.child.id()).unwrap());

    let output = spawned.output.captured_bytes();

    assert!(output.ends_with(b"failure before wrapper exit\n"));
    assert!(
        process_group_alive(spawned.child.id()).unwrap(),
        "output capture completion must not depend on the pipe-owning grandchild exiting"
    );
    terminate_and_reap(&mut spawned.child).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn repeated_silent_escaped_pipe_owners_do_not_leave_capture_threads() {
    struct EscapedWriterGuard {
        stop: std::path::PathBuf,
        armed: bool,
    }

    impl EscapedWriterGuard {
        fn stop(mut self, pid: u32, start_token: &str) {
            fs::write(&self.stop, b"stop\n").unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            while process_identity_is_alive(pid, start_token) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            self.armed = false;
            assert!(
                !process_identity_is_alive(pid, start_token),
                "escaped writer {pid} did not stop cooperatively"
            );
        }
    }

    impl Drop for EscapedWriterGuard {
        fn drop(&mut self) {
            if self.armed {
                let _ = fs::write(&self.stop, b"stop\n");
            }
        }
    }

    let _guard = termination_test_guard();
    for iteration in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let pid_path = temp.path().join("escaped-writer.pid");
        let stop_path = temp.path().join("stop-escaped-writer");
        let writer_guard = EscapedWriterGuard {
            stop: stop_path.clone(),
            armed: true,
        };
        let settings = ProxySettings::default();
        let spec = AppRunSpec {
            name: format!("captured-{iteration}"),
            dir: temp.path().to_path_buf(),
            command: CommandSpec::Argv(Vec::new()),
            kind: AppKind::EnvPort,
            hostname: format!("captured-{iteration}.example.localhost"),
            target_host: "127.0.0.1".into(),
            explicit_port: None,
            proxy: false,
        };
        let argv = [
            "sh".to_string(),
            "-c".to_string(),
            "setsid sh -c 'printf \"%s\\n\" \"$$\" > \"$1\"; while [ ! -f \"$2\" ]; do sleep 0.05; done' sh \"$1\" \"$2\" & while [ ! -s \"$1\" ]; do sleep 0.01; done; exit 1".to_string(),
            "sh".to_string(),
            pid_path.display().to_string(),
            stop_path.display().to_string(),
        ];
        let mut spawned = spawn_child(&spec, &argv, 4321, &settings, &[]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let status = loop {
            match try_wait_preserving_process_group(&mut spawned.child).unwrap() {
                Some(status) => break status,
                None if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                None => panic!("wrapper did not exit after launching escaped writer"),
            }
        };
        assert!(!status.success());
        let escaped_pid = fs::read_to_string(&pid_path)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        let escaped_start_token = crate::state::process_start_token(escaped_pid).unwrap();
        terminate_and_reap(&mut spawned.child).unwrap();

        assert!(spawned.output.captured_bytes().is_empty());

        assert_eq!(spawned.output.active_reader_count(), 0);
        assert!(
            process_identity_is_alive(escaped_pid, &escaped_start_token),
            "capture completion unexpectedly depended on escaped writer exit"
        );
        writer_guard.stop(escaped_pid, &escaped_start_token);
    }
}

#[cfg(not(windows))]
#[test]
fn failure_tail_is_finalized_after_process_group_shutdown() {
    let _guard = termination_test_guard();
    let temp = tempfile::tempdir().unwrap();
    let ready = temp.path().join("term-trap-ready");
    let release = temp.path().join("release-wrapper");
    let settings = ProxySettings::default();
    let spec = AppRunSpec {
        name: "captured".into(),
        dir: temp.path().to_path_buf(),
        command: CommandSpec::Argv(Vec::new()),
        kind: AppKind::EnvPort,
        hostname: "captured.example.localhost".into(),
        target_host: "127.0.0.1".into(),
        explicit_port: None,
        proxy: false,
    };
    let argv = [
        "sh".to_string(),
        "-c".to_string(),
        "(trap 'printf shutdown-tail\\n >&2; exit 0' TERM; : > \"$1\"; while :; do sleep 1; done) & while [ ! -f \"$1\" ]; do sleep 0.01; done; while [ ! -f \"$2\" ]; do sleep 0.01; done; printf wrapper-failed\\n; exit 1".to_string(),
        "sh".to_string(),
        ready.display().to_string(),
        release.display().to_string(),
    ];
    let mut spawned = spawn_child(&spec, &argv, 4321, &settings, &[]).unwrap();
    let ready_deadline = Instant::now() + Duration::from_secs(2);
    while !ready.exists() && Instant::now() < ready_deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if !ready.exists() {
        terminate_and_reap(&mut spawned.child).unwrap();
        panic!("TERM trap did not report readiness");
    }
    fs::write(&release, b"release\n").unwrap();

    let exit_deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        match try_wait_preserving_process_group(&mut spawned.child).unwrap() {
            Some(status) => break status,
            None if Instant::now() < exit_deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            None => {
                terminate_and_reap(&mut spawned.child).unwrap();
                panic!("wrapper did not exit after release");
            }
        }
    };
    assert!(!status.success());

    terminate_and_reap(&mut spawned.child).unwrap();
    let output = String::from_utf8(spawned.output.captured_bytes()).unwrap();

    assert!(output.contains("wrapper-failed"));
    assert!(output.contains("shutdown-tail"));
}

#[test]
fn captured_output_keeps_a_bounded_tail() {
    let mut buffer = TailBuffer::default();
    buffer.push(&vec![b'a'; MAX_APP_OUTPUT_BYTES]);
    buffer.push(b"failure tail");

    assert_eq!(buffer.bytes.len(), MAX_APP_OUTPUT_BYTES);
    assert!(
        buffer
            .bytes
            .iter()
            .copied()
            .collect::<Vec<_>>()
            .ends_with(b"failure tail")
    );
    assert!(buffer.truncated);
}

#[test]
fn remove_route_best_effort_tolerates_cleanup_failure() {
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    fs::write(store.root().join("routes.json"), b"{not json").unwrap();
    let ownership = ProcessRouteOwnership::new(
        RouteHostname::new("app.example.localhost").unwrap(),
        4242,
        "owner".into(),
    );

    remove_route_best_effort(&store, &ownership, "app", true);
}

#[test]
fn failed_app_output_is_finalized_before_route_cleanup_error_returns() {
    let finalized = std::cell::Cell::new(false);

    finalize_single_app_cleanup(
        false,
        true,
        Some(anyhow::anyhow!("route cleanup failed")),
        || finalized.set(true),
    )
    .unwrap();

    assert!(finalized.get());
}

#[test]
fn successful_app_does_not_print_failure_before_route_cleanup_error() {
    let finalized = std::cell::Cell::new(false);

    finalize_single_app_cleanup(
        true,
        true,
        Some(anyhow::anyhow!("route cleanup failed")),
        || finalized.set(true),
    )
    .unwrap_err();

    assert!(!finalized.get());
}

#[test]
fn successful_app_rejects_unconfirmed_process_cleanup() {
    let error = finalize_single_app_cleanup(true, false, None, || {}).unwrap_err();

    assert!(error.to_string().contains("process-tree cleanup"));
}

#[test]
fn successful_multi_app_session_requires_complete_cleanup() {
    assert!(require_cleanup_for_success(true, false).is_ok());
    assert!(require_cleanup_for_success(false, false).is_err());
}

#[test]
fn failed_multi_app_session_preserves_primary_failure_when_cleanup_is_incomplete() {
    assert!(require_cleanup_for_success(false, true).is_ok());
}

#[cfg(not(windows))]
#[test]
fn run_apps_launches_non_proxied_apps_without_routes() {
    let _guard = termination_test_guard();
    let temp = tempfile::tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        http_port: 0,
        ..ProxySettings::default()
    };
    let output = run_apps_with_interrupt_probe(
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
        || None,
    )
    .unwrap();

    assert_eq!(output["ok"].as_bool(), Some(true));
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    assert!(store.read_http_port().unwrap().is_none());
    assert!(store.read_routes(false).unwrap().is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn interrupted_run_apps_reaps_spawned_child_before_returning() {
    let _guard = termination_test_guard();
    let temp = tempdir().unwrap();
    let child_pid_path = temp.path().join("child.pid");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        http_port: 0,
        ..ProxySettings::default()
    };
    let spec = AppRunSpec {
        name: "direct".into(),
        dir: temp.path().to_path_buf(),
        command: CommandSpec::Argv(vec![
            "sh".into(),
            "-c".into(),
            "echo $$ > \"$1\"; sleep 60".into(),
            "sh".into(),
            child_pid_path.display().to_string(),
        ]),
        kind: AppKind::EnvPort,
        hostname: "unused.localhost".into(),
        target_host: "127.0.0.1".into(),
        explicit_port: None,
        proxy: false,
    };

    let child_identity = std::sync::OnceLock::new();
    let error =
        run_apps_with_interrupt_probe(vec![spec], &settings, Path::new("unused-jig"), || {
            if child_identity.get().is_none() {
                let identity = fs::read_to_string(&child_pid_path)
                    .ok()
                    .and_then(|text| text.trim().parse::<u32>().ok())
                    .and_then(|pid| {
                        crate::state::process_start_token(pid).map(|token| (pid, token))
                    });
                if let Some(identity) = identity {
                    let _ = child_identity.set(identity);
                }
            }
            child_identity
                .get()
                .is_some()
                .then_some(TerminationReason::from_signal(libc::SIGINT))
        })
        .unwrap_err();

    assert!(is_interruption(&error));
    let (child_pid, child_start_token) = child_identity.into_inner().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !process_identity_is_alive(child_pid, &child_start_token) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("interrupted child process {child_pid} survived dev-session cleanup");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn app_readiness_wait_returns_when_child_owns_listener() {
    const HELPER_ENV: &str = "JIG_APP_READINESS_OWN_LISTENER_HELPER";
    const MARKER_ENV: &str = "JIG_APP_READINESS_OWN_LISTENER_MARKER";
    const STOP_ENV: &str = "JIG_APP_READINESS_OWN_LISTENER_STOP";
    const TEST_NAME: &str = "processes::tests::app_readiness_wait_returns_when_child_owns_listener";

    if std::env::var_os(HELPER_ENV).is_some() {
        let marker = PathBuf::from(std::env::var_os(MARKER_ENV).expect("helper marker path"));
        let stop = PathBuf::from(std::env::var_os(STOP_ENV).expect("helper stop path"));
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        write_process_test_marker(
            &marker,
            &format!("{}\n", listener.local_addr().unwrap().port()),
        );
        while !stop.exists() {
            thread::sleep(Duration::from_millis(10));
        }
        return;
    }

    let _guard = termination_test_guard();
    let temp = tempdir().unwrap();
    let marker = temp.path().join("listener.marker");
    let stop = TestStopFile(temp.path().join("stop-listener"));
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", TEST_NAME, "--nocapture"])
        .env(HELPER_ENV, "1")
        .env(MARKER_ENV, &marker)
        .env(STOP_ENV, &stop.0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_app_child_process_group(&mut command);
    let mut child = command.spawn().unwrap();
    let port = wait_for_process_test_marker(&marker, &mut child, "listener helper")
        .trim()
        .parse()
        .unwrap();

    let owner_token = wait_for_app_ready_with_timeout(
        "ready",
        "127.0.0.1",
        port,
        &mut child,
        Duration::from_secs(2),
    )
    .unwrap();

    assert!(owner_token.is_some());
    stop.stop();
    assert!(child.wait().unwrap().success());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn run_apps_preflights_live_process_route_before_spawning() {
    let _guard = termination_test_guard();
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

    let error = run_apps_with_interrupt_probe(
        vec![spec],
        &settings,
        Path::new("/definitely/not/jig"),
        || None,
    )
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
    let _guard = termination_test_guard();
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

    let error = run_apps_with_interrupt_probe(
        vec![first, second],
        &settings,
        Path::new("/definitely/not/jig"),
        || None,
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
    let _guard = termination_test_guard();
    struct ChildCleanup(Child);
    impl Drop for ChildCleanup {
        fn drop(&mut self) {
            terminate_and_reap_logged(&mut self.0, "test cleanup failed");
        }
    }
    let target_host = "127.0.0.1";
    let mut command = Command::new("sh");
    command.args(["-c", "sleep 1; exit 7"]);
    configure_app_child_process_group(&mut command);
    let mut child = ChildCleanup(command.spawn().unwrap());

    let error = wait_for_app_ready_with_timeout_and_test_probe(
        "dead",
        target_host,
        43_211,
        &mut child.0,
        Duration::from_secs(3),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("exited before listening"));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn production_readiness_path_waits_for_delayed_owned_listener() {
    const HELPER_ENV: &str = "JIG_APP_READINESS_DELAYED_LISTENER_HELPER";
    const MARKER_ENV: &str = "JIG_APP_READINESS_DELAYED_LISTENER_MARKER";
    const START_ENV: &str = "JIG_APP_READINESS_DELAYED_LISTENER_START";
    const STOP_ENV: &str = "JIG_APP_READINESS_DELAYED_LISTENER_STOP";
    const TEST_NAME: &str =
        "processes::tests::production_readiness_path_waits_for_delayed_owned_listener";

    if std::env::var_os(HELPER_ENV).is_some() {
        let marker = PathBuf::from(std::env::var_os(MARKER_ENV).expect("helper marker path"));
        let start = PathBuf::from(std::env::var_os(START_ENV).expect("helper start path"));
        let stop = PathBuf::from(std::env::var_os(STOP_ENV).expect("helper stop path"));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();
        let _runtime_guard = runtime.enter();
        // Reserve the port before the delay without accepting connections, so
        // the fixture cannot lose it while proving that readiness keeps waiting.
        let socket = tokio::net::TcpSocket::new_v4().unwrap();
        socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let port = socket.local_addr().unwrap().port();
        write_process_test_marker(&marker, &format!("{port}\n"));
        let start_deadline = Instant::now() + Duration::from_secs(5);
        while !start.exists() && !stop.exists() && Instant::now() < start_deadline {
            thread::sleep(Duration::from_millis(10));
        }
        if !start.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(750));
        let _listener = socket.listen(128).unwrap();
        let exit_deadline = Instant::now() + Duration::from_secs(10);
        while !stop.exists() && Instant::now() < exit_deadline {
            thread::sleep(Duration::from_millis(10));
        }
        return;
    }

    let _guard = termination_test_guard();
    struct ChildCleanup(Child);
    impl Drop for ChildCleanup {
        fn drop(&mut self) {
            terminate_and_reap_logged(&mut self.0, "test cleanup failed");
        }
    }
    let temp = tempdir().unwrap();
    let marker = temp.path().join("delayed-listener.marker");
    let start = temp.path().join("start-delayed-listener");
    let stop = TestStopFile(temp.path().join("stop-delayed-listener"));
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", TEST_NAME, "--nocapture"])
        .env(HELPER_ENV, "1")
        .env(MARKER_ENV, &marker)
        .env(START_ENV, &start)
        .env(STOP_ENV, &stop.0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_app_child_process_group(&mut command);
    let mut child = ChildCleanup(command.spawn().unwrap());
    let port = wait_for_process_test_marker(&marker, &mut child.0, "delayed listener helper")
        .trim()
        .parse()
        .unwrap();

    let started_at = Instant::now();
    fs::write(&start, b"start\n").unwrap();
    let owner_token = wait_for_app_ready(
        &AppRunSpec::new(
            "delayed",
            std::env::current_dir().unwrap(),
            CommandSpec::Argv(vec![]),
            "delayed.localhost",
        ),
        port,
        &mut child.0,
    )
    .unwrap();

    assert!(owner_token.is_some());
    assert!(
        started_at.elapsed() >= Duration::from_millis(600),
        "production readiness returned before the delayed listener started"
    );
    stop.stop();
    assert!(child.0.wait().unwrap().success());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn app_listener_owner_rejects_external_listener() {
    let _guard = termination_test_guard();
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
    terminate_child(&mut child).unwrap();
    let _ = child.wait();
    drop(listener);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn app_readiness_wait_rejects_port_owned_by_other_process() {
    let _guard = termination_test_guard();
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
    terminate_child(&mut child).unwrap();
    let _ = child.wait();
    drop(listener);
}

#[cfg(target_os = "linux")]
#[test]
fn app_readiness_wait_rejects_listener_in_different_process_group() {
    const ROLE_ENV: &str = "JIG_APP_READINESS_DETACHED_ROLE";
    const MARKER_ENV: &str = "JIG_APP_READINESS_DETACHED_MARKER";
    const STOP_ENV: &str = "JIG_APP_READINESS_DETACHED_STOP";
    const TEST_NAME: &str =
        "processes::tests::app_readiness_wait_rejects_listener_in_different_process_group";

    match std::env::var(ROLE_ENV).ok().as_deref() {
        Some("listener") => {
            let marker = PathBuf::from(std::env::var_os(MARKER_ENV).expect("helper marker path"));
            let stop = PathBuf::from(std::env::var_os(STOP_ENV).expect("helper stop path"));
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            write_process_test_marker(
                &marker,
                &format!(
                    "{} {}\n",
                    std::process::id(),
                    listener.local_addr().unwrap().port()
                ),
            );
            while !stop.exists() {
                thread::sleep(Duration::from_millis(10));
            }
            return;
        }
        Some("wrapper") => {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", TEST_NAME, "--nocapture"])
                .env(ROLE_ENV, "listener")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            configure_app_child_process_group(&mut command);
            let status = command.spawn().unwrap().wait().unwrap();
            assert!(
                status.success(),
                "detached listener helper failed: {status}"
            );
            return;
        }
        Some(role) => panic!("unexpected detached-listener helper role {role:?}"),
        None => {}
    }

    let _guard = termination_test_guard();
    let temp = tempdir().unwrap();
    let marker_path = temp.path().join("listener.marker");
    let stop_path = temp.path().join("stop-listener");
    let detached_stop = TestStopFile(stop_path.clone());
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", TEST_NAME, "--nocapture"])
        .env(ROLE_ENV, "wrapper")
        .env(MARKER_ENV, &marker_path)
        .env(STOP_ENV, &stop_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_app_child_process_group(&mut command);
    let mut child = command.spawn().unwrap();
    let marker =
        wait_for_process_test_marker(&marker_path, &mut child, "detached listener wrapper");
    let mut marker_fields = marker.split_whitespace();
    let detached_pid = marker_fields.next().unwrap().parse::<u32>().unwrap();
    let port = marker_fields.next().unwrap().parse::<u16>().unwrap();
    assert!(
        marker_fields.next().is_none(),
        "unexpected marker {marker:?}"
    );
    let detached_identity =
        crate::state::process_start_token(detached_pid).map(|token| (detached_pid, token));

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
    detached_stop.stop();
    terminate_child(&mut child).unwrap();
    let _ = child.wait();
    if let Some((pid, token)) = detached_identity {
        let deadline = Instant::now() + Duration::from_secs(2);
        while crate::state::pid_is_alive(pid)
            && crate::state::process_start_token(pid).as_deref() == Some(token.as_str())
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !crate::state::pid_is_alive(pid)
                || crate::state::process_start_token(pid).as_deref() != Some(token.as_str()),
            "detached listener did not stop cooperatively"
        );
    }
    drop(detached_stop);
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

    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let error =
        ensure_proxy_running_interruptible(&store, &settings, Path::new("unused-jig"), &|| false)
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

    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let error =
        ensure_proxy_running_interruptible(&store, &settings, Path::new("unused-jig"), &|| false)
            .unwrap_err()
            .to_string();
    handle.join().unwrap();

    assert!(error.contains("cannot authenticate"));
    assert!(error.contains(temp.path().to_string_lossy().as_ref()));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn direct_proxy_start_sigint_reaps_unready_background_process_group() {
    const HELPER_ENV: &str = "JIG_PROXY_START_SIGNAL_HELPER";
    const SCRIPT_ENV: &str = "JIG_PROXY_START_SIGNAL_SCRIPT";
    const STATE_ENV: &str = "JIG_PROXY_START_SIGNAL_STATE";
    const TEST_NAME: &str =
        "processes::tests::direct_proxy_start_sigint_reaps_unready_background_process_group";

    if std::env::var_os(HELPER_ENV).is_some() {
        let settings = ProxySettings {
            state_dir: Some(PathBuf::from(
                std::env::var_os(STATE_ENV).expect("helper state dir"),
            )),
            http_port: 0,
            ..ProxySettings::default()
        };
        let script = PathBuf::from(std::env::var_os(SCRIPT_ENV).expect("helper script path"));
        let error = ensure_proxy_running(&settings, &script)
            .expect_err("sleeping proxy helper must be interrupted");
        let reason = interruption_reason(&error).expect("SIGINT remains the primary outcome");
        std::process::exit(reason.exit_status());
    }

    let temp = tempdir().unwrap();
    let script = temp.path().join("sleeping-proxy");
    let marker = temp.path().join("sleeping-proxy.marker");
    fs::write(
        &script,
        "#!/bin/sh\ntrap '' HUP INT TERM\nsleep 60 &\nchild=$!\nmarker=\"$0.marker\"\ntemporary=\"$marker.tmp.$$\"\nprintf '%s %s\\n' \"$$\" \"$child\" > \"$temporary\"\n/bin/mv \"$temporary\" \"$marker\"\nwait \"$child\"\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let state_dir = temp.path().join("state");

    let mut launcher = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST_NAME, "--nocapture"])
        .env(HELPER_ENV, "1")
        .env(SCRIPT_ENV, &script)
        .env(STATE_ENV, &state_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let marker_deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() && Instant::now() < marker_deadline {
        if let Some(status) = launcher.try_wait().unwrap() {
            panic!("direct proxy-start helper exited before spawn marker: {status}");
        }
        thread::sleep(Duration::from_millis(10));
    }
    if !marker.exists() {
        let _ = launcher.kill();
        let _ = launcher.wait();
        panic!("direct proxy-start helper did not spawn its background child");
    }

    let marker = fs::read_to_string(&marker).unwrap();
    let mut fields = marker.split_whitespace();
    let leader_pid = fields.next().unwrap().parse::<u32>().unwrap();
    let descendant_pid = fields.next().unwrap().parse::<u32>().unwrap();
    assert!(
        fields.next().is_none(),
        "unexpected proxy marker: {marker:?}"
    );
    let leader_token = crate::state::process_start_token(leader_pid).unwrap();
    let descendant_token = crate::state::process_start_token(descendant_pid).unwrap();
    let identity_is_alive = |pid, token: &str| {
        crate::state::pid_is_alive(pid)
            && crate::state::process_start_token(pid).as_deref() == Some(token)
    };
    assert!(identity_is_alive(leader_pid, &leader_token));
    assert!(identity_is_alive(descendant_pid, &descendant_token));

    assert_eq!(
        unsafe { libc::kill(launcher.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let exit_deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = launcher.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= exit_deadline {
            break None;
        }
        thread::sleep(Duration::from_millis(10));
    };
    if status.is_none() {
        let _ = launcher.kill();
        let _ = launcher.wait();
    }

    let cleanup_deadline = Instant::now() + Duration::from_secs(5);
    while (identity_is_alive(leader_pid, &leader_token)
        || identity_is_alive(descendant_pid, &descendant_token))
        && Instant::now() < cleanup_deadline
    {
        thread::sleep(Duration::from_millis(10));
    }
    let leader_survived = identity_is_alive(leader_pid, &leader_token);
    let descendant_survived = identity_is_alive(descendant_pid, &descendant_token);
    if leader_survived {
        let _ = unsafe { libc::kill(-(leader_pid as libc::pid_t), libc::SIGKILL) };
    } else if descendant_survived {
        let _ = unsafe { libc::kill(descendant_pid as libc::pid_t, libc::SIGKILL) };
    }

    assert_eq!(status.and_then(|status| status.code()), Some(130));
    assert!(
        !leader_survived,
        "unfinished proxy launcher survived SIGINT"
    );
    assert!(
        !descendant_survived,
        "unfinished proxy launcher descendant survived SIGINT"
    );
}
