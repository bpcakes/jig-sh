use std::io;
use std::process::{Command, ExitStatus};

use anyhow::{Context, Result};

/// Executes a provider-prepared command with inherited streams and working directory.
pub(crate) fn launch(
    command: &mut Command,
    agent: &'static str,
    error_context: impl FnOnce() -> String,
) -> Result<()> {
    let status = execute(command).with_context(error_context)?;
    if !status.success() {
        return Err(AgentChildExitStatus {
            agent,
            status: status.code().unwrap_or(1).clamp(1, 255),
        }
        .into());
    }
    Ok(())
}

#[cfg(unix)]
fn execute(command: &mut Command) -> io::Result<ExitStatus> {
    use std::os::unix::process::CommandExt;

    Err(command.exec())
}

#[cfg(not(unix))]
fn execute(command: &mut Command) -> io::Result<ExitStatus> {
    command.status()
}

#[derive(Debug)]
pub(crate) struct AgentChildExitStatus {
    pub(crate) agent: &'static str,
    pub(crate) status: i32,
}

impl std::fmt::Display for AgentChildExitStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} exited with status {}",
            self.agent, self.status
        )
    }
}

impl std::error::Error for AgentChildExitStatus {}
