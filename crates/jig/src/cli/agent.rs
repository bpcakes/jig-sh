use clap::{Args, Subcommand};

use crate::tool_defs;

pub(super) const AGENT_AFTER_HELP: &str = "\
Human-readable output is the default. Pass --json for structured automation output.

Examples:
  jig agent doctor
  jig agent doctor --json
  jig agent bootstrap";

pub(super) const AGENT_BOOTSTRAP_AFTER_HELP: &str = "\
Use --marketplace for a GitHub owner/repo skill marketplace or another configured marketplace source.

Examples:
  jig agent bootstrap
  jig agent bootstrap --marketplace owner/skills-repo";

#[derive(Debug, Subcommand)]
pub(crate) enum AgentCommand {
    /// Report local Codex marketplace readiness for this repo.
    #[command(name = tool_defs::cli_command::AGENT_DOCTOR)]
    Doctor,
    /// Register the configured Codex skills marketplace.
    #[command(
        name = tool_defs::cli_command::AGENT_BOOTSTRAP,
        after_help = AGENT_BOOTSTRAP_AFTER_HELP
    )]
    Bootstrap(AgentBootstrapOpts),
}

#[derive(Args, Debug)]
pub(crate) struct AgentBootstrapOpts {
    #[arg(
        long,
        help = "Marketplace source to register; defaults to the single configured source"
    )]
    pub(crate) marketplace: Option<String>,
}
