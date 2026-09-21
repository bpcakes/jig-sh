use std::fs;
use std::io::{self, Write};
use std::process::Command;
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
use crate::surface::ResponseSurface;
use crate::test_env::TestRepoBuilder;

fn write_failing_phase_fixture(root: &std::path::Path) {
    fs::create_dir_all(root.join("api")).unwrap();
    fs::write(root.join("api/example.go"), "package example\n").unwrap();
    TestRepoBuilder::new(root)
        .contract_version(6)
        .config(
            r#"
[commands]
api_test_command = "exit 7"

[work]
iteration_profile = "iteration"

[repository]
default_check_profile = "iteration"

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
id = "iteration"
targets = [{ component = "api", action = "test" }]
"#,
        )
        .required_commands(["api_test_command"])
        .write();
    let manifest_path = root.join(".agent/jig-contract.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["components"] = json!([
        {"id": "api", "root": "api", "adapters": ["go"]}
    ]);
    manifest["actions"] = json!([{
        "target": {"component": "api", "action": "test"},
        "intent": "check",
        "effects": ["read_only", "process"],
        "runner": {"kind": "command", "command": "api_test_command"},
        "inputs": ["api/**"]
    }]);
    manifest["profiles"] = json!([{
        "id": "iteration",
        "targets": [{"component": "api", "action": "test"}]
    }]);
    manifest["default_check_profile"] = json!("iteration");
    fs::write(
        manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "agent@example.invalid"],
        vec!["config", "user.name", "Fixture Agent"],
        vec!["add", "."],
        vec!["commit", "-qm", "fixture"],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
    }
}

#[test]
fn failed_phase_check_sets_mcp_tool_error_and_preserves_the_report() {
    let temp = tempdir().unwrap();
    write_failing_phase_fixture(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    crate::state::seed_open_plan_for_test(&ctx, "plan_1", "Example plan", "Body").unwrap();
    let mut writer = Vec::new();

    let response = handle_tool_call(
        &ctx,
        Some(json!(1)),
        json!({
            "name": crate::tool_defs::tool::WORK_CHECK,
            "arguments": {"plan_id": "plan_1", "phase": "iteration"}
        }),
        &mut writer,
        MessageFraming::JsonLine,
        ResponseSurface::Standard,
    );

    assert!(response.get("error").is_none(), "{response:#}");
    assert_eq!(response["result"]["isError"], true, "{response:#}");
    assert_eq!(response["result"]["structuredContent"]["ok"], false);
    assert_eq!(
        response["result"]["structuredContent"]["phase"],
        "iteration"
    );
    let text: serde_json::Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text, response["result"]["structuredContent"]);
    assert!(writer.is_empty());

    let preview = handle_tool_call(
        &ctx,
        Some(json!(2)),
        json!({
            "name": crate::tool_defs::tool::WORK_CHECK,
            "arguments": {
                "plan_id": "plan_1",
                "phase": "iteration",
                "explain": true
            }
        }),
        &mut writer,
        MessageFraming::JsonLine,
        ResponseSurface::Standard,
    );
    assert_eq!(preview["result"]["isError"], false, "{preview:#}");
    assert_eq!(preview["result"]["structuredContent"]["ok"], true);
    assert_eq!(preview["result"]["structuredContent"]["selected_ok"], false);
}

#[test]
fn work_retire_error_exposes_partial_completion_over_mcp() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .repo_name("ExampleProject")
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut writer = Vec::new();
    let started = handle_tool_call(
        &ctx,
        Some(json!(1)),
        json!({
            "name": "jig.work_start", "arguments": {"title": "Example closure", "body": "Validate publication failure."}
        }),
        &mut writer,
        MessageFraming::JsonLine,
        ResponseSurface::Standard,
    );
    let plan_id = started["result"]["structuredContent"]["plan"]["plan_id"]
        .as_str()
        .unwrap();
    let lock_path = temp
        .path()
        .join(".agent/.cache/state-locks/receipts.jsonl.lock");
    fs::remove_file(&lock_path).unwrap();
    fs::create_dir(&lock_path).unwrap();
    let response = handle_tool_call(
        &ctx,
        Some(json!(2)),
        json!({
            "name": "jig.work_retire", "arguments": {"plan_id": plan_id, "disposition": "obsolete", "reason": "No longer needed."}
        }),
        &mut writer,
        MessageFraming::JsonLine,
        ResponseSurface::Standard,
    );
    assert_eq!(response["error"]["code"], -32000, "{response:#}");
    let partial = &response["error"]["data"]["partial_completion"];
    assert_eq!(partial["plan_id"], plan_id);
    assert_eq!(partial["plan_state"], "closed");
    assert_eq!(partial["receipt"]["status"], "not_recorded");
    assert_eq!(partial["session_teardown"]["status"], "not_attempted");
    assert!(partial["close_event_id"].is_string());
    assert!(response["result"].is_null());
}
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

    let response = handle_tools_list(&ctx, Some(json!(1)), ResponseSurface::Standard);
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
        ResponseSurface::Standard,
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
