use anyhow::{Context, Result};
use serde_json::Value;

pub(in crate::runtime) fn manifest_tool_result_failure(
    response: &Value,
) -> Result<Option<(i32, String)>> {
    let tool_name = response
        .get("tool")
        .and_then(Value::as_str)
        .context("Tool execution response is missing `tool`")?;
    let result = response
        .get("result")
        .context("Tool execution response is missing `result`")?;
    let exit_status = result
        .get("exit_status")
        .and_then(Value::as_i64)
        .and_then(|status| i32::try_from(status).ok())
        .context("Tool execution response has an invalid `result.exit_status`")?;
    let stdout = result
        .get("stdout")
        .and_then(Value::as_str)
        .context("Tool execution response is missing `result.stdout`")?;
    let stderr = result
        .get("stderr")
        .and_then(Value::as_str)
        .context("Tool execution response is missing `result.stderr`")?;
    let command_key = response.get("command_key").and_then(Value::as_str);

    Ok(
        tool_failure_message(tool_name, command_key, exit_status, stdout, stderr)
            .map(|message| (exit_status, message)),
    )
}

pub(super) fn tool_failure_message(
    tool_name: &str,
    command_key: Option<&str>,
    exit_status: i32,
    stdout: &str,
    stderr: &str,
) -> Option<String> {
    if exit_status == 0 {
        return None;
    }
    Some(match command_key {
        Some(command_key) => format!(
            "{tool_name} failed with status {exit_status}\ncommand key: {command_key}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        ),
        None => format!(
            "{tool_name} failed with status {exit_status}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        ),
    })
}
