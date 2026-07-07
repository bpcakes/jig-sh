use std::thread;

use tempfile::tempdir;

use super::*;

fn write_private_routes_fixture(store: &StateStore, contents: impl AsRef<[u8]>) {
    fs::write(store.routes_path(), contents).unwrap();
    #[cfg(unix)]
    fs::set_permissions(store.routes_path(), fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn add_replaces_existing_route() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    store
        .add_route(Route {
            hostname: "web.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        })
        .unwrap();
    store
        .add_route(Route {
            hostname: "web.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4001,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        })
        .unwrap();
    let routes = store.read_routes(false).unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].target_port, 4001);
    let text = fs::read_to_string(store.routes_path()).unwrap();
    assert!(text.contains(r#""version": 1"#));
    assert!(text.contains(r#""routes""#));
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(store.routes_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(store.lock_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

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
            || {
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

#[test]
fn routes_are_lowercased_on_write() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    store
        .add_route(Route {
            hostname: "Web.LocalHost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        })
        .unwrap();

    let routes = store.read_routes(false).unwrap();
    let text = fs::read_to_string(store.routes_path()).unwrap();

    assert_eq!(routes[0].hostname, "web.localhost");
    assert!(text.contains(r#""hostname": "web.localhost""#));
}

#[test]
fn concurrent_add_route_keeps_all_routes() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let handles = (0..16)
        .map(|index| {
            let store = store.clone();
            thread::spawn(move || {
                store
                    .add_alias_route(Route {
                        hostname: format!("app-{index}.localhost").into(),
                        target_host: "127.0.0.1".into(),
                        target_port: 4000 + index,
                        owner_pid: None,
                        owner_start_token: None,
                        mode: RouteMode::Alias,
                        created_at_ms: now_ms(),
                    })
                    .unwrap();
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        handle.join().unwrap();
    }

    let routes = store.read_routes(false).unwrap();
    assert_eq!(routes.len(), 16);
    for index in 0..16 {
        assert!(
            routes
                .iter()
                .any(|route| route.hostname == format!("app-{index}.localhost"))
        );
    }
}

#[test]
fn remove_route_matches_case_insensitively() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    store
        .add_route(Route {
            hostname: "web.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        })
        .unwrap();

    store.remove_route("Web.LocalHost").unwrap();

    assert!(store.read_routes(false).unwrap().is_empty());
}

#[test]
fn legacy_route_arrays_remain_readable() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    write_private_routes_fixture(
        &store,
        serde_json::to_string(&vec![Route {
            hostname: "web.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        }])
        .unwrap(),
    );

    let routes = store.read_routes(false).unwrap();

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].hostname, "web.localhost");
}

#[test]
fn invalid_route_file_returns_error() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    write_private_routes_fixture(&store, "{not json");

    let error = store.read_routes(false).unwrap_err().to_string();
    assert!(error.contains("Failed to parse Jig proxy routes"));
}

#[test]
fn oversized_route_file_returns_error() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    write_private_routes_fixture(&store, vec![b' '; (MAX_ROUTES_FILE_BYTES + 1) as usize]);

    let error = store.read_routes(false).unwrap_err().to_string();

    assert!(error.contains("above the"));
}

#[test]
fn invalid_route_entries_return_error() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    write_private_routes_fixture(
        &store,
        r#"{"version":1,"routes":[{"hostname":"bad,host","target_host":"127.0.0.1","target_port":4000,"owner_pid":null,"mode":"alias","created_at_ms":1}]}"#,
    );

    let error = format!("{:#}", store.read_routes(false).unwrap_err());
    assert!(error.contains("Failed to parse Jig proxy routes"));
}

#[test]
fn process_route_reads_require_owner_identity() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    write_private_routes_fixture(
        &store,
        r#"{"version":1,"routes":[{"hostname":"web.localhost","target_host":"127.0.0.1","target_port":4000,"owner_pid":null,"owner_start_token":null,"mode":"process","created_at_ms":1}]}"#,
    );

    let error = store.read_routes(false).unwrap_err().to_string();

    assert!(error.contains("owner PID and start token"));
}

#[test]
fn route_files_ignore_unknown_top_level_fields() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    write_private_routes_fixture(
        &store,
        r#"{"version":1,"routes":[{"hostname":"web.localhost","target_host":"127.0.0.1","target_port":4000,"owner_pid":null,"mode":"alias","created_at_ms":1}],"unexpected":true}"#,
    );

    let routes = store.read_routes(false).unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].hostname, "web.localhost");
}

#[test]
fn route_files_ignore_unknown_route_fields() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    write_private_routes_fixture(
        &store,
        r#"{"version":1,"routes":[{"hostname":"web.localhost","target_host":"127.0.0.1","target_port":4000,"owner_pid":null,"mode":"alias","created_at_ms":1,"unexpected":true}]}"#,
    );

    let routes = store.read_routes(false).unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].hostname, "web.localhost");
}

#[test]
fn reading_missing_routes_file_does_not_create_it() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

    let routes = store.read_routes(false).unwrap();

    assert!(routes.is_empty());
    assert!(!store.routes_path().exists());
}

#[cfg(unix)]
#[test]
fn route_reads_reject_symlink_file() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let route_file = temp.path().join("routes.json");
    let outside = temp.path().join("outside-routes.json");
    fs::write(&outside, "[]").unwrap();
    symlink(&outside, &route_file).unwrap();

    assert!(read_routes_from_path(&route_file).is_err());
}

#[cfg(unix)]
#[test]
fn route_reads_reject_loose_permissions() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    fs::write(store.routes_path(), "[]").unwrap();
    fs::set_permissions(store.routes_path(), fs::Permissions::from_mode(0o644)).unwrap();

    let error = store.read_routes(false).unwrap_err().to_string();

    assert!(error.contains("must have mode 600"));
}

#[cfg(unix)]
#[test]
fn private_state_reads_reject_symlink_file() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let target = temp.path().join("outside-token");
    let link = temp.path().join("proxy-health-token");
    fs::write(
        &target,
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    )
    .unwrap();
    symlink(&target, &link).unwrap();

    assert!(read_health_token_file(&link).is_err());
    assert_eq!(read_port_file(&link), None);
}

#[test]
fn invalid_pid_file_is_reported() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    fs::write(store.pid_path(), "not-a-pid").unwrap();

    let error = store.read_pid().unwrap_err().to_string();

    assert!(error.contains("Invalid Jig proxy PID file"));
}

#[test]
fn health_token_is_private_and_reused_until_runtime_clear() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

    let first = store.ensure_health_token().unwrap();
    let second = store.ensure_health_token().unwrap();

    assert_eq!(first, second);
    assert_eq!(first.len(), 64);
    assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(store.read_health_token().unwrap(), Some(first));
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(store.health_token_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    store.clear_runtime_files();

    assert_eq!(store.read_health_token().unwrap(), None);
}

#[cfg(unix)]
#[test]
fn health_token_reads_reject_loose_permissions() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let token = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    fs::write(store.health_token_path(), token).unwrap();
    fs::set_permissions(store.health_token_path(), fs::Permissions::from_mode(0o644)).unwrap();

    let error = store.read_health_token().unwrap_err().to_string();

    assert!(error.contains("must have mode 600"));
}

#[test]
fn replace_runtime_files_rewrites_state_under_one_runtime_lock() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

    fs::write(store.https_port_path(), "1443").unwrap();
    let token = store
        .replace_runtime_files(Path::new("/tmp/jig"), 1355, None)
        .unwrap();

    assert_eq!(store.read_health_token().unwrap(), Some(token));
    assert_eq!(store.read_pid().unwrap(), Some(std::process::id()));
    assert_eq!(
        fs::read_to_string(store.proxy_exe_path()).unwrap(),
        "/tmp/jig"
    );
    assert_eq!(store.read_http_port().unwrap(), Some(1355));
    assert_eq!(store.read_https_port().unwrap(), None);
}

