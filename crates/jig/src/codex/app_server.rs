use std::ffi::OsStr;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::process::{ChildStdin, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use jig_owned_process::interaction::{
    OwnedProcessTreeInteractionError, ProcessInteractionStdout,
    run_owned_process_tree_with_cooperative_interaction,
};
use jig_owned_process::{BoundedProcessOutput, OwnedProcessTreeError};
use serde_json::{Value as JsonValue, json};

use super::CODEX_HOME_ENV;

const APP_SERVER_TIMEOUT: Duration = Duration::from_secs(5);
const THREAD_MISSING_ERROR_CODE: i64 = -32600;
pub(super) const APP_SERVER_PROTOCOL_MESSAGE_LIMIT: usize = 64 * 1024;
pub(super) const APP_SERVER_INSPECTION_CANCELLED: &str =
    "Codex app-server inspection was cancelled";
const APP_SERVER_PROTOCOL_POLL_INTERVAL: Duration = Duration::from_millis(2);

#[derive(Debug)]
pub(super) struct AppServerAccountResponse {
    pub(super) account: JsonValue,
    pub(super) rate_limits: Option<JsonValue>,
    pub(super) usage_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AppServerThreadLookup {
    Found,
    Missing,
}

pub(super) fn app_server_account(
    home: &Path,
    codex_bin: &OsStr,
    include_usage: bool,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> std::result::Result<AppServerAccountResponse, String> {
    app_server_account_with_timeout(
        home,
        codex_bin,
        include_usage,
        APP_SERVER_TIMEOUT,
        cancelled,
    )
}

pub(super) fn app_server_account_with_timeout(
    home: &Path,
    codex_bin: &OsStr,
    include_usage: bool,
    timeout: Duration,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> std::result::Result<AppServerAccountResponse, String> {
    run_app_server(home, codex_bin, timeout, move |stdin, stdout, deadline| {
        app_server_exchange(stdin, stdout, include_usage, deadline, cancelled)
    })
}

pub(super) fn app_server_thread(
    home: &Path,
    codex_bin: &OsStr,
    thread_id: &str,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> std::result::Result<AppServerThreadLookup, String> {
    run_app_server(
        home,
        codex_bin,
        APP_SERVER_TIMEOUT,
        move |stdin, stdout, deadline| {
            app_server_thread_exchange(stdin, stdout, thread_id, deadline, cancelled)
        },
    )
}

fn run_app_server<T>(
    home: &Path,
    codex_bin: &OsStr,
    timeout: Duration,
    interaction: impl FnOnce(
        ChildStdin,
        ProcessInteractionStdout,
        Option<Instant>,
    ) -> std::result::Result<T, String>,
) -> std::result::Result<T, String> {
    let mut command = Command::new(codex_bin);
    command
        .arg("app-server")
        .env(CODEX_HOME_ENV, home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::shell::sanitize_bash_environment(&mut command);

    run_owned_process_tree_with_cooperative_interaction(&mut command, timeout, interaction)
        .map_err(|error| app_server_error(&error, timeout))
}

fn app_server_exchange(
    stdin: ChildStdin,
    stdout: ProcessInteractionStdout,
    include_usage: bool,
    deadline: Option<Instant>,
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<AppServerAccountResponse, String> {
    let mut writer = BufWriter::new(stdin);
    let mut reader = BufReader::new(stdout);
    let outcome = app_server_protocol(&mut writer, &mut reader, include_usage, deadline, cancelled);
    let stderr = reader.get_mut().take_stderr_output();
    outcome.map_err(|error| append_stderr_context(error, stderr.as_ref()))
}

fn app_server_thread_exchange(
    stdin: ChildStdin,
    stdout: ProcessInteractionStdout,
    thread_id: &str,
    deadline: Option<Instant>,
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<AppServerThreadLookup, String> {
    let mut writer = BufWriter::new(stdin);
    let mut reader = BufReader::new(stdout);
    let outcome =
        app_server_thread_protocol(&mut writer, &mut reader, thread_id, deadline, cancelled);
    let stderr = reader.get_mut().take_stderr_output();
    outcome.map_err(|error| append_stderr_context(error, stderr.as_ref()))
}

pub(super) fn app_server_protocol(
    writer: &mut impl Write,
    reader: &mut impl BufRead,
    include_usage: bool,
    deadline: Option<Instant>,
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<AppServerAccountResponse, String> {
    initialize_protocol(writer, reader, deadline, cancelled)?;
    ensure_protocol_active(deadline, cancelled)?;
    write_message(
        writer,
        &json!({
            "method": "account/read",
            "id": 1,
            "params": { "refreshToken": false }
        }),
    )?;
    if include_usage {
        ensure_protocol_active(deadline, cancelled)?;
        write_message(
            writer,
            &json!({
                "method": "account/rateLimits/read",
                "id": 2,
                "params": {}
            }),
        )?;
    }

    let mut account = None;
    let mut rate_limits = None;
    let mut usage_complete = !include_usage;
    let mut usage_error = None;
    while account.is_none() || !usage_complete {
        let response = match read_next_response(reader, deadline, cancelled) {
            Ok(response) => response,
            Err(error)
                if account.is_some()
                    && !usage_complete
                    && error != APP_SERVER_INSPECTION_CANCELLED =>
            {
                usage_error = Some(format!("account/rateLimits/read unavailable: {error}"));
                break;
            }
            Err(error) => return Err(error),
        };
        match response.get("id").and_then(JsonValue::as_u64) {
            Some(1) => {
                let result = response_result(response, "account/read")?;
                if result.get("account").is_some_and(JsonValue::is_null) {
                    usage_complete = true;
                    rate_limits = None;
                    usage_error = None;
                }
                account = Some(result);
            }
            Some(2) if include_usage => {
                usage_complete = true;
                match response_result(response, "account/rateLimits/read") {
                    Ok(result) => rate_limits = Some(result),
                    Err(error) => usage_error = Some(error),
                }
            }
            _ => {}
        }
    }

    Ok(AppServerAccountResponse {
        account: account.expect("account response checked above"),
        rate_limits,
        usage_error,
    })
}

pub(super) fn app_server_thread_protocol(
    writer: &mut impl Write,
    reader: &mut impl BufRead,
    thread_id: &str,
    deadline: Option<Instant>,
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<AppServerThreadLookup, String> {
    initialize_protocol(writer, reader, deadline, cancelled)?;
    ensure_protocol_active(deadline, cancelled)?;
    write_message(
        writer,
        &json!({
            "method": "thread/read",
            "id": 1,
            "params": {
                "threadId": thread_id,
                "includeTurns": false
            }
        }),
    )?;
    let response = read_response(reader, 1, deadline, cancelled)?;
    if let Some(error) = response.get("error") {
        if thread_missing_error(error, thread_id) {
            return Ok(AppServerThreadLookup::Missing);
        }
        return response_result(response, "thread/read").map(|_| AppServerThreadLookup::Found);
    }
    let result = response_result(response, "thread/read")?;
    let returned_id = result
        .get("thread")
        .and_then(|thread| thread.get("id"))
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "thread/read response did not include a thread id".to_owned())?;
    if returned_id != thread_id {
        return Err("thread/read returned a different thread id".into());
    }
    Ok(AppServerThreadLookup::Found)
}

fn thread_missing_error(error: &JsonValue, thread_id: &str) -> bool {
    if error.get("code").and_then(JsonValue::as_i64) != Some(THREAD_MISSING_ERROR_CODE) {
        return false;
    }
    let Some(message) = error.get("message").and_then(JsonValue::as_str) else {
        return false;
    };
    ["thread not loaded:", "thread not found:"]
        .into_iter()
        .any(|prefix| {
            message
                .strip_prefix(prefix)
                .is_some_and(|id| id.trim() == thread_id)
        })
}

fn initialize_protocol(
    writer: &mut impl Write,
    reader: &mut impl BufRead,
    deadline: Option<Instant>,
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<(), String> {
    ensure_protocol_active(deadline, cancelled)?;
    write_message(
        writer,
        &json!({
            "method": "initialize",
            "id": 0,
            "params": {
                "clientInfo": {
                    "name": "jig",
                    "title": "Jig",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        }),
    )?;
    response_result(read_response(reader, 0, deadline, cancelled)?, "initialize")?;
    ensure_protocol_active(deadline, cancelled)?;
    write_message(writer, &json!({ "method": "initialized", "params": {} }))?;
    Ok(())
}

fn write_message(writer: &mut impl Write, message: &JsonValue) -> std::result::Result<(), String> {
    serde_json::to_writer(&mut *writer, message)
        .map_err(|error| format!("could not encode app-server request: {error}"))?;
    writer
        .write_all(b"\n")
        .and_then(|()| writer.flush())
        .map_err(|error| format!("could not write app-server request: {error}"))
}

pub(super) fn read_response(
    reader: &mut impl BufRead,
    expected_id: u64,
    deadline: Option<Instant>,
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<JsonValue, String> {
    loop {
        let response = read_next_response(reader, deadline, cancelled)?;
        if response.get("id").and_then(JsonValue::as_u64) == Some(expected_id) {
            return Ok(response);
        }
    }
}

pub(super) fn read_next_response(
    reader: &mut impl BufRead,
    deadline: Option<Instant>,
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<JsonValue, String> {
    let mut line = Vec::new();
    loop {
        line.clear();
        let Some(line) = read_protocol_line(reader, &mut line, deadline, cancelled)? else {
            return Err("app-server closed before returning the requested response".into());
        };
        if !line.ends_with(b"\n") {
            return Err("app-server closed before completing a protocol line".into());
        }
        let line = std::str::from_utf8(line)
            .map_err(|_| "app-server returned a non-UTF-8 protocol line")?
            .trim();
        if line.is_empty() {
            continue;
        }
        return serde_json::from_str(line)
            .map_err(|_| "app-server returned a non-JSON protocol line".into());
    }
}

fn read_protocol_line<'a>(
    reader: &mut impl BufRead,
    line: &'a mut Vec<u8>,
    deadline: Option<Instant>,
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<Option<&'a [u8]>, String> {
    loop {
        ensure_protocol_active(deadline, cancelled)?;
        let remaining = APP_SERVER_PROTOCOL_MESSAGE_LIMIT
            .saturating_add(1)
            .saturating_sub(line.len());
        if remaining == 0 {
            return Err(protocol_message_too_large());
        }
        let read = std::io::Read::take(&mut *reader, remaining.try_into().unwrap_or(u64::MAX))
            .read_until(b'\n', line);
        match read {
            Ok(0) if line.is_empty() => return Ok(None),
            Ok(_) if protocol_payload_len(line) > APP_SERVER_PROTOCOL_MESSAGE_LIMIT => {
                return Err(protocol_message_too_large());
            }
            Ok(_) => return Ok(Some(line)),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if protocol_payload_len(line) > APP_SERVER_PROTOCOL_MESSAGE_LIMIT {
                    return Err(protocol_message_too_large());
                }
                let wait = match deadline {
                    Some(deadline) => {
                        let Some(wait) = deadline.checked_duration_since(Instant::now()) else {
                            return Err("Codex app-server protocol timed out".into());
                        };
                        wait.min(APP_SERVER_PROTOCOL_POLL_INTERVAL)
                    }
                    None => APP_SERVER_PROTOCOL_POLL_INTERVAL,
                };
                thread::sleep(wait);
            }
            Err(error) => {
                return Err(format!("could not read app-server response: {error}"));
            }
        }
    }
}

fn ensure_protocol_deadline(deadline: Option<Instant>) -> std::result::Result<(), String> {
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err("Codex app-server protocol timed out".into());
    }
    Ok(())
}

fn ensure_protocol_active(
    deadline: Option<Instant>,
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<(), String> {
    if cancelled() {
        return Err(APP_SERVER_INSPECTION_CANCELLED.into());
    }
    ensure_protocol_deadline(deadline)
}

fn protocol_payload_len(line: &[u8]) -> usize {
    line.len()
        .saturating_sub(usize::from(line.last() == Some(&b'\n')))
}

pub(super) fn protocol_message_too_large() -> String {
    format!(
        "app-server protocol message exceeded the {APP_SERVER_PROTOCOL_MESSAGE_LIMIT}-byte limit"
    )
}

fn response_result(response: JsonValue, method: &str) -> std::result::Result<JsonValue, String> {
    if let Some(error) = response.get("error") {
        let message = error
            .get("message")
            .and_then(JsonValue::as_str)
            .unwrap_or("unknown app-server error");
        return Err(format!("{method} failed: {}", bounded_message(message)));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| format!("{method} response did not include a result"))
}

fn app_server_error(error: &OwnedProcessTreeInteractionError, timeout: Duration) -> String {
    match error {
        OwnedProcessTreeInteractionError::Interaction(error) => error.clone(),
        OwnedProcessTreeInteractionError::InteractionAndCleanup(error) => format!(
            "Codex app-server process tree could not be cleaned up safely; the app-server interaction also failed: {error}"
        ),
        OwnedProcessTreeInteractionError::Process(OwnedProcessTreeError::Start(error)) => {
            format!("could not start Codex app-server: {error}")
        }
        OwnedProcessTreeInteractionError::Process(OwnedProcessTreeError::TimedOut) => {
            format!(
                "Codex app-server timed out after {} seconds",
                timeout.as_secs_f64()
            )
        }
        OwnedProcessTreeInteractionError::Process(OwnedProcessTreeError::Cancelled) => {
            APP_SERVER_INSPECTION_CANCELLED.into()
        }
        OwnedProcessTreeInteractionError::Process(OwnedProcessTreeError::Await) => {
            "Codex app-server could not be awaited".into()
        }
        OwnedProcessTreeInteractionError::Process(OwnedProcessTreeError::Cleanup) => {
            "Codex app-server process tree could not be cleaned up safely".into()
        }
    }
}

fn append_stderr_context(error: String, stderr: Option<&BoundedProcessOutput>) -> String {
    let Some(stderr) = stderr else {
        return error;
    };
    let stderr = stderr.to_string_lossy();
    let Some(stderr) = stderr.lines().find(|line| !line.trim().is_empty()) else {
        return error;
    };
    format!(
        "{error}; app-server stderr: {}",
        bounded_message(stderr.trim())
    )
}

fn bounded_message(message: &str) -> String {
    const MAX_CHARS: usize = 160;
    let first_line = message.lines().next().unwrap_or_default();
    let mut bounded = first_line.chars().take(MAX_CHARS).collect::<String>();
    if first_line.chars().count() > MAX_CHARS {
        bounded.push('…');
    }
    bounded
}
