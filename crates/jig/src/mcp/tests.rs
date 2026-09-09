use std::fs;
use std::io::{self, Write};
use std::time::Duration;

use anyhow::anyhow;
use serde_json::json;
use tempfile::tempdir;

use super::{
    MCP_PROGRESS_EVENT_LIMIT, McpProgressObserver, MessageFraming,
    combine_tool_and_progress_results, handle_tool_call, handle_tools_list,
};
use crate::context::RepoContext;
use crate::execution::{ExecutionEvent, ExecutionObserver, ExecutionStream, PhasePosition};
use crate::test_env::TestRepoBuilder;
#[test]
fn tools_list_refreshes_manifest_tools_after_server_start() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"[commands]
custom_check_command = "printf 'check\n'"
"#,
        )
        .required_commands(["custom_check_command"])
        .tool(json!({
            "name": "jig.old_check",
            "kind": "command",
            "description": "Run the old check name.",
            "command": "custom_check_command"
        }))
        .write();
    let ctx = crate::context::RepoContext::load_from(temp.path()).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["tools"][0]["name"] = json!("jig.new_check");
    fs::write(
        manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let response = handle_tools_list(&ctx, Some(json!(1)));
    let names = response["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();

    assert!(names.contains(&"jig.new_check"), "{response:#}");
    assert!(!names.contains(&"jig.old_check"), "{response:#}");
}
#[test]
fn tool_and_progress_results_preserve_success_and_individual_failures() {
    assert_eq!(
        combine_tool_and_progress_results(Ok("result"), Ok(())).unwrap(),
        "result"
    );

    let tool_error =
        combine_tool_and_progress_results::<()>(Err(anyhow!("fixture tool failed")), Ok(()))
            .unwrap_err()
            .to_string();
    assert_eq!(tool_error, "fixture tool failed");

    let progress_error =
        combine_tool_and_progress_results(Ok(()), Err(anyhow!("fixture progress failed")))
            .unwrap_err()
            .to_string();
    assert_eq!(progress_error, "fixture progress failed");
}

#[test]
fn tool_failure_remains_primary_when_progress_delivery_also_fails() {
    let error = combine_tool_and_progress_results::<()>(
        Err(anyhow!("fixture tool failed")),
        Err(anyhow!("fixture progress failed")),
    )
    .unwrap_err()
    .to_string();

    assert!(error.starts_with("fixture tool failed"), "{error}");
    assert!(
        error.contains("MCP progress delivery also failed: fixture progress failed"),
        "{error}"
    );
}

#[test]
fn tool_call_keeps_tool_failure_primary_when_progress_flush_fails() {
    struct FailingProgressWriter;

    impl Write for FailingProgressWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("fixture progress sink failed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
[commands]
fixture_check_command = "printf 'fixture tool failed\n' >&2; exit 7"
"#,
        )
        .required_commands(["fixture_check_command"])
        .tool(json!({
            "name": "jig.fixture_check",
            "kind": "command",
            "description": "Run the fixture check.",
            "command": "fixture_check_command"
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let response = handle_tool_call(
        &ctx,
        Some(json!(1)),
        json!({
            "name": "jig.fixture_check",
            "arguments": {},
            "_meta": { "progressToken": "fixture-progress" }
        }),
        &mut FailingProgressWriter,
        MessageFraming::JsonLine,
    );
    let message = response["error"]["message"].as_str().unwrap();

    assert!(
        message.starts_with("jig.fixture_check failed with status 7"),
        "{message}"
    );
    assert!(message.contains("fixture tool failed"), "{message}");
    assert!(
            message.contains(
                "MCP progress delivery also failed: Failed to send MCP progress notification: fixture progress sink failed"
            ),
            "{message}"
        );
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

#[test]
fn progress_observer_flushes_only_output_queued_since_the_previous_flush() {
    let mut output = Vec::new();
    {
        let mut observer =
            McpProgressObserver::new(&mut output, MessageFraming::JsonLine, Some(json!(7)));
        observer.event(ExecutionEvent::Output {
            stream: ExecutionStream::Stderr,
            bytes: b"first batch\n",
        });
        observer.flush().unwrap();
        observer.event(ExecutionEvent::Output {
            stream: ExecutionStream::Stderr,
            bytes: b"second batch\n",
        });
        observer.flush().unwrap();
    }

    let notifications = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(notifications.len(), 2, "{notifications:#?}");
    assert_eq!(notifications[0]["params"]["message"], "stderr: first batch");
    assert_eq!(
        notifications[1]["params"]["message"],
        "stderr: second batch"
    );
}

#[test]
fn progress_observer_resets_event_truncation_after_each_flush() {
    let mut output = Vec::new();
    {
        let mut observer =
            McpProgressObserver::new(&mut output, MessageFraming::JsonLine, Some(json!(7)));
        for _ in 0..=MCP_PROGRESS_EVENT_LIMIT {
            observer.event(ExecutionEvent::Heartbeat {
                label: "first batch",
                elapsed: Duration::ZERO,
            });
        }
        observer.flush().unwrap();
        observer.event(ExecutionEvent::Heartbeat {
            label: "second batch",
            elapsed: Duration::ZERO,
        });
        observer.flush().unwrap();
    }

    let messages = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["params"]["message"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        messages
            .iter()
            .filter(|message| *message == "additional progress events omitted")
            .count(),
        1,
        "{messages:#?}"
    );
    assert_eq!(messages.last().unwrap(), "second batch reached 0s");
}

mod transport;
