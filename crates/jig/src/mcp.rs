use std::io::{self, BufRead, Write};

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::runtime::call_tool;
use crate::tool_defs;

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
                    "tools": tool_defs::tool_descriptors(ctx.contract_version(), ctx.tool_specs())
                }
            })),
            "tools/call" => Some(handle_tool_call(ctx, id, params)),
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

fn handle_tool_call(ctx: &RepoContext, id: Option<Value>, params: Value) -> Value {
    let result = (|| -> Result<Value> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("tools/call requires params.name"))?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let tool_result = call_tool(ctx, name, args)?;
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

    use serde_json::json;

    use super::{MessageFraming, read_message, write_message};

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
}
