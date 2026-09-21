use std::fs;
use std::io::Cursor;

use serde_json::{Value, json};
use tempfile::tempdir;

use super::super::{MessageFraming, read_frame, serve_messages, write_message};
use crate::context::RepoContext;
use crate::surface::ResponseSurface;
use crate::test_env::TestRepoBuilder;

fn write_repository_fixture(root: &std::path::Path) {
    TestRepoBuilder::new(root)
        .contract_version(6)
        .config(
            r#"
[commands]
api_test_command = "printf passed"

[repository]
default_check_profile = "verify"

[[repository.components]]
id = "api"
root = "api"
adapters = ["go"]

[[repository.actions]]
target = { component = "api", action = "test" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "api_test_command" }
inputs = ["api/**"]

[[repository.profiles]]
id = "verify"
targets = [{ component = "api", action = "test" }]
"#,
        )
        .required_commands(["api_test_command"])
        .write();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 6,
            "tool_namespace": "jig",
            "required_commands": ["api_test_command"],
            "tools": [],
            "components": [{"id": "api", "root": "api", "adapters": ["go"]}],
            "actions": [{
                "target": {"component": "api", "action": "test"},
                "intent": "check",
                "effects": ["read_only", "process"],
                "runner": {"kind": "command", "command": "api_test_command"},
                "inputs": ["api/**"]
            }],
            "profiles": [{
                "id": "verify",
                "targets": [{"component": "api", "action": "test"}]
            }],
            "default_check_profile": "verify"
        }))
        .unwrap(),
    )
    .unwrap();
}

fn listed_inspect_schema(ctx: &RepoContext, surface: ResponseSurface) -> Value {
    let wire = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\",\"params\":{}}\n";
    let mut output = Vec::new();
    serve_messages(ctx, &mut Cursor::new(wire), &mut output, surface).unwrap();
    let mut responses = Cursor::new(output);
    let (response, _) = read_frame(&mut responses).unwrap().unwrap();
    serde_json::from_slice::<Value>(&response).unwrap()["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "jig.inspect")
        .unwrap()["outputSchema"]
        .clone()
}

fn contains_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(object) => {
            object.contains_key(key) || object.values().any(|value| contains_key(value, key))
        }
        Value::Array(values) => values.iter().any(|value| contains_key(value, key)),
        _ => false,
    }
}

#[test]
fn tools_list_uses_the_process_selected_inspection_schema() {
    let temp = tempdir().unwrap();
    write_repository_fixture(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let standard = listed_inspect_schema(&ctx, ResponseSurface::Standard);
    let agent = listed_inspect_schema(&ctx, ResponseSurface::AgentV1);

    assert!(!contains_key(&standard, "freshness_policy"));
    assert!(contains_key(&agent, "freshness_policy"));
}

#[test]
fn rejected_payloads_return_parse_errors_and_preserve_the_session() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config("[commands]\nfixture_command = \"printf executed > rejected-tool-ran\"\n")
        .required_commands(["fixture_command"])
        .tool(json!({
            "name": "jig.fixture",
            "kind": "command",
            "description": "Record fixture execution.",
            "command": "fixture_command"
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let ping = r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#;
    for bad in [
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"jig.fixture","arguments":{"message":"a","message":"b"}}}"#,
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"jig.fixture","arguments":{"arguments":{"api:generate":{},"api:generate":{}}}}}"#,
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"jig.fixture"}} trailing"#,
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"jig.fixture"}"#,
    ] {
        for framing in [MessageFraming::JsonLine, MessageFraming::ContentLength] {
            let wire = match framing {
                MessageFraming::JsonLine => format!("{bad}\n{ping}\n"),
                MessageFraming::ContentLength => format!(
                    "Content-Length: {}\r\n\r\n{bad}Content-Length: {}\r\n\r\n{ping}",
                    bad.len(),
                    ping.len()
                ),
            };
            let mut output = Vec::new();
            serve_messages(
                &ctx,
                &mut Cursor::new(wire),
                &mut output,
                ResponseSurface::Standard,
            )
            .unwrap();

            let mut responses = Cursor::new(output);
            let (error, error_framing) = read_frame(&mut responses).unwrap().unwrap();
            assert_eq!(error_framing, framing);
            assert_eq!(
                serde_json::from_slice::<Value>(&error).unwrap(),
                json!({
                    "jsonrpc": "2.0", "id": null,
                    "error": { "code": -32700, "message": "Parse error" }
                })
            );
            let (reply, reply_framing) = read_frame(&mut responses).unwrap().unwrap();
            assert_eq!(reply_framing, framing);
            assert_eq!(
                serde_json::from_slice::<Value>(&reply).unwrap(),
                json!({
                    "jsonrpc": "2.0", "id": 2, "result": {}
                })
            );
            assert!(read_frame(&mut responses).unwrap().is_none());
            assert!(!temp.path().join("rejected-tool-ran").exists());
        }
    }
}

#[test]
fn incomplete_or_invalid_framing_remains_fatal() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    for wire in [
        "Content-Length: invalid\r\n\r\n{}",
        "Content-Type: application/json\r\n\r\n{}",
        "Content-Length: 2\r\n",
        "Content-Length: 10\r\n\r\n{}",
    ] {
        let mut output = Vec::new();
        assert!(
            serve_messages(
                &ctx,
                &mut Cursor::new(wire),
                &mut output,
                ResponseSurface::Standard,
            )
            .is_err()
        );
        assert!(output.is_empty());
    }
}

#[test]
fn read_frame_accepts_json_line() {
    let input = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    })
    .to_string()
        + "\n";
    let mut reader = Cursor::new(input.into_bytes());

    let (body, framing) = read_frame(&mut reader).unwrap().unwrap();
    let message: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(message["method"], "initialize");
    assert_eq!(framing, MessageFraming::JsonLine);
}

#[test]
fn read_frame_keeps_consecutive_json_lines_separate() {
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

    let (first_body, first_framing) = read_frame(&mut reader).unwrap().unwrap();
    let (second_body, second_framing) = read_frame(&mut reader).unwrap().unwrap();

    let first_message: serde_json::Value = serde_json::from_slice(&first_body).unwrap();
    let second_message: serde_json::Value = serde_json::from_slice(&second_body).unwrap();
    assert_eq!(first_message["id"], 1);
    assert_eq!(second_message["id"], 2);
    assert_eq!(first_framing, MessageFraming::JsonLine);
    assert_eq!(second_framing, MessageFraming::JsonLine);
}

#[test]
fn read_frame_accepts_lf_only_header_separator() {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    })
    .to_string();
    let input = format!("Content-Length: {}\n\n{body}", body.len());
    let mut reader = Cursor::new(input.into_bytes());

    let (body, framing) = read_frame(&mut reader).unwrap().unwrap();
    let message: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(message["method"], "initialize");
    assert_eq!(framing, MessageFraming::ContentLength);
}

#[test]
fn read_frame_accepts_crlf_header_separator() {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    })
    .to_string();
    let input = format!("Content-Length: {}\r\n\r\n{body}", body.len());
    let mut reader = Cursor::new(input.into_bytes());

    let (body, framing) = read_frame(&mut reader).unwrap().unwrap();
    let message: serde_json::Value = serde_json::from_slice(&body).unwrap();

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
