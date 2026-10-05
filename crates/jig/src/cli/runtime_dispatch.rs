//! The one path from a parsed command to `runtime::dispatch`.
//!
//! A command family describes its dispatch as data: the runtime command, the
//! function that renders its result for people, and whether `ok: false` in
//! the result fails the command. [`dispatch_runtime`] adds repository loading, signal
//! supervision, progress reporting, and output.

use anyhow::Result;

use super::output::{self, Render, emit};
use super::run::finish_after_json_output;
use super::structured_error::require_json_ok;
use crate::command::RuntimeCommand;
use crate::context::RepoContext;
use crate::runtime;

/// How a runtime result decides the exit status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FailurePolicy {
    /// `ok: false` in the result fails the command.
    OkFalseFails,
    /// The result is a report that may carry `ok: false`; only an error fails
    /// the command.
    ErrorsOnly,
}

/// A runtime dispatch as described by the command family that owns it.
pub(super) struct RuntimeDispatch {
    pub(super) command: RuntimeCommand,
    pub(super) render: Render,
    pub(super) failure: FailurePolicy,
}

impl RuntimeDispatch {
    /// A report rendered with `render` that fails only on an error.
    pub(super) const fn new(command: RuntimeCommand, render: Render) -> Self {
        Self {
            command,
            render,
            failure: FailurePolicy::ErrorsOnly,
        }
    }

    /// A manifest-tool execution with the shared tool summary.
    pub(super) const fn tool(command: RuntimeCommand) -> Self {
        Self::new(command, output::format_tool_execution_summary)
    }

    /// Also fail the command when the result reports `ok: false`.
    pub(super) const fn failing_on_ok_false(mut self) -> Self {
        self.failure = FailurePolicy::OkFalseFails;
        self
    }
}

pub(super) fn dispatch_runtime(dispatch: RuntimeDispatch, json_output: bool) -> Result<()> {
    let RuntimeDispatch {
        command,
        render,
        failure,
    } = dispatch;
    let require_ok = failure == FailurePolicy::OkFalseFails;
    let ctx = RepoContext::load()?;
    #[cfg(all(unix, not(test)))]
    if command.signal_policy() == crate::command::RuntimeSignalPolicy::Native {
        let output = runtime::dispatch(&ctx, command)?;
        emit(json_output, render, &output)?;
        return finish_after_json_output(require_json_ok(require_ok, &output), json_output);
    }
    #[cfg(all(unix, not(test)))]
    let signal_session = crate::signal_supervision::SignalSession::start().map_err(|_| {
        anyhow::anyhow!("Command was not started because signal supervision is unavailable")
    })?;
    #[cfg(all(unix, not(test)))]
    let cancellation = signal_session.cancellation();
    #[cfg(all(unix, not(test)))]
    let mut observer =
        crate::progress::CliExecutionObserver::with_cancellation(json_output, move || {
            cancellation.cancelled()
        });
    #[cfg(any(not(unix), test))]
    let mut observer = crate::progress::CliExecutionObserver::for_human_output(json_output);
    let outcome = runtime::dispatch_with_observer(&ctx, command, &mut observer);
    let outcome = observer.finish_with(outcome);
    #[cfg(all(unix, not(test)))]
    let outcome = crate::signal_supervision::finish(
        outcome,
        signal_session.finish(),
        "Command signal supervision could not retire safely",
    );
    let output = outcome?;
    emit(json_output, render, &output)?;
    finish_after_json_output(require_json_ok(require_ok, &output), json_output)
}
