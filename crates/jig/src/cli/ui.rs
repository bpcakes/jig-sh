use std::time::Duration;

use anyhow::Result;
use clap::Args;

use super::structured_error::{json_command_error, json_output_already_emitted};
use crate::context::RepoContext;
use crate::{root_commands, ui};

pub(super) const UI_AFTER_HELP: &str = "\
Opens a read-only terminal dashboard over repository status and .agent/state:
run history, loops, repository state, and activity.
Interactive mode requires terminal stdin and stdout.

Pass --json for one local recorder snapshot.

Examples:
  jig ui
  jig ui --timeline-limit 120
  jig ui --json";

#[derive(Args, Debug)]
pub(crate) struct UiOpts {
    #[arg(
        long,
        value_name = "SECONDS",
        value_parser = clap::value_parser!(u64).range(1..=3600),
        help = "Read-only dashboard refresh interval; defaults to 10 seconds"
    )]
    pub(crate) refresh_seconds: Option<u64>,
    #[arg(
        long,
        value_name = "ROWS",
        value_parser = clap::value_parser!(u64).range(1..=1000),
        help = "Initial activity rows for the TUI or recorder JSON; defaults to 120"
    )]
    pub(crate) timeline_limit: Option<u64>,
    #[arg(long = "port", hide = true)]
    pub(crate) retired_port: Option<u16>,
}

impl UiOpts {
    /// Option combinations Clap cannot reject because they involve the
    /// global `--json` flag or a retired option that still parses.
    pub(crate) const fn usage_conflict(&self, json: bool) -> Option<&'static str> {
        if self.retired_port.is_some() {
            Some(
                "the `jig ui` browser server and `--port` option were removed in 0.3.0; use `jig ui` for the terminal dashboard or `jig ui --json` for one-shot data (`--port` will stop parsing in 0.4.0)",
            )
        } else if json && self.refresh_seconds.is_some() {
            Some("`--refresh-seconds` cannot be combined with `--json`")
        } else {
            None
        }
    }

    pub(crate) fn effective_refresh_seconds(&self) -> u64 {
        self.refresh_seconds.unwrap_or(10)
    }

    pub(crate) fn effective_timeline_limit(&self) -> u64 {
        self.timeline_limit.unwrap_or(120)
    }
}

pub(super) fn run_ui_command(opts: UiOpts, json_output: bool) -> Result<()> {
    if !json_output {
        return ui::run(
            RepoContext::load()?,
            ui::DashboardRequest {
                timeline_limit: opts.effective_timeline_limit(),
                refresh_interval: Duration::from_secs(opts.effective_refresh_seconds()),
            },
        );
    }
    let ctx = RepoContext::load().map_err(name_ui_error)?;
    ui::write_recorder_json(ctx, opts.effective_timeline_limit()).map_err(recorder_json_error)
}

/// Names `ui` in the structured error document for a failure before output.
pub(super) fn name_ui_error(error: anyhow::Error) -> anyhow::Error {
    json_command_error(root_commands::UI.name, error)
}

fn recorder_json_error(failure: ui::RecorderJsonError) -> anyhow::Error {
    if failure.output_started {
        json_output_already_emitted(failure.error)
    } else {
        name_ui_error(failure.error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::structured_error::{JsonCommandError, is_json_output_already_emitted};

    #[test]
    fn recorder_failures_after_output_are_marked_as_already_emitted() {
        let error = recorder_json_error(ui::RecorderJsonError {
            error: anyhow::anyhow!("retirement failed"),
            output_started: true,
        });
        assert!(is_json_output_already_emitted(&error));
    }

    #[test]
    fn recorder_failures_before_output_name_the_ui_command() {
        let error = recorder_json_error(ui::RecorderJsonError {
            error: anyhow::anyhow!("collection failed"),
            output_started: false,
        });
        assert!(!is_json_output_already_emitted(&error));
        assert_eq!(
            error.downcast_ref::<JsonCommandError>().unwrap().command,
            "ui"
        );
    }
}
