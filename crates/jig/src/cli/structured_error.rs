//! The CLI's `--json` error protocol and the output checks that turn a
//! command result into a decided [`CliExit`].

use anyhow::Result;

use crate::exit::CliExit;

/// The command's JSON document is already on stdout; report the wrapped error
/// on stderr only, never as a second document.
#[derive(Debug)]
struct JsonOutputAlreadyEmitted(anyhow::Error);

/// A failure before any output that the JSON error document names by command.
#[derive(Debug)]
pub(super) struct JsonCommandError {
    pub(super) command: &'static str,
    message: String,
}

impl std::fmt::Display for JsonOutputAlreadyEmitted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#}", self.0)
    }
}

impl std::error::Error for JsonOutputAlreadyEmitted {}

impl std::fmt::Display for JsonCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for JsonCommandError {}

pub(super) fn json_error_payload(
    kind: &'static str,
    message: &str,
    exit_status: i32,
) -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "error": {
            "kind": kind,
            "message": message,
        },
        "exit_status": exit_status,
    })
}

/// The exit after a JSON error document has been printed.
pub(super) fn json_reported_error(exit_status: i32) -> anyhow::Error {
    CliExit::reported(
        exit_status,
        format!("JSON error response reported with exit status {exit_status}"),
    )
    .into()
}

pub(super) fn json_output_already_emitted(error: anyhow::Error) -> anyhow::Error {
    JsonOutputAlreadyEmitted(error).into()
}

pub(super) fn json_command_error(command: &'static str, error: anyhow::Error) -> anyhow::Error {
    JsonCommandError {
        command,
        message: format!("{error:#}"),
    }
    .into()
}

pub(super) fn is_json_output_already_emitted(error: &anyhow::Error) -> bool {
    error.is::<JsonOutputAlreadyEmitted>()
}

pub(super) fn require_json_ok(required: bool, output: &serde_json::Value) -> Result<()> {
    if required && output.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
        return Err(CliExit::reported(1, "Command reported ok=false").into());
    }
    Ok(())
}

pub(super) fn require_foreground_status(output: &serde_json::Value) -> Result<()> {
    if output
        .get("interrupted")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        let exit_status = output
            .get("exit_status")
            .and_then(serde_json::Value::as_i64)
            .filter(|status| (1..=255).contains(status))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "interrupted foreground result is missing a valid shell exit_status"
                )
            })?;
        return Err(CliExit::reported(
            exit_status as i32,
            format!("Foreground process interrupted with status {exit_status}"),
        )
        .into());
    }
    require_json_ok(true, output)
}

/// Whether the failure was already reported in the command's own protocol, so
/// neither `main` nor the JSON error reporter may report it again.
pub(super) fn is_structured_json_failure(error: &anyhow::Error) -> bool {
    CliExit::of(error).is_some_and(CliExit::is_reported)
}

#[cfg(test)]
pub(super) fn structured_error_exit_code(error: &anyhow::Error) -> Option<i32> {
    CliExit::of(error).map(CliExit::code)
}
