#[cfg(unix)]
use std::fs;
use std::io::{BufReader, Cursor, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::super::app_server::{
    APP_SERVER_INSPECTION_CANCELLED, APP_SERVER_PROTOCOL_MESSAGE_LIMIT, AppServerThreadLookup,
    app_server_protocol, app_server_thread_protocol, protocol_message_too_large,
    read_next_response, read_response,
};
#[cfg(unix)]
use super::super::app_server::{app_server_account_with_timeout, app_server_thread};
use super::super::*;

struct FragmentedNonblockingReader {
    bytes: &'static [u8],
    offset: usize,
    would_block: bool,
}

impl Read for FragmentedNonblockingReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.would_block {
            self.would_block = false;
            return Err(std::io::ErrorKind::WouldBlock.into());
        }
        if self.offset == self.bytes.len() {
            return Ok(0);
        }
        let read = buffer
            .len()
            .min(3)
            .min(self.bytes.len().saturating_sub(self.offset));
        buffer[..read].copy_from_slice(&self.bytes[self.offset..self.offset + read]);
        self.offset += read;
        self.would_block = true;
        Ok(read)
    }
}

struct RepeatingReader {
    bytes: &'static [u8],
    offset: usize,
}

struct CancelAfterEofReader {
    bytes: &'static [u8],
    offset: usize,
    cancelled: Arc<AtomicBool>,
}

impl Read for CancelAfterEofReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.offset == self.bytes.len() {
            self.cancelled.store(true, Ordering::Release);
            return Err(std::io::ErrorKind::WouldBlock.into());
        }
        let read = buffer
            .len()
            .min(self.bytes.len().saturating_sub(self.offset));
        buffer[..read].copy_from_slice(&self.bytes[self.offset..self.offset + read]);
        self.offset += read;
        Ok(read)
    }
}

impl Read for RepeatingReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        for byte in buffer.iter_mut() {
            *byte = self.bytes[self.offset];
            self.offset = (self.offset + 1) % self.bytes.len();
        }
        Ok(buffer.len())
    }
}

#[test]
fn app_server_protocol_completes_handshake_before_account_requests() {
    let responses = concat!(
        "{\"id\":0,\"result\":{}}\n",
        "{\"method\":\"account/updated\",\"params\":{}}\n",
        "{\"id\":1,\"result\":{\"account\":{\"type\":\"chatgpt\",\"email\":\"person@example.com\",\"planType\":\"pro\"}}}\n",
        "{\"id\":2,\"result\":{\"rateLimitsByLimitId\":{}}}\n"
    );
    let mut reader = Cursor::new(responses.as_bytes());
    let mut requests = Vec::new();

    let response = app_server_protocol(&mut requests, &mut reader, true, None, &|| false).unwrap();

    assert_eq!(response.account["account"]["email"], "person@example.com");
    assert!(response.rate_limits.is_some());
    let messages = String::from_utf8(requests)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<JsonValue>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(messages[0]["method"], "initialize");
    assert_eq!(messages[1]["method"], "initialized");
    assert_eq!(messages[2]["method"], "account/read");
    assert_eq!(messages[3]["method"], "account/rateLimits/read");
    assert_eq!(messages[2]["params"]["refreshToken"], false);
}

#[test]
fn app_server_thread_protocol_reads_only_the_requested_thread_metadata() {
    let thread_id = "019fe6e4-972f-7392-aaf3-58cb652a4e20";
    let responses = format!(
        "{{\"id\":0,\"result\":{{}}}}\n{{\"id\":1,\"result\":{{\"thread\":{{\"id\":\"{thread_id}\"}}}}}}\n"
    );
    let mut reader = Cursor::new(responses.as_bytes());
    let mut requests = Vec::new();

    let result =
        app_server_thread_protocol(&mut requests, &mut reader, thread_id, None, &|| false).unwrap();

    assert_eq!(result, AppServerThreadLookup::Found);
    let messages = String::from_utf8(requests)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<JsonValue>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(messages[0]["method"], "initialize");
    assert_eq!(messages[1]["method"], "initialized");
    assert_eq!(messages[2]["method"], "thread/read");
    assert_eq!(messages[2]["params"]["threadId"], thread_id);
    assert_eq!(messages[2]["params"]["includeTurns"], false);
}

