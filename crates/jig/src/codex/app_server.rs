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

/// One in-flight app-server protocol exchange.
///
/// Keep the reader before the writer so its drop order matches the former
/// buffered local variables in the exchange helper.
struct AppServerProtocol<'a, W, R> {
    reader: R,
    writer: W,
    deadline: Option<Instant>,
    cancelled: &'a dyn Fn() -> bool,
}

impl<'a, W, R> AppServerProtocol<'a, W, R> {
    fn new(
        writer: W,
        reader: R,
        deadline: Option<Instant>,
        cancelled: &'a dyn Fn() -> bool,
    ) -> Self {
        Self {
            reader,
            writer,
            deadline,
            cancelled,
        }
    }
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
        app_server_exchange(stdin, stdout, deadline, cancelled, |protocol| {
            protocol.account(include_usage)
        })
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
            app_server_exchange(stdin, stdout, deadline, cancelled, |protocol| {
                protocol.thread(thread_id)
            })
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

fn app_server_exchange<'a, T>(
    stdin: ChildStdin,
    stdout: ProcessInteractionStdout,
    deadline: Option<Instant>,
    cancelled: &'a dyn Fn() -> bool,
    exchange: impl FnOnce(
        &mut AppServerProtocol<'a, BufWriter<ChildStdin>, BufReader<ProcessInteractionStdout>>,
    ) -> std::result::Result<T, String>,
) -> std::result::Result<T, String> {
    let writer = BufWriter::new(stdin);
    let reader = BufReader::new(stdout);
    let mut protocol = AppServerProtocol::new(writer, reader, deadline, cancelled);
    let outcome = exchange(&mut protocol);
    let stderr = protocol.reader.get_mut().take_stderr_output();
    outcome.map_err(|error| append_stderr_context(error, stderr.as_ref()))
}