#[test]
fn windows_tasklist_csv_pid_reads_second_field_only() {
    assert_eq!(
        windows_tasklist_csv_pid(r#""jig.exe","1234","Console","1","10,000 K""#),
        Some(1234)
    );
    assert_eq!(
        windows_tasklist_csv_pid(r#""bad "",""1234"", suffix","9999","Console","1","1 K""#),
        Some(9999)
    );
}

#[cfg(unix)]
#[test]
fn state_dir_is_owner_only() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("state");

    let store = StateStore::resolve(Some(state_dir)).unwrap();
    let mode = fs::metadata(store.root()).unwrap().permissions().mode() & 0o777;

    assert_eq!(mode, 0o700);
}

#[cfg(unix)]
#[test]
fn existing_explicit_state_dir_must_already_be_private() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("state");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("marker"), "not-empty").unwrap();
    fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o755)).unwrap();

    let error = StateStore::resolve(Some(state_dir.clone()))
        .unwrap_err()
        .to_string();
    let mode = fs::metadata(&state_dir).unwrap().permissions().mode() & 0o777;

    assert!(error.contains("already exists with permissions"));
    assert_eq!(mode, 0o755);
}

#[cfg(unix)]
#[test]
fn existing_explicit_state_dir_must_be_writable_and_searchable() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("state");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("marker"), "not-empty").unwrap();
    fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o500)).unwrap();

    let error = StateStore::resolve(Some(state_dir.clone()))
        .unwrap_err()
        .to_string();

    fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(error.contains("mode 700"));
}

