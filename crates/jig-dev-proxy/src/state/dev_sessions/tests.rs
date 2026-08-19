use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt, symlink};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::test_tempdir as tempdir;
use fs4::fs_std::FileExt;

use super::*;
use crate::state::{StateStore, now_ms, open_lock_file};
use crate::types::{Route, RouteMode};

fn session(session_id: &str) -> DevSessionRecord {
    let timestamp = now_ms();
    DevSessionRecord {
        session_id: session_id.into(),
        repo_name: "demo".into(),
        repo_root_display: "/tmp/demo".into(),
        repo_root_identity: "canonical:/tmp/demo".into(),
        phase: DevSessionPhase::Starting,
        started_at_ms: timestamp,
        updated_at_ms: timestamp,
        cleanup_required: true,
        preflight_cleanup_pending: false,
        supervisor: DevProcessIdentity {
            pid: std::process::id(),
            start_token: Some("test-supervisor".into()),
        },
        control: DevSessionControl {
            port: 45_123,
            token: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        },
        apps: vec![
            DevSessionApp {
                name: "web".into(),
                hostname: Some("web.demo.localhost".into()),
                target_host: "127.0.0.1".into(),
                target_port: None,
                spawn_state_tracked: true,
                spawn_pending: false,
                process: None,
            },
            DevSessionApp {
                name: "worker".into(),
                hostname: None,
                target_host: "::1".into(),
                target_port: Some(4100),
                spawn_state_tracked: true,
                spawn_pending: false,
                process: Some(DevProcessIdentity {
                    pid: std::process::id(),
                    start_token: None,
                }),
            },
        ],
    }
}

fn write_private_fixture(path: &Path, contents: impl AsRef<[u8]>) {
    fs::write(path, contents).unwrap();
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn missing_file_is_an_empty_snapshot_without_creating_state() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

    let snapshot = store.snapshot_dev_state().unwrap();

    assert!(snapshot.sessions.is_empty());
    assert!(snapshot.routes.is_empty());
    assert!(!store.dev_sessions_path().exists());
}

#[test]
fn resolve_existing_does_not_create_missing_state() {
    let temp = tempdir().unwrap();
    let missing = temp.path().join("missing-proxy-state");

    let resolved = StateStore::resolve_existing(Some(missing.clone())).unwrap();

    assert!(resolved.is_none());
    assert!(!missing.exists());
}

#[test]
fn resolve_existing_applies_normal_validation_to_present_state() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("proxy-state");
    let created = StateStore::resolve(Some(state_dir.clone())).unwrap();
    let expected = session("dev_existing");
    created
        .mutate_dev_sessions(|sessions, _| {
            sessions.push(expected.clone());
            Ok(())
        })
        .unwrap();

    let existing = StateStore::resolve_existing(Some(state_dir))
        .unwrap()
        .unwrap();

    assert_eq!(
        existing.snapshot_dev_state().unwrap().sessions,
        vec![expected]
    );
}

#[test]
fn resolve_existing_recovers_a_session_replace_backup_under_the_shared_lock() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("proxy-state");
    fs::create_dir_all(&state_dir).unwrap();
    #[cfg(unix)]
    fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let expected = session("dev_recovered");
    let backup = state_dir.join("dev-sessions.json.4294967295.123456.7.replace-backup");
    let document = serde_json::to_vec_pretty(&DevSessionsDocument {
        version: VERSION,
        sessions: std::slice::from_ref(&expected),
    })
    .unwrap();
    write_private_fixture(&backup, document);

    let store = StateStore::resolve_existing(Some(state_dir))
        .unwrap()
        .unwrap();

    assert!(!backup.exists());
    assert_eq!(store.snapshot_dev_state().unwrap().sessions, vec![expected]);
}