#[test]
fn app_server_thread_protocol_distinguishes_missing_threads_from_failures() {
    let thread_id = "019fe6e4-972f-7392-aaf3-58cb652a4e20";
    for message in [
        format!("thread not loaded: {thread_id}"),
        format!("thread not found: {thread_id}"),
        format!("thread {thread_id} not found"),
        format!("no rollout found for thread id {thread_id}"),
        format!("no rollout found for conversation id {thread_id}"),
        format!("thread/read failed: thread not loaded: {thread_id}"),
    ] {
        let missing = format!(
            "{{\"id\":0,\"result\":{{}}}}\n{{\"id\":1,\"error\":{{\"code\":-32600,\"message\":{}}}}}\n",
            serde_json::to_string(&message).unwrap()
        );
        let mut reader = Cursor::new(missing.as_bytes());
        assert_eq!(
            app_server_thread_protocol(&mut Vec::new(), &mut reader, thread_id, None, &|| false)
                .unwrap(),
            AppServerThreadLookup::Missing,
            "missing-thread variant was not recognized: {message}"
        );
    }

    let wrong_code = format!(
        "{{\"id\":0,\"result\":{{}}}}\n{{\"id\":1,\"error\":{{\"code\":-32603,\"message\":\"thread not loaded: {thread_id}\"}}}}\n"
    );
    let mut reader = Cursor::new(wrong_code.as_bytes());
    assert!(
        app_server_thread_protocol(&mut Vec::new(), &mut reader, thread_id, None, &|| false)
            .unwrap_err()
            .contains("thread not loaded")
    );

    let unrelated_invalid_request = concat!(
        "{\"id\":0,\"result\":{}}\n",
        "{\"id\":1,\"error\":{\"code\":-32600,\"message\":\"failed to load configuration\"}}\n"
    );
    let mut reader = Cursor::new(unrelated_invalid_request.as_bytes());
    assert!(
        app_server_thread_protocol(&mut Vec::new(), &mut reader, thread_id, None, &|| false)
            .unwrap_err()
            .contains("failed to load configuration")
    );

    let different_thread = concat!(
        "{\"id\":0,\"result\":{}}\n",
        "{\"id\":1,\"error\":{\"code\":-32600,\"message\":\"no rollout found for thread id 00000000-0000-0000-0000-000000000000\"}}\n"
    );
    let mut reader = Cursor::new(different_thread.as_bytes());
    assert!(
        app_server_thread_protocol(&mut Vec::new(), &mut reader, thread_id, None, &|| false)
            .unwrap_err()
            .contains("no rollout found")
    );

    let failed = concat!(
        "{\"id\":0,\"result\":{}}\n",
        "{\"id\":1,\"error\":{\"code\":-32603,\"message\":\"state database unavailable\"}}\n"
    );
    let mut reader = Cursor::new(failed.as_bytes());
    let error =
        app_server_thread_protocol(&mut Vec::new(), &mut reader, thread_id, None, &|| false)
            .unwrap_err();
    assert!(error.contains("state database unavailable"), "{error}");
}

#[test]
fn app_server_thread_protocol_uses_method_neutral_eof_errors() {
    let mut reader = Cursor::new(b"{\"id\":0,\"result\":{}}\n".as_slice());
    let error = app_server_thread_protocol(
        &mut Vec::new(),
        &mut reader,
        "019fe6e4-972f-7392-aaf3-58cb652a4e20",
        None,
        &|| false,
    )
    .unwrap_err();

    assert!(error.contains("requested response"), "{error}");
    assert!(!error.contains("account data"), "{error}");
}

#[test]
fn app_server_protocol_stops_before_writing_when_cancelled() {
    let mut reader = Cursor::new(Vec::<u8>::new());
    let mut requests = Vec::new();

    let error = app_server_protocol(&mut requests, &mut reader, true, None, &|| true).unwrap_err();

    assert_eq!(error, "Codex app-server inspection was cancelled");
    assert!(requests.is_empty());
}

#[test]
fn app_server_protocol_keeps_account_when_usage_is_unavailable() {
    let responses = concat!(
        "{\"id\":0,\"result\":{}}\n",
        "{\"id\":1,\"result\":{\"account\":{\"type\":\"apiKey\"}}}\n",
        "{\"id\":2,\"error\":{\"message\":\"rate limits require ChatGPT auth\"}}\n"
    );
    let mut reader = Cursor::new(responses.as_bytes());
    let mut requests = Vec::new();

    let response = app_server_protocol(&mut requests, &mut reader, true, None, &|| false).unwrap();

    assert_eq!(response.account["account"]["type"], "apiKey");
    assert!(response.rate_limits.is_none());
    assert_eq!(
        response.usage_error.as_deref(),
        Some("account/rateLimits/read failed: rate limits require ChatGPT auth")
    );
}

