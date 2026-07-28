use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::file_ops;
use crate::host::{RouteHostname, TargetHost};
use crate::session_id::{MAX_SESSION_ID_BYTES, is_valid_session_id};
use crate::types::Route;

use super::{ensure_private_state_file_permissions, open_read_no_follow_maybe_missing};

const VERSION: u32 = 1;
pub(super) const FILE_NAME: &str = "dev-sessions.json";
const STATE_FILE_FALLBACK: &str = "jig-dev-sessions-state";
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
    if !is_valid_session_id(session_id) {
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
mod tests;