#[cfg(test)]
pub(super) fn app_server_protocol(
    writer: &mut impl Write,
    reader: &mut impl BufRead,
    include_usage: bool,
    deadline: Option<Instant>,
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<AppServerAccountResponse, String> {
    let mut protocol = AppServerProtocol::new(writer, reader, deadline, cancelled);
    protocol.account(include_usage)
}

#[cfg(test)]
pub(super) fn app_server_thread_protocol(
    writer: &mut impl Write,
    reader: &mut impl BufRead,
    thread_id: &str,
    deadline: Option<Instant>,
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<AppServerThreadLookup, String> {
    let mut protocol = AppServerProtocol::new(writer, reader, deadline, cancelled);
    protocol.thread(thread_id)
}

impl<W, R> AppServerProtocol<'_, W, R>
where
    W: Write,
    R: BufRead,
{
    fn account(
        &mut self,
        include_usage: bool,
    ) -> std::result::Result<AppServerAccountResponse, String> {
        self.initialize()?;
        self.ensure_active()?;
        self.write_message(&json!({
            "method": "account/read",
            "id": 1,
            "params": { "refreshToken": false }
        }))?;
        if include_usage {
            self.ensure_active()?;
            self.write_message(&json!({
                "method": "account/rateLimits/read",
                "id": 2,
                "params": {}
            }))?;
        }

        let mut account = None;
        let mut rate_limits = None;
        let mut usage_complete = !include_usage;
        let mut usage_error = None;
        while account.is_none() || !usage_complete {
            let response = match self.read_next_response() {
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

    fn thread(&mut self, thread_id: &str) -> std::result::Result<AppServerThreadLookup, String> {
        self.initialize()?;
        self.ensure_active()?;
        self.write_message(&json!({
            "method": "thread/read",
            "id": 1,
            "params": {
                "threadId": thread_id,
                "includeTurns": false
            }
        }))?;
        let response = self.read_response(1)?;
        if let Some(error) = response.get("error") {
            if thread_missing_error(error, thread_id) {
                return Ok(AppServerThreadLookup::Missing);
            }
            return Err(response_error(error, "thread/read"));
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

    fn initialize(&mut self) -> std::result::Result<(), String> {
        self.ensure_active()?;
        self.write_message(&json!({
            "method": "initialize",
            "id": 0,
            "params": {
                "clientInfo": {
                    "name": "jig",
                    "title": "Jig",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        }))?;
        response_result(self.read_response(0)?, "initialize")?;
        self.ensure_active()?;
        self.write_message(&json!({ "method": "initialized", "params": {} }))?;
        Ok(())
    }

    fn write_message(&mut self, message: &JsonValue) -> std::result::Result<(), String> {
        serde_json::to_writer(&mut self.writer, message)
            .map_err(|error| format!("could not encode app-server request: {error}"))?;
        self.writer
            .write_all(b"\n")
            .and_then(|()| self.writer.flush())
            .map_err(|error| format!("could not write app-server request: {error}"))
    }

    fn read_response(&mut self, expected_id: u64) -> std::result::Result<JsonValue, String> {
        loop {
            let response = self.read_next_response()?;
            if response.get("id").and_then(JsonValue::as_u64) == Some(expected_id) {
                return Ok(response);
            }
        }
    }

    fn read_next_response(&mut self) -> std::result::Result<JsonValue, String> {
        let mut line = Vec::new();
        loop {
            line.clear();
            let Some(line) = self.read_protocol_line(&mut line)? else {
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

    fn read_protocol_line<'line>(
        &mut self,
        line: &'line mut Vec<u8>,
    ) -> std::result::Result<Option<&'line [u8]>, String> {
        loop {
            self.ensure_active()?;
            let remaining = APP_SERVER_PROTOCOL_MESSAGE_LIMIT
                .saturating_add(1)
                .saturating_sub(line.len());
            if remaining == 0 {
                return Err(protocol_message_too_large());
            }
            let read =
                std::io::Read::take(&mut self.reader, remaining.try_into().unwrap_or(u64::MAX))
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
                    let wait = match self.deadline {
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

    fn ensure_active(&self) -> std::result::Result<(), String> {
        if (self.cancelled)() {
            return Err(APP_SERVER_INSPECTION_CANCELLED.into());
        }
        self.ensure_deadline()
    }

    fn ensure_deadline(&self) -> std::result::Result<(), String> {
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err("Codex app-server protocol timed out".into());
        }
        Ok(())
    }
}

fn thread_missing_error(error: &JsonValue, thread_id: &str) -> bool {
    if error.get("code").and_then(JsonValue::as_i64) != Some(THREAD_MISSING_ERROR_CODE) {
        return false;
    }
    let Some(message) = error.get("message").and_then(JsonValue::as_str) else {
        return false;
    };
    let message = message
        .strip_prefix("thread/read failed:")
        .map(str::trim)
        .unwrap_or(message);
    // Codex has emitted each of these missing-rollout forms across supported
    // app-server generations. Keep the templates and requested id exact:
    // -32600 is also used for unrelated failures such as invalid config.
    [
        format!("thread not loaded: {thread_id}"),
        format!("thread not found: {thread_id}"),
        format!("thread {thread_id} not found"),
        format!("no rollout found for thread id {thread_id}"),
        format!("no rollout found for conversation id {thread_id}"),
    ]
    .iter()
    .any(|missing| message == missing)
}

#[cfg(test)]
pub(super) fn read_response(
    reader: &mut impl BufRead,
    expected_id: u64,
    deadline: Option<Instant>,
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<JsonValue, String> {
    let mut protocol = AppServerProtocol::new(std::io::sink(), reader, deadline, cancelled);
    protocol.read_response(expected_id)
}

#[cfg(test)]
pub(super) fn read_next_response(
    reader: &mut impl BufRead,
    deadline: Option<Instant>,
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<JsonValue, String> {
    let mut protocol = AppServerProtocol::new(std::io::sink(), reader, deadline, cancelled);
    protocol.read_next_response()
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
        return Err(response_error(error, method));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| format!("{method} response did not include a result"))
}

fn response_error(error: &JsonValue, method: &str) -> String {
    let message = error
        .get("message")
        .and_then(JsonValue::as_str)
        .unwrap_or("unknown app-server error");
    format!("{method} failed: {}", bounded_message(message))
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
        OwnedProcessTreeInteractionError::Process(OwnedProcessTreeError::OutputLimitExceeded(
            stream,
        )) => format!("Codex app-server exceeded its {stream} output limit"),
        OwnedProcessTreeInteractionError::Process(OwnedProcessTreeError::Await) => {
            "Codex app-server could not be awaited".into()
        }
        OwnedProcessTreeInteractionError::Process(OwnedProcessTreeError::Cleanup) => {
            "Codex app-server process tree could not be cleaned up safely".into()
        }
    }
}

fn append_stderr_context(error: String, stderr: Option<&BoundedProcessOutput>) -> String {
    // Cancellation is control flow, not a protocol failure. Keep its sentinel
    // stable so callers can promote it to a typed cancellation outcome even
    // when the child happened to write diagnostics before it was stopped.
    if error == APP_SERVER_INSPECTION_CANCELLED {
        return error;
    }
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