#[test]
fn app_server_protocol_keeps_account_when_usage_response_never_arrives() {
    let responses = concat!(
        "{\"id\":0,\"result\":{}}\n",
        "{\"id\":1,\"result\":{\"account\":{\"type\":\"chatgpt\",\"email\":\"person@example.com\"}}}\n"
    );
    let mut reader = Cursor::new(responses.as_bytes());
    let mut requests = Vec::new();

    let response = app_server_protocol(&mut requests, &mut reader, true, None, &|| false).unwrap();

    assert_eq!(response.account["account"]["email"], "person@example.com");
    assert!(response.rate_limits.is_none());
    assert!(
        response
            .usage_error
            .as_deref()
            .is_some_and(|error| error.contains("closed before returning"))
    );
}

#[test]
fn app_server_protocol_propagates_cancellation_after_account_response() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let source = CancelAfterEofReader {
        bytes: concat!(
            "{\"id\":0,\"result\":{}}\n",
            "{\"id\":1,\"result\":{\"account\":{\"type\":\"chatgpt\"}}}\n"
        )
        .as_bytes(),
        offset: 0,
        cancelled: Arc::clone(&cancelled),
    };
    let mut reader = BufReader::new(source);
    let mut requests = Vec::new();

    let error = app_server_protocol(&mut requests, &mut reader, true, None, &|| {
        cancelled.load(Ordering::Acquire)
    })
    .unwrap_err();

    assert_eq!(error, APP_SERVER_INSPECTION_CANCELLED);
}

#[test]
fn app_server_protocol_does_not_wait_for_usage_after_logged_out_account() {
    let responses = concat!(
        "{\"id\":0,\"result\":{}}\n",
        "{\"id\":1,\"result\":{\"account\":null}}\n"
    );
    let mut reader = Cursor::new(responses.as_bytes());
    let mut requests = Vec::new();

    let response = app_server_protocol(&mut requests, &mut reader, true, None, &|| false).unwrap();

    assert!(response.account["account"].is_null());
    assert!(response.rate_limits.is_none());
    assert!(response.usage_error.is_none());
}

#[test]
fn app_server_protocol_rejects_a_newline_free_oversized_message() {
    let oversized = vec![b'x'; APP_SERVER_PROTOCOL_MESSAGE_LIMIT + 1];
    let mut reader = Cursor::new(oversized);

    let error = read_next_response(&mut reader, None, &|| false).unwrap_err();

    assert_eq!(error, protocol_message_too_large());
}

#[test]
fn app_server_protocol_accepts_an_exact_limit_payload_plus_newline() {
    let mut message = br#"{"id":1}"#.to_vec();
    message.resize(APP_SERVER_PROTOCOL_MESSAGE_LIMIT, b' ');
    message.push(b'\n');
    let mut reader = Cursor::new(message);

    let response = read_next_response(&mut reader, None, &|| false).unwrap();

    assert_eq!(response["id"], 1);
}