#[test]
fn mutation_writes_versioned_private_state_and_round_trips() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let expected = session("dev_one");

    store
        .mutate_dev_sessions(|sessions, routes| {
            assert!(routes.is_empty());
            sessions.push(expected.clone());
            Ok(())
        })
        .unwrap();

    let snapshot = store.snapshot_dev_state().unwrap();
    assert_eq!(snapshot.sessions, vec![expected]);
    let contents = fs::read_to_string(store.dev_sessions_path()).unwrap();
    assert!(contents.contains(r#""version": 1"#));
    assert!(contents.contains(r#""sessions""#));
    assert!(contents.contains(r#""phase": "starting""#));
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(store.dev_sessions_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn records_without_preflight_cleanup_state_remain_compatible() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let mut document = serde_json::to_value(DevSessionsDocument {
        version: VERSION,
        sessions: &[session("dev_legacy_preflight")],
    })
    .unwrap();
    document["sessions"][0]
        .as_object_mut()
        .unwrap()
        .remove("preflight_cleanup_pending");
    write_private_fixture(
        &store.dev_sessions_path(),
        serde_json::to_vec_pretty(&document).unwrap(),
    );

    let persisted = store.snapshot_dev_state().unwrap();

    assert_eq!(persisted.sessions.len(), 1);
    assert!(!persisted.sessions[0].preflight_cleanup_pending);
}

#[test]
fn debug_output_redacts_control_tokens() {
    let record = session("dev_redacted");
    let rendered = format!("{record:?}");

    assert!(rendered.contains("<redacted>"));
    assert!(!rendered.contains(&record.control.token));
}

#[test]
fn mutation_observes_routes_under_the_shared_lock() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    store
        .add_route(Route {
            hostname: "web.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        })
        .unwrap();

    store
        .mutate_dev_sessions(|sessions, routes| {
            assert_eq!(routes.len(), 1);
            assert_eq!(routes[0].hostname, "web.demo.localhost");
            sessions.push(session("dev_routes"));
            Ok(())
        })
        .unwrap();

    assert_eq!(store.snapshot_dev_state().unwrap().routes.len(), 1);
}

#[test]
fn unchanged_and_failed_mutations_do_not_rewrite_state() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    store
        .mutate_dev_sessions(|sessions, _| {
            sessions.push(session("dev_unchanged"));
            Ok(())
        })
        .unwrap();
    let before = fs::read(store.dev_sessions_path()).unwrap();

    store
        .mutate_dev_sessions(|_, _| Ok::<_, anyhow::Error>(()))
        .unwrap();
    let error = store
        .mutate_dev_sessions::<()>(|sessions, _| {
            sessions.clear();
            anyhow::bail!("injected mutation failure")
        })
        .unwrap_err();

    assert!(error.to_string().contains("injected mutation failure"));
    assert_eq!(fs::read(store.dev_sessions_path()).unwrap(), before);
}

#[test]
fn duplicate_session_ids_are_rejected_without_replacing_existing_state() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    store
        .mutate_dev_sessions(|sessions, _| {
            sessions.push(session("dev_duplicate"));
            Ok(())
        })
        .unwrap();
    let before = fs::read(store.dev_sessions_path()).unwrap();

    let error = store
        .mutate_dev_sessions(|sessions, _| {
            sessions.push(session("dev_duplicate"));
            Ok(())
        })
        .unwrap_err();

    assert!(error.to_string().contains("duplicate session id"));
    assert_eq!(fs::read(store.dev_sessions_path()).unwrap(), before);
}

#[test]
fn unknown_version_and_malformed_state_are_rejected() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    write_private_fixture(&store.dev_sessions_path(), r#"{"version":2,"sessions":[]}"#);
    let version_error = store.snapshot_dev_state().unwrap_err().to_string();
    assert!(version_error.contains("Unsupported Jig development sessions version 2"));

    write_private_fixture(&store.dev_sessions_path(), "{not json");
    let parse_error = format!("{:#}", store.snapshot_dev_state().unwrap_err());
    assert!(parse_error.contains("Failed to parse Jig development sessions"));

    write_private_fixture(&store.dev_sessions_path(), " \n");
    let empty_error = store.snapshot_dev_state().unwrap_err().to_string();
    assert!(empty_error.contains("is empty"));
}

#[test]
fn oversized_state_is_rejected() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    write_private_fixture(
        &store.dev_sessions_path(),
        vec![b' '; (MAX_DEV_SESSIONS_FILE_BYTES + 1) as usize],
    );

    let error = store.snapshot_dev_state().unwrap_err().to_string();

    assert!(error.contains("above the"));
}

#[test]
fn record_validation_rejects_invalid_identity_control_and_app_data() {
    let mut invalid_pid = session("invalid_pid");
    invalid_pid.supervisor.pid = 0;
    assert!(
        validate_records(&[invalid_pid])
            .unwrap_err()
            .to_string()
            .contains("process id 0")
    );

    let mut invalid_token = session("invalid_token");
    invalid_token.control.token = "not-a-token".into();
    assert!(
        validate_records(&[invalid_token])
            .unwrap_err()
            .to_string()
            .contains("64 hexadecimal")
    );

    let mut invalid_hostname = session("invalid_hostname");
    invalid_hostname.apps[0].hostname = Some("bad,host".into());
    assert!(validate_records(&[invalid_hostname]).is_err());

    let mut invalid_target = session("invalid_target");
    invalid_target.apps[0].target_host = "localhost".into();
    assert!(
        format!("{:#}", validate_records(&[invalid_target]).unwrap_err())
            .contains("invalid target host")
    );

    let mut direct_localhost = session("direct_localhost");
    direct_localhost.apps[1].target_host = "localhost".into();
    validate_records(&[direct_localhost]).unwrap();

    let mut invalid_direct_target = session("invalid_direct_target");
    invalid_direct_target.apps[1].target_host.clear();
    assert!(
        validate_records(&[invalid_direct_target])
            .unwrap_err()
            .to_string()
            .contains("target host must not be empty")
    );

    let mut invalid_port = session("invalid_port");
    invalid_port.apps[0].target_port = Some(0);
    assert!(
        validate_records(&[invalid_port])
            .unwrap_err()
            .to_string()
            .contains("target port 0")
    );

    let mut missing_cleanup = session("missing_cleanup");
    missing_cleanup.cleanup_required = false;
    assert!(
        validate_records(&[missing_cleanup])
            .unwrap_err()
            .to_string()
            .contains("without requiring cleanup")
    );

    let mut pending_without_cleanup = session("pending_without_cleanup");
    pending_without_cleanup.cleanup_required = false;
    pending_without_cleanup.apps[0].target_port = Some(4005);
    pending_without_cleanup.apps[0].spawn_pending = true;
    assert!(
        validate_records(&[pending_without_cleanup])
            .unwrap_err()
            .to_string()
            .contains("pending spawn without requiring cleanup")
    );

    let mut preflight_without_cleanup = session("preflight_without_cleanup");
    preflight_without_cleanup.cleanup_required = false;
    preflight_without_cleanup.preflight_cleanup_pending = true;
    assert!(
        validate_records(&[preflight_without_cleanup])
            .unwrap_err()
            .to_string()
            .contains("pending preflight cleanup without requiring cleanup")
    );

    let mut pending_without_port = session("pending_without_port");
    pending_without_port.apps[0].spawn_pending = true;
    assert!(
        validate_records(&[pending_without_port])
            .unwrap_err()
            .to_string()
            .contains("pending spawn without a target port")
    );

    let mut pending_without_tracking = session("pending_without_tracking");
    pending_without_tracking.apps[0].target_port = Some(4005);
    pending_without_tracking.apps[0].spawn_state_tracked = false;
    pending_without_tracking.apps[0].spawn_pending = true;
    assert!(
        validate_records(&[pending_without_tracking])
            .unwrap_err()
            .to_string()
            .contains("pending spawn without tracked spawn state")
    );

    let mut pending_with_process = session("pending_with_process");
    pending_with_process.apps[1].spawn_pending = true;
    assert!(
        validate_records(&[pending_with_process])
            .unwrap_err()
            .to_string()
            .contains("both a pending spawn and a process identity")
    );

    let mut invalid_time = session("invalid_time");
    invalid_time.updated_at_ms = invalid_time.started_at_ms.saturating_sub(1);
    if invalid_time.updated_at_ms < invalid_time.started_at_ms {
        assert!(
            validate_records(&[invalid_time])
                .unwrap_err()
                .to_string()
                .contains("updated before it started")
        );
    }

    let mut duplicate_app = session("duplicate_app");
    duplicate_app.apps[1].name = duplicate_app.apps[0].name.clone();
    assert!(
        validate_records(&[duplicate_app])
            .unwrap_err()
            .to_string()
            .contains("duplicate app name")
    );

    let invalid_session_id = session("contains spaces");
    let error = validate_records(&[invalid_session_id])
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        format!(
            "Jig development session id must be 1 to {MAX_SESSION_ID_BYTES} ASCII letters, digits, dots, dashes, or underscores"
        )
    );
}

#[test]
fn session_mutation_uses_the_existing_routes_lock() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let held_lock = open_lock_file(store.lock_path()).unwrap();
    held_lock.lock_exclusive().unwrap();
    let worker_store = store.clone();
    let (sender, receiver) = mpsc::channel();
    let (started_sender, started_receiver) = mpsc::channel();
    let worker = thread::spawn(move || {
        started_sender.send(()).unwrap();
        let result = worker_store.mutate_dev_sessions(|sessions, _| {
            sessions.push(session("dev_waited"));
            Ok(())
        });
        sender.send(result).unwrap();
    });

    started_receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert!(receiver.recv_timeout(Duration::from_millis(150)).is_err());
    FileExt::unlock(&held_lock).unwrap();
    receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    assert_eq!(store.snapshot_dev_state().unwrap().sessions.len(), 1);
}

#[cfg(unix)]
#[test]
fn session_reads_reject_symlink_non_regular_and_loose_files() {
    let symlink_temp = tempdir().unwrap();
    let symlink_store = StateStore::resolve(Some(symlink_temp.path().to_path_buf())).unwrap();
    let outside = symlink_temp.path().join("outside-sessions.json");
    write_private_fixture(&outside, r#"{"version":1,"sessions":[]}"#);
    symlink(&outside, symlink_store.dev_sessions_path()).unwrap();
    assert!(symlink_store.snapshot_dev_state().is_err());

    let directory_temp = tempdir().unwrap();
    let directory_store = StateStore::resolve(Some(directory_temp.path().to_path_buf())).unwrap();
    fs::create_dir(directory_store.dev_sessions_path()).unwrap();
    let non_regular = directory_store
        .snapshot_dev_state()
        .unwrap_err()
        .to_string();
    assert!(non_regular.contains("regular file"));

    let permissions_temp = tempdir().unwrap();
    let permissions_store =
        StateStore::resolve(Some(permissions_temp.path().to_path_buf())).unwrap();
    fs::write(
        permissions_store.dev_sessions_path(),
        r#"{"version":1,"sessions":[]}"#,
    )
    .unwrap();
    fs::set_permissions(
        permissions_store.dev_sessions_path(),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let permissions = permissions_store
        .snapshot_dev_state()
        .unwrap_err()
        .to_string();
    assert!(permissions.contains("must have mode 600"));
}
