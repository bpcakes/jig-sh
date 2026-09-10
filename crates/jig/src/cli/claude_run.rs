use super::claude::ClaudeCommand;
use super::output::HumanOutput;
use crate::claude::provider::Claude;
use anyhow::Result;

pub(super) fn run_claude_command(command: ClaudeCommand, json_output: bool) -> Result<()> {
    match command {
        ClaudeCommand::Homes(opts) => {
            super::agent_run::homes(&Claude, opts.usage, json_output, HumanOutput::ClaudeHomes)
        }
        ClaudeCommand::Launch(opts) => super::agent_run::launch(
            &Claude,
            opts.home.as_deref(),
            &opts.claude_args,
            opts.dry_run,
            json_output,
            HumanOutput::ClaudeLaunch,
        ),
    }
}
