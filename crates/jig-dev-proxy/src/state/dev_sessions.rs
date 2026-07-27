use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::file_ops;
use crate::host::{RouteHostname, TargetHost};
use crate::types::Route;

use super::{ensure_private_state_file_permissions, open_read_no_follow_maybe_missing};

const VERSION: u32 = 1;
pub(super) const FILE_NAME: &str = "dev-sessions.json";
const STATE_FILE_FALLBACK: &str = "jig-dev-sessions-state";
const MAX_SESSION_ID_BYTES: usize = 128;
const MAX_TEXT_BYTES: usize = 16 * 1024;
const MAX_DEV_SESSIONS_FILE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct DevProcessIdentity {
    pub(crate) pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) start_token: Option<String>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct DevSessionControl {
    pub(crate) port: u16,
    pub(crate) token: String,
}

impl std::fmt::Debug for DevSessionControl {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DevSessionControl")
            .field("port", &self.port)
            .field("token", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum DevSessionPhase {
    Starting,
    Running,
    Stopping,
    Orphaned,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct DevSessionApp {
    pub(crate) name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) hostname: Option<String>,
    pub(crate) target_host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) target_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) process: Option<DevProcessIdentity>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct DevSessionRecord {
    pub(crate) session_id: String,
    pub(crate) repo_name: String,
    pub(crate) repo_root_display: String,
    pub(crate) repo_root_identity: String,
    pub(crate) phase: DevSessionPhase,
    pub(crate) started_at_ms: u64,
    pub(crate) updated_at_ms: u64,
    #[serde(default)]
    pub(crate) cleanup_required: bool,
    pub(crate) supervisor: DevProcessIdentity,
    pub(crate) control: DevSessionControl,
    pub(crate) apps: Vec<DevSessionApp>,
}

#[derive(Clone, Debug)]
pub(crate) struct DevStateSnapshot {
    pub(crate) sessions: Vec<DevSessionRecord>,
    pub(crate) routes: Vec<Route>,
}

#[derive(Deserialize)]
struct DevSessionsDocumentOwned {
    version: u32,
    sessions: Vec<DevSessionRecord>,
}

#[derive(Serialize)]
struct DevSessionsDocument<'a> {
    version: u32,
    sessions: &'a [DevSessionRecord],
}

pub(super) fn read_from_path(path: &Path) -> Result<Vec<DevSessionRecord>> {
    let mut file = match open_read_no_follow_maybe_missing(path)? {
        Some(file) => file,
        None => return Ok(Vec::new()),
    };
    ensure_private_state_file_permissions(path, &file)?;
    file.seek(SeekFrom::Start(0))?;
    let len = file.metadata()?.len();
    if len > MAX_DEV_SESSIONS_FILE_BYTES {
        bail!(
            "Jig development sessions file is {len} bytes, above the {MAX_DEV_SESSIONS_FILE_BYTES} byte limit"
        );
    }
    let mut text = String::new();
    file.take(MAX_DEV_SESSIONS_FILE_BYTES.saturating_add(1))
        .read_to_string(&mut text)?;
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > MAX_DEV_SESSIONS_FILE_BYTES {
        bail!(
            "Jig development sessions file grew above the {MAX_DEV_SESSIONS_FILE_BYTES} byte limit while it was read"
        );
    }
    if text.trim().is_empty() {
        bail!("Jig development sessions file {} is empty", path.display());
    }
    let document = serde_json::from_str::<DevSessionsDocumentOwned>(&text)
        .context("Failed to parse Jig development sessions")?;
    if document.version != VERSION {
        bail!(
            "Unsupported Jig development sessions version {}",
            document.version
        );
    }
    validate_records(&document.sessions)?;
    Ok(document.sessions)
}

pub(super) fn write_to_path(path: &Path, sessions: &[DevSessionRecord]) -> Result<()> {
    validate_records(sessions)?;
    let document = serde_json::to_vec_pretty(&DevSessionsDocument {
        version: VERSION,
        sessions,
    })?;
    let document_len = u64::try_from(document.len()).unwrap_or(u64::MAX);
    let persisted_len = document_len.saturating_add(1);
    if persisted_len > MAX_DEV_SESSIONS_FILE_BYTES {
        bail!(
            "Jig development sessions document is {} bytes, above the {MAX_DEV_SESSIONS_FILE_BYTES} byte limit",
            persisted_len
        );
    }

    let tmp = file_ops::temp_path(path, STATE_FILE_FALLBACK);
    let mut file = file_ops::create_new_file(&tmp, 0o600)?;
    file.write_all(&document)?;
    file.write_all(b"\n")?;
    file.sync_data()?;
    drop(file);
    file_ops::replace_file(&tmp, path, STATE_FILE_FALLBACK)
}

