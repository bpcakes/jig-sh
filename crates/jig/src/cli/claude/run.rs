use super::{ClaudeCommand, render};
use crate::claude::provider::Claude;
use crate::cli::agent_run;
use anyhow::Result;

pub(in crate::cli) fn run_claude_command(command: ClaudeCommand, json_output: bool) -> Result<()> {
    match command {
        ClaudeCommand::Homes(opts) => {
            agent_run::homes(&Claude, opts.usage, json_output, render::homes_summary)
        }
        ClaudeCommand::Launch(opts) => agent_run::launch(
            &Claude,
            opts.home.as_deref(),
            &opts.claude_args,
            opts.dry_run,
            json_output,
            render::launch_summary,
        ),
    }
}