#[cfg(unix)]
#[test]
fn missing_state_dir_rejects_shared_writable_creation_ancestor() {
    let temp = tempdir().unwrap();
    let parent = temp.path().join("shared");
    let state_dir = parent.join("state");
    fs::create_dir_all(&parent).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o777)).unwrap();

    let error = StateStore::resolve(Some(state_dir))
        .unwrap_err()
        .to_string();

    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(error.contains("shared-writable ancestor"));
}

#[cfg(unix)]
#[test]
fn existing_default_state_parent_must_already_be_private() {
    let temp = tempdir().unwrap();
    let parent = temp.path().join(".jig");
    let state_dir = parent.join("proxy");
    fs::create_dir_all(&state_dir).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();

    let error = ensure_default_state_parent_permissions(&state_dir, true)
        .unwrap_err()
        .to_string();
    let mode = fs::metadata(&parent).unwrap().permissions().mode() & 0o777;

    assert!(error.contains("Default proxy state parent"));
    assert_eq!(mode, 0o755);
}

#[cfg(unix)]
#[test]
fn state_dir_rejects_symlinked_entries() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("state");
    fs::create_dir_all(&state_dir).unwrap();
    fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
    symlink(temp.path().join("target"), state_dir.join("proxy.pid")).unwrap();

    let error = StateStore::resolve(Some(state_dir))
        .unwrap_err()
        .to_string();

    assert!(error.contains("contains symlink"));
}

#[cfg(unix)]
#[test]
fn state_dir_rejects_nested_symlinked_entries() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let nested = state_dir.join("nested");
    fs::create_dir_all(&nested).unwrap();
    fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
    symlink(temp.path().join("target"), nested.join("proxy.pid")).unwrap();

    let error = StateStore::resolve(Some(state_dir))
        .unwrap_err()
        .to_string();

    assert!(error.contains("contains symlink"));
}

#[test]
fn read_proxy_exe_reports_missing_path() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    store
        .write_proxy_exe(&temp.path().join("missing-jig"))
        .unwrap();

    let status = store.read_proxy_exe_status().unwrap();
    assert_eq!(status.path, None);
    assert!(
        status
            .warning
            .as_deref()
            .is_some_and(|warning| warning.contains("not available"))
    );
}

