use std::io::{self, BufRead, Write};

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::execution::{ExecutionCancellation, ExecutionEvent, ExecutionObserver, ExecutionStream};
use crate::runtime::call_tool_with_observer;
use crate::tool_defs;

const MCP_PROGRESS_EVENT_LIMIT: usize = 64;
const MCP_OUTPUT_PREVIEW_LIMIT: usize = 4 * 1024;

pub fn serve(ctx: &RepoContext) -> Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = stdout.lock();

    loop {
        let Some((message, framing)) = read_message(&mut reader)? else {
            return Ok(());
        };

        let Some(method) = message.get("method").and_then(Value::as_str) else {
            continue;
        };

        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));

        let response = match method {
            "initialize" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {
                        "tools": {
                            "listChanged": false
                        }
                    },
                    "serverInfo": {
                        "name": "jig",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }
            })),
            "notifications/initialized" => None,
            "ping" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {}
            })),
            "tools/list" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": tool_defs::tool_descriptors(ctx.tool_specs())
                }
            })),
            "tools/call" => Some(handle_tool_call(ctx, id, params, &mut writer, framing)),
            other => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("Unsupported method: {other}")
                }
            })),
        };

        if let Some(response) = response {
            write_message(&mut writer, &response, framing)?;
        }
    }
}

fn handle_tool_call(
    ctx: &RepoContext,
    id: Option<Value>,
    params: Value,
    writer: &mut dyn Write,
    framing: MessageFraming,
) -> Value {
    let result = (|| -> Result<Value> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("tools/call requires params.name"))?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let progress_token = params
            .get("_meta")
            .and_then(|meta| meta.get("progressToken"))
            .filter(|token| token.is_string() || token.is_number())
            .cloned();
        let mut observer = McpProgressObserver::new(writer, framing, progress_token);
        let tool_result = call_tool_with_observer(ctx, name, args, &mut observer);
        observer.flush()?;
        let tool_result = tool_result?;
        Ok(json!({
            "content": [
                {
                    "type": "text",
                    "text": serde_json::to_string_pretty(&tool_result)?
                }
            ],
            "structuredContent": tool_result,
            "isError": false
        }))
    })();

    match result {
        Ok(result) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }),
        Err(error) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32000,
                "message": error.to_string()
            }
        }),
    }
}

struct McpProgressObserver<'a> {
    writer: &'a mut dyn Write,
    framing: MessageFraming,
    progress_token: Option<Value>,
    progress: u64,
    messages: Vec<String>,
    messages_truncated: bool,
    stdout: OutputPreview,
    stderr: OutputPreview,
}

#[derive(Default)]
struct OutputPreview {
    bytes: Vec<u8>,
    truncated: bool,
}

impl OutputPreview {
    fn push(&mut self, bytes: &[u8]) {
        let remaining = MCP_OUTPUT_PREVIEW_LIMIT.saturating_sub(self.bytes.len());
        let retained = remaining.min(bytes.len());
        self.bytes.extend_from_slice(&bytes[..retained]);
        self.truncated |= retained < bytes.len();
    }

    fn message(&self, stream: &str) -> Option<String> {
        if self.bytes.is_empty() && !self.truncated {
            return None;
        }
        let preview = String::from_utf8_lossy(&self.bytes);
        let suffix = if self.truncated {
            " [preview truncated]"
        } else {
            ""
        };
        Some(format!("{stream}: {}{suffix}", preview.trim_end()))
    }
}

impl<'a> McpProgressObserver<'a> {
    fn new(
        writer: &'a mut dyn Write,
        framing: MessageFraming,
        progress_token: Option<Value>,
    ) -> Self {
        Self {
            writer,
            framing,
            progress_token,
            progress: 0,
            messages: Vec::new(),
            messages_truncated: false,
            stdout: OutputPreview::default(),
            stderr: OutputPreview::default(),
        }
    }

    fn queue(&mut self, message: String) {
        if self.progress_token.is_none() {
            return;
        }
        if self.messages.len() == MCP_PROGRESS_EVENT_LIMIT {
            self.messages_truncated = true;
            return;
        }
        self.messages.push(message);
    }