#[test]
fn app_server_protocol_reports_eof_in_a_partial_protocol_line() {
    let mut reader = Cursor::new(br#"{\"id\":1"#.to_vec());

    let error = read_next_response(&mut reader, None, &|| false).unwrap_err();

    assert_eq!(error, "app-server closed before completing a protocol line");
}

#[test]
fn app_server_protocol_preserves_fragmented_nonblocking_utf8_lines() {
    let source = FragmentedNonblockingReader {
        bytes: "{\"id\":1,\"result\":\"é\"}\n".as_bytes(),
        offset: 0,
        would_block: false,
    };
    let mut reader = BufReader::with_capacity(4, source);

    let response = read_next_response(
        &mut reader,
        Instant::now().checked_add(Duration::from_secs(1)),
        &|| false,
    )
    .unwrap();

    assert_eq!(response["result"], "é");
}

#[test]
fn app_server_protocol_deadline_survives_continuous_irrelevant_messages() {
    let source = RepeatingReader {
        bytes: b"{\"method\":\"tick\"}\n",
        offset: 0,
    };
    let mut reader = BufReader::with_capacity(128, source);
    let started = Instant::now();

    let error = read_response(
        &mut reader,
        99,
        Instant::now().checked_add(Duration::from_millis(20)),
        &|| false,
    )
    .unwrap_err();

    assert_eq!(error, "Codex app-server protocol timed out");
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[cfg(unix)]
#[test]
fn app_server_client_interacts_with_and_cleans_up_a_long_lived_process_tree() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex-home");
    fs::create_dir(&home).unwrap();
    let stub = temp.path().join("codex-stub.sh");
    fs::write(
        &stub,
        r#"#!/bin/sh
read -r initialize
printf '%s\n' '{"id":0,"result":{}}'
read -r initialized
read -r account
read -r limits
printf '%s\n' '{"id":1,"result":{"account":{"type":"chatgpt","email":"stub@example.com","planType":"plus"}}}'
printf '%s\n' '{"id":2,"result":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":12,"windowDurationMins":300}}}}'
sleep 30
"#,
    )
    .unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();

    let response = app_server_account_with_timeout(
        &home,
        stub.as_os_str(),
        true,
        Duration::from_secs(2),
        &|| false,
    )
    .unwrap();

    assert_eq!(response.account["account"]["email"], "stub@example.com");
    assert_eq!(
        response.rate_limits.unwrap()["rateLimits"]["limitId"],
        "codex"
    );
}

#[cfg(unix)]
#[test]
fn app_server_client_surfaces_bounded_stderr_from_startup_failures() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex-home");
    fs::create_dir(&home).unwrap();
    let stub = temp.path().join("codex-stub.sh");
    fs::write(
        &stub,
        r#"#!/bin/sh
printf '%s\n' 'unsupported app-server subcommand' >&2
dd if=/dev/zero bs=1024 count=128 2>/dev/null | tr '\000' x >&2
exit 64
"#,
    )
    .unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    let started = Instant::now();

    let error = app_server_account_with_timeout(
        &home,
        stub.as_os_str(),
        false,
        Duration::from_secs(2),
        &|| false,
    )
    .unwrap_err();

    assert!(
        error.contains("unsupported app-server subcommand"),
        "{error}"
    );
    assert!(error.len() < 512, "stderr preview was not bounded: {error}");
    assert!(started.elapsed() < Duration::from_secs(1));
}
#[cfg(unix)]
#[test]
fn app_server_client_thread_attaches_stderr_to_protocol_failures() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex-home");
    fs::create_dir(&home).unwrap();
    let stub = temp.path().join("codex-stub.sh");
    fs::write(
        &stub,
        r#"#!/bin/sh
read -r initialize
printf '%s\n' '{"id":0,"result":{}}'
read -r initialized
read -r thread
printf '%s\n' 'thread lookup diagnostics' >&2
printf '%s\n' '{"id":1,"error":{"code":-32603,"message":"thread state unavailable"}}'
"#,
    )
    .unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();

    let error = app_server_thread(
        &home,
        stub.as_os_str(),
        "019fe6e4-972f-7392-aaf3-58cb652a4e20",
        &|| false,
    )
    .unwrap_err();

    assert_eq!(
        error,
        "thread/read failed: thread state unavailable; app-server stderr: thread lookup diagnostics"
    );
}

#[cfg(unix)]
#[test]
fn app_server_client_thread_keeps_cancellation_distinct_from_stderr() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex-home");
    let cancelled = temp.path().join("cancelled");
    fs::create_dir(&home).unwrap();
    let stub = temp.path().join("codex-stub.sh");
    fs::write(
        &stub,
        format!(
            "#!/bin/sh\nread -r initialize\nprintf '%s\\n' 'shutdown diagnostics' >&2\ntouch '{}'\nsleep 30\n",
            cancelled.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();

    let error = app_server_thread(
        &home,
        stub.as_os_str(),
        "019fe6e4-972f-7392-aaf3-58cb652a4e20",
        &|| cancelled.exists(),
    )
    .unwrap_err();

    assert_eq!(error, APP_SERVER_INSPECTION_CANCELLED);
}

#[cfg(unix)]
#[test]
fn app_server_client_bounds_an_unresponsive_child() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex-home");
    fs::create_dir(&home).unwrap();
    let stub = temp.path().join("codex-stub.sh");
    fs::write(&stub, "#!/bin/sh\nsleep 30\n").unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();

    let error = app_server_account_with_timeout(
        &home,
        stub.as_os_str(),
        false,
        Duration::from_millis(50),
        &|| false,
    )
    .unwrap_err();

    assert!(error.contains("timed out"), "{error}");
}

#[cfg(unix)]
#[test]
fn app_server_client_cancels_during_a_live_inspection() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex-home");
    fs::create_dir(&home).unwrap();
    let stub = temp.path().join("codex-stub.sh");
    fs::write(&stub, "#!/bin/sh\nsleep 30\n").unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let started = Instant::now();

    let error = std::thread::scope(|scope| {
        let cancellation = Arc::clone(&cancelled);
        scope.spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            cancellation.store(true, Ordering::Release);
        });
        app_server_account_with_timeout(
            &home,
            stub.as_os_str(),
            false,
            Duration::from_secs(2),
            &|| cancelled.load(Ordering::Acquire),
        )
        .unwrap_err()
    });

    assert!(error.contains("cancelled"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(1));
}