#[test]
fn resolve_recovers_interrupted_replace_backup() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("state");
    fs::create_dir_all(&state_dir).unwrap();
    #[cfg(unix)]
    fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let backup = state_dir.join("routes.json.4294967295.123456.7.replace-backup");
    fs::write(&backup, "[]").unwrap();

    let store = StateStore::resolve(Some(state_dir)).unwrap();

    assert!(!backup.exists());
    assert_eq!(fs::read_to_string(store.routes_path()).unwrap(), "[]");
}

#[test]
fn replace_backup_detection_matches_state_file_name() {
    let temp = tempdir().unwrap();
    let routes = temp.path().join("routes.json");
    let ports = temp.path().join("proxy-port");
    fs::write(
        temp.path().join("routes.json.42.123456.7.replace-backup"),
        "[]",
    )
    .unwrap();
    fs::write(
        temp.path().join("routes.json.not-a-pid.replace-backup"),
        "[]",
    )
    .unwrap();

    assert!(file_ops::replace_backup_for_path_exists(&routes));
    assert!(!file_ops::replace_backup_for_path_exists(&ports));
    assert_eq!(
        file_ops::replace_backup_parts("routes.json.42.123456.7.replace-backup"),
        Some(("routes.json", "42"))
    );
    assert_eq!(
        file_ops::replace_backup_parts("routes.json.not-a-pid.replace-backup"),
        None
    );
}

#[test]
fn missing_route_file_with_replace_backup_fails_closed() {
    let temp = tempdir().unwrap();
    let routes = temp.path().join("routes.json");
    fs::write(
        temp.path().join("routes.json.42.123456.7.replace-backup"),
        "[]",
    )
    .unwrap();

    let error = missing_file_read_result(&routes, true).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    assert!(error.to_string().contains("temporarily unavailable"));
}

#[test]
fn backup_promotion_requires_start_token_support() {
    if process_start_tokens_supported() {
        return;
    }
    let temp = tempdir().unwrap();
    let backup = temp.path().join("routes.json.4294967295.replace-backup");
    fs::write(&backup, "[]").unwrap();

    assert!(!replace_backup_can_be_promoted(&backup, "4294967295"));
}

#[test]
fn cert_lock_inside_route_lock_returns_error() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

    let error = store
        .with_route_lock(|_| store.with_cert_lock(|| Ok(())))
        .unwrap_err()
        .to_string();

    assert!(error.contains("cert lock cannot be acquired"));
    assert!(
        !store.root().join(CERT_LOCK_FILE).exists(),
        "route-held cert-lock attempts must fail before opening the cert lock"
    );
}

#[cfg(unix)]
#[test]
fn state_dir_rejects_symlink_root() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let target = temp.path().join("target");
    fs::create_dir_all(&target).unwrap();
    let link = temp.path().join("state-link");
    symlink(&target, &link).unwrap();

    let error = StateStore::resolve(Some(link)).unwrap_err().to_string();

    assert!(error.contains("must not be a symlink"));
}

#[cfg(unix)]
#[test]
fn state_dir_canonicalizes_symlink_ancestor() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let target = temp.path().join("target");
    fs::create_dir_all(&target).unwrap();
    let link = temp.path().join("state-parent-link");
    symlink(&target, &link).unwrap();

    let store = StateStore::resolve(Some(link.join("state"))).unwrap();

    assert!(store.root().starts_with(fs::canonicalize(target).unwrap()));
}

#[test]
fn file_signature_changes_for_same_size_rewrites() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("routes.json");
    fs::write(&path, "aa").unwrap();
    let first = file_signature(&path).unwrap();

    fs::write(&path, "bb").unwrap();

    assert_ne!(file_signature(&path).unwrap(), first);
}