    fn notify(&mut self, message: &str) -> Result<()> {
        let Some(progress_token) = self.progress_token.clone() else {
            return Ok(());
        };
        self.progress = self.progress.saturating_add(1);
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/progress",
            "params": {
                "progressToken": progress_token,
                "progress": self.progress,
                "message": message,
            }
        });
        write_message(self.writer, &notification, self.framing)
    }

    fn flush(&mut self) -> Result<()> {
        let mut messages = std::mem::take(&mut self.messages);
        if let Some(message) = self.stdout.message("stdout") {
            messages.push(message);
        }
        if let Some(message) = self.stderr.message("stderr") {
            messages.push(message);
        }
        if self.messages_truncated {
            messages.push("additional progress events omitted".to_string());
        }
        for message in messages {
            self.notify(&message)
                .map_err(|error| anyhow!("Failed to send MCP progress notification: {error}"))?;
        }
        Ok(())
    }
}

impl ExecutionObserver for McpProgressObserver<'_> {
    fn event(&mut self, event: ExecutionEvent<'_>) {
        let message = match event {
            ExecutionEvent::PhaseStarted { label, position } => format!(
                "{label} started ({}/{})",
                position.current(),
                position.total()
            ),
            ExecutionEvent::Output { stream, bytes } => {
                match stream {
                    ExecutionStream::Stdout => self.stdout.push(bytes),
                    ExecutionStream::Stderr => self.stderr.push(bytes),
                }
                return;
            }
            ExecutionEvent::Heartbeat { label, elapsed } => {
                format!("{label} reached {}s", elapsed.as_secs())
            }
            ExecutionEvent::PhaseFinished {
                label,
                success,
                elapsed,
            } => format!(
                "{label} {} ({}s)",
                if success { "finished" } else { "failed" },
                elapsed.as_secs()
            ),
        };
        self.queue(message);
    }
}

impl ExecutionCancellation for McpProgressObserver<'_> {
    fn cancelled(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MessageFraming {
    JsonLine,
    ContentLength,
}

fn read_message(reader: &mut dyn BufRead) -> Result<Option<(Value, MessageFraming)>> {
    let mut first_line = String::new();
    loop {
        let bytes = reader.read_line(&mut first_line)?;
        if bytes == 0 {
            return Ok(None);
        }

        if first_line.trim().is_empty() {
            first_line.clear();
        } else {
            break;
        }
    }

    let trimmed = first_line.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        let message = serde_json::from_str(trimmed).context("Failed to decode MCP JSON line")?;
        return Ok(Some((message, MessageFraming::JsonLine)));
    }

    let mut content_length = parse_content_length_header(&first_line)?;
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            return Err(anyhow!("Unexpected EOF while reading MCP headers"));
        }

        // Retain the legacy Content-Length transport for existing Jig clients,
        // accepting both CRLF and LF-only header separators.
        if line == "\r\n" || line == "\n" {
            break;
        }

        if let Some(value) = parse_content_length_header(&line)? {
            content_length = Some(value);
        }
    }

    let content_length = content_length.ok_or_else(|| anyhow!("Missing Content-Length header"))?;
    let mut body = vec![0_u8; content_length];
    reader.read_exact(&mut body)?;
    let message = serde_json::from_slice(&body).context("Failed to decode MCP message body")?;
    Ok(Some((message, MessageFraming::ContentLength)))
}

fn parse_content_length_header(line: &str) -> Result<Option<usize>> {
    let Some((name, value)) = line.split_once(':') else {
        return Ok(None);
    };
    if !name.trim().eq_ignore_ascii_case("content-length") {
        return Ok(None);
    }

    Ok(Some(value.trim().parse::<usize>()?))
}

