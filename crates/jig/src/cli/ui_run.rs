use std::time::Duration;

use anyhow::Result;

use super::UiOpts;
use super::structured_error::{json_command_error, json_output_already_emitted};
use crate::context::RepoContext;
use crate::{root_commands, ui};

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