#[test]
fn prune_skips_rewrite_when_routes_are_unchanged() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    store
        .add_route(Route {
            hostname: "web.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        })
        .unwrap();
    let before = store.routes_signature();

    store.prune().unwrap();

    assert_eq!(store.routes_signature(), before);
}

#[test]
fn process_route_with_mismatched_start_token_is_dead() {
    let route = Route {
        hostname: "web.localhost".into(),
        target_host: "127.0.0.1".into(),
        target_port: 4000,
        owner_pid: Some(std::process::id()),
        owner_start_token: Some("not-this-process".into()),
        mode: RouteMode::Process,
        created_at_ms: now_ms(),
    };

    assert!(!route_is_alive(&route));
}

#[test]
fn process_route_without_start_token_is_dead() {
    let route = Route {
        hostname: "web.localhost".into(),
        target_host: "127.0.0.1".into(),
        target_port: 4000,
        owner_pid: Some(std::process::id()),
        owner_start_token: None,
        mode: RouteMode::Process,
        created_at_ms: now_ms(),
    };

    assert!(!route_is_alive(&route));
}

#[test]
fn process_routes_are_rejected_without_start_token_support() {
    if process_start_tokens_supported() {
        return;
    }
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

    let error = store
        .add_route(Route {
            hostname: "web.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: Some(std::process::id()),
            owner_start_token: None,
            mode: RouteMode::Process,
            created_at_ms: now_ms(),
        })
        .unwrap_err()
        .to_string();

    assert!(error.contains("Process routes require process start-token verification"));
}

#[test]
fn add_route_refuses_to_replace_live_process_route() {
    if !process_start_tokens_supported() {
        return;
    }
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    store
        .add_route(Route {
            hostname: "web.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: Some(std::process::id()),
            owner_start_token: process_start_token(std::process::id()),
            mode: RouteMode::Process,
            created_at_ms: now_ms(),
        })
        .unwrap();

    let error = store
        .add_route(Route {
            hostname: "web.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4001,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        })
        .unwrap_err()
        .to_string();

    assert!(error.contains("would replace a live process route"));
    assert!(error.contains(&std::process::id().to_string()));
    assert!(error.contains("127.0.0.1:4000"));
    assert!(error.contains("jig proxy prune"));
}

#[test]
fn preflight_refuses_live_process_route() {
    if !process_start_tokens_supported() {
        return;
    }
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    store
        .add_route(Route {
            hostname: "web.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: Some(std::process::id()),
            owner_start_token: process_start_token(std::process::id()),
            mode: RouteMode::Process,
            created_at_ms: now_ms(),
        })
        .unwrap();

    let error = store
        .ensure_no_live_process_routes_for_hostnames(["web.localhost"])
        .unwrap_err()
        .to_string();

    assert!(error.contains("would replace a live process route"));
    assert!(error.contains(&std::process::id().to_string()));
    assert!(error.contains("127.0.0.1:4000"));
}

#[test]
fn add_alias_route_requires_alias_mode() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

    let error = store
        .add_alias_route(Route {
            hostname: "web.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Process,
            created_at_ms: now_ms(),
        })
        .unwrap_err()
        .to_string();

    assert!(error.contains("requires RouteMode::Alias"));
}

#[test]
fn add_alias_route_rejects_public_suffix_hostname() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

    let error = store
        .add_alias_route(Route {
            hostname: crate::host::RouteHostname::unchecked("api.example.com"),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        })
        .unwrap_err()
        .to_string();

    assert!(error.contains("private/local suffix"));
}

#[test]
fn add_process_route_requires_owner_identity() {
    if !process_start_tokens_supported() {
        return;
    }
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

    let error = store
        .add_route(Route {
            hostname: "web.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: Some(std::process::id()),
            owner_start_token: None,
            mode: RouteMode::Process,
            created_at_ms: now_ms(),
        })
        .unwrap_err()
        .to_string();

    assert!(error.contains("owner PID and start token"));
}
