use std::io;
use std::process::{Command, ExitStatus};

use anyhow::{Context, Result};

use crate::exit::CliExit;

/// Executes a provider-prepared command with inherited streams and working directory.
pub(crate) fn launch(
    command: &mut Command,
    agent: &'static str,
    error_context: impl FnOnce() -> String,
) -> Result<()> {
    let status = execute(command).with_context(error_context)?;
    if !status.success() {
        return Err(child_exit(agent, status.code().unwrap_or(1).clamp(1, 255)).into());
    }
    Ok(())
}

/// The agent owned the terminal, so its failure is already visible; only its
/// status is propagated.
fn child_exit(agent: &str, status: i32) -> CliExit {
    CliExit::reported(status, format!("{agent} exited with status {status}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_child_exit_is_silent_and_preserves_the_status() {
        for agent in ["Claude", "Codex"] {
            let error: anyhow::Error = child_exit(agent, 37).into();
            let exit = CliExit::of(&error).unwrap();

            assert!(exit.is_reported());
            assert_eq!(exit.code(), 37);
            assert_eq!(error.to_string(), format!("{agent} exited with status 37"));
        }
    }
}