fn write_message(writer: &mut dyn Write, value: &Value, framing: MessageFraming) -> Result<()> {
    let body = serde_json::to_vec(value)?;
    if framing == MessageFraming::ContentLength {
        write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    }
    writer.write_all(&body)?;
    if framing == MessageFraming::JsonLine {
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::time::Duration;

    use serde_json::json;

    use super::{McpProgressObserver, MessageFraming, read_message, write_message};
    use crate::execution::{ExecutionEvent, ExecutionObserver, ExecutionStream, PhasePosition};

    #[test]
    fn read_message_accepts_json_line() {
        let input = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        })
        .to_string()
            + "\n";
        let mut reader = Cursor::new(input.into_bytes());

        let (message, framing) = read_message(&mut reader).unwrap().unwrap();

        assert_eq!(message["method"], "initialize");
        assert_eq!(framing, MessageFraming::JsonLine);
    }

    #[test]
    fn read_message_keeps_consecutive_json_lines_separate() {
        let first = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });
        let second = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });
        let input = format!("{first}\n{second}\n");
        let mut reader = Cursor::new(input.into_bytes());

        let (first_message, first_framing) = read_message(&mut reader).unwrap().unwrap();
        let (second_message, second_framing) = read_message(&mut reader).unwrap().unwrap();

        assert_eq!(first_message["id"], 1);
        assert_eq!(second_message["id"], 2);
        assert_eq!(first_framing, MessageFraming::JsonLine);
        assert_eq!(second_framing, MessageFraming::JsonLine);
    }

    #[test]
    fn read_message_accepts_lf_only_header_separator() {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        })
        .to_string();
        let input = format!("Content-Length: {}\n\n{body}", body.len());
        let mut reader = Cursor::new(input.into_bytes());

        let (message, framing) = read_message(&mut reader).unwrap().unwrap();

        assert_eq!(message["method"], "initialize");
        assert_eq!(framing, MessageFraming::ContentLength);
    }

    #[test]
    fn read_message_accepts_crlf_header_separator() {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        })
        .to_string();
        let input = format!("Content-Length: {}\r\n\r\n{body}", body.len());
        let mut reader = Cursor::new(input.into_bytes());

        let (message, framing) = read_message(&mut reader).unwrap().unwrap();

        assert_eq!(message["method"], "initialize");
        assert_eq!(framing, MessageFraming::ContentLength);
    }

    #[test]
    fn write_message_uses_json_line_framing() {
        let mut output = Vec::new();

        write_message(
            &mut output,
            &json!({"jsonrpc": "2.0", "id": 1, "result": {}}),
            MessageFraming::JsonLine,
        )
        .unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "{\"id\":1,\"jsonrpc\":\"2.0\",\"result\":{}}\n"
        );
    }

    #[test]
    fn write_message_preserves_content_length_framing() {
        let value = json!({"jsonrpc": "2.0", "id": 1, "result": {}});
        let body = serde_json::to_vec(&value).unwrap();
        let mut output = Vec::new();

        write_message(&mut output, &value, MessageFraming::ContentLength).unwrap();

        let expected = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        assert_eq!(&output[..expected.len()], expected);
        assert_eq!(&output[expected.len()..], body);
    }

    #[test]
    fn progress_observer_emits_standard_notification_with_call_token() {
        let mut output = Vec::new();
        {
            let mut observer = McpProgressObserver::new(
                &mut output,
                MessageFraming::JsonLine,
                Some(json!("request-progress")),
            );
            observer.event(ExecutionEvent::Heartbeat {
                label: "jig.test",
                elapsed: Duration::from_secs(25),
            });
            assert_eq!(
                observer.progress, 0,
                "progress must stay buffered during work"
            );
            observer.flush().unwrap();
        }

        let notification: serde_json::Value =
            serde_json::from_slice(output.strip_suffix(b"\n").unwrap()).unwrap();
        assert_eq!(notification["method"], "notifications/progress");
        assert_eq!(notification["params"]["progressToken"], "request-progress");
        assert_eq!(notification["params"]["progress"], 1);
        assert!(
            notification["params"]["message"]
                .as_str()
                .unwrap()
                .contains("reached 25s")
        );
    }

    #[test]
    fn progress_observer_is_silent_without_a_call_token() {
        let mut output = Vec::new();
        let mut observer = McpProgressObserver::new(&mut output, MessageFraming::JsonLine, None);
        observer.event(ExecutionEvent::Heartbeat {
            label: "jig.test",
            elapsed: Duration::from_secs(25),
        });
        observer.flush().unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn progress_observer_coalesces_noisy_output_into_one_bounded_preview() {
        let mut output = Vec::new();
        {
            let mut observer =
                McpProgressObserver::new(&mut output, MessageFraming::JsonLine, Some(json!(7)));
            observer.event(ExecutionEvent::PhaseStarted {
                label: "fixture",
                position: PhasePosition::single(),
            });
            for _ in 0..100 {
                observer.event(ExecutionEvent::Output {
                    stream: ExecutionStream::Stdout,
                    bytes: &[b'x'; 4_096],
                });
            }
            observer.event(ExecutionEvent::PhaseFinished {
                label: "fixture",
                success: true,
                elapsed: Duration::from_secs(1),
            });
            assert_eq!(observer.progress, 0, "progress must not write during work");
            observer.flush().unwrap();
        }

        let notifications = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(notifications.len(), 3);
        assert_eq!(
            notifications[0]["params"]["message"],
            "fixture started (1/1)"
        );
        assert_eq!(
            notifications[1]["params"]["message"],
            "fixture finished (1s)"
        );
        let output_message = notifications[2]["params"]["message"].as_str().unwrap();
        assert!(output_message.starts_with("stdout: "));
        assert!(output_message.ends_with(" [preview truncated]"));
        assert!(output_message.len() < 4_200);
    }
}