pub(super) fn validate_records(sessions: &[DevSessionRecord]) -> Result<()> {
    let mut session_ids = HashSet::with_capacity(sessions.len());
    for session in sessions {
        validate_session_id(&session.session_id)?;
        if !session_ids.insert(session.session_id.as_str()) {
            bail!(
                "Jig development sessions contain duplicate session id '{}'",
                session.session_id
            );
        }
        validate_text("repository name", &session.repo_name)?;
        validate_text("repository root display", &session.repo_root_display)?;
        validate_text("repository root identity", &session.repo_root_identity)?;
        if session.updated_at_ms < session.started_at_ms {
            bail!(
                "Jig development session '{}' was updated before it started",
                session.session_id
            );
        }
        validate_process_identity(
            &format!("session '{}' supervisor", session.session_id),
            &session.supervisor,
        )?;
        validate_control(&session.session_id, &session.control)?;
        validate_apps(session)?;
    }
    Ok(())
}

fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.is_empty()
        || session_id.len() > MAX_SESSION_ID_BYTES
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        bail!(
            "Jig development session id must be 1 to {MAX_SESSION_ID_BYTES} ASCII letters, digits, dots, dashes, or underscores"
        );
    }
    Ok(())
}

fn validate_apps(session: &DevSessionRecord) -> Result<()> {
    let mut app_names = HashSet::with_capacity(session.apps.len());
    let mut hostnames = HashSet::with_capacity(session.apps.len());
    for app in &session.apps {
        validate_text("development app name", &app.name)?;
        if !app_names.insert(app.name.as_str()) {
            bail!(
                "Jig development session '{}' contains duplicate app name '{}'",
                session.session_id,
                app.name
            );
        }
        if let Some(hostname) = app.hostname.as_deref() {
            let normalized = RouteHostname::new(hostname)?;
            if normalized.as_str() != hostname {
                bail!(
                    "Jig development session '{}' app '{}' hostname '{}' is not canonical; expected '{}'",
                    session.session_id,
                    app.name,
                    hostname,
                    normalized
                );
            }
            if !hostnames.insert(hostname) {
                bail!(
                    "Jig development session '{}' contains duplicate hostname '{}'",
                    session.session_id,
                    hostname
                );
            }
            TargetHost::ip_literal(&app.target_host).with_context(|| {
                format!(
                    "Jig development session '{}' app '{}' has invalid target host",
                    session.session_id, app.name
                )
            })?;
        } else {
            validate_text("development app target host", &app.target_host)?;
        }
        if app.target_port == Some(0) {
            bail!(
                "Jig development session '{}' app '{}' has target port 0",
                session.session_id,
                app.name
            );
        }
        if let Some(process) = app.process.as_ref() {
            if !session.cleanup_required {
                bail!(
                    "Jig development session '{}' records app processes without requiring cleanup",
                    session.session_id
                );
            }
            validate_process_identity(
                &format!("session '{}' app '{}'", session.session_id, app.name),
                process,
            )?;
        }
    }
    Ok(())
}

fn validate_process_identity(label: &str, identity: &DevProcessIdentity) -> Result<()> {
    if identity.pid == 0 {
        bail!("Jig development {label} has process id 0");
    }
    if let Some(start_token) = identity.start_token.as_deref() {
        validate_text("process start token", start_token)?;
    }
    Ok(())
}

fn validate_control(session_id: &str, control: &DevSessionControl) -> Result<()> {
    if control.port == 0 {
        bail!("Jig development session '{session_id}' has control port 0");
    }
    if control.token.len() != 64 || !control.token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!(
            "Jig development session '{session_id}' control token must contain exactly 64 hexadecimal characters"
        );
    }
    Ok(())
}

fn validate_text(label: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("Jig development {label} must not be empty");
    }
    if value.len() > MAX_TEXT_BYTES {
        bail!(
            "Jig development {label} is {} bytes, above the {MAX_TEXT_BYTES} byte limit",
            value.len()
        );
    }
    if value.chars().any(char::is_control) {
        bail!("Jig development {label} contains control characters");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use fs4::fs_std::FileExt;
    use tempfile::tempdir;

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
                    process: None,
                },
                DevSessionApp {
                    name: "worker".into(),
                    hostname: None,
                    target_host: "::1".into(),
                    target_port: Some(4100),
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
        assert!(
            validate_records(&[invalid_session_id])
                .unwrap_err()
                .to_string()
                .contains("ASCII letters")
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
        let directory_store =
            StateStore::resolve(Some(directory_temp.path().to_path_buf())).unwrap();
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
}
