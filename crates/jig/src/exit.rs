//! The one error type that carries a decided process exit.

use std::fmt;

/// A command failure whose process exit status is already decided.
///
/// A *reported* exit means the command already told the user what happened in
/// its own protocol, such as a structured JSON document or a child process's
/// own output, so the binary exits with the status without printing anything.
/// An *unreported* exit still has the binary print its message first.
///
/// A command that propagates a child status or a structured failure returns
/// this type; nothing else has to change for the status to reach `main`.
#[derive(Debug)]
pub(crate) struct CliExit {
    code: i32,
    reported: bool,
    message: String,
}

impl CliExit {
    /// A failure the command already reported; exit with `code` silently.
    pub(crate) fn reported(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            reported: true,
            message: message.into(),
        }
    }

    /// A failure the binary still prints before exiting with `code`.
    pub(crate) fn unreported(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            reported: false,
            message: message.into(),
        }
    }

    /// The decided exit carried by `error`, if it is one.
    pub(crate) fn of(error: &anyhow::Error) -> Option<&Self> {
        error.downcast_ref()
    }

    pub(crate) const fn code(&self) -> i32 {
        self.code
    }

    pub(crate) const fn is_reported(&self) -> bool {
        self.reported
    }
}

impl fmt::Display for CliExit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CliExit {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reported_exit_is_silent_and_keeps_its_status() {
        let error: anyhow::Error = CliExit::reported(37, "child exited with status 37").into();
        let exit = CliExit::of(&error).unwrap();

        assert!(exit.is_reported());
        assert_eq!(exit.code(), 37);
        assert_eq!(error.to_string(), "child exited with status 37");
    }

    #[test]
    fn an_unreported_exit_keeps_its_message_for_the_binary_to_print() {
        let error: anyhow::Error = CliExit::unreported(2, "--max-candidates is invalid").into();
        let exit = CliExit::of(&error).unwrap();

        assert!(!exit.is_reported());
        assert_eq!(exit.code(), 2);
        assert_eq!(format!("{error:#}"), "--max-candidates is invalid");
    }

    #[test]
    fn ordinary_errors_carry_no_decided_exit() {
        assert!(CliExit::of(&anyhow::anyhow!("configuration is invalid")).is_none());
    }

    #[test]
    fn context_does_not_hide_a_decided_exit() {
        let error = anyhow::Error::from(CliExit::reported(3, "reported")).context("while running");

        assert_eq!(CliExit::of(&error).map(CliExit::code), Some(3));
    }
}
