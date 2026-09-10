use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, Subcommand};

pub(super) const AFTER_HELP: &str = "\
Claude homes are separate CLAUDE_CONFIG_DIR directories.
Discover ~/.claude, ~/.claude-*, and the current CLAUDE_CONFIG_DIR.

Examples:
  jig claude homes
  jig claude homes --usage
  jig claude launch
  jig claude launch work -- --model sonnet
  jig claude launch default -- auth login";

#[derive(Debug, Subcommand)]
pub(crate) enum ClaudeCommand {
    /// List Claude homes; optionally read subscription usage limits.
    Homes(ClaudeHomesOpts),
    /// Select a Claude home and launch Claude Code.
    #[command(
        after_help = "A bare HOME such as work selects ~/.claude-work; claude and default select\n~/.claude with CLAUDE_CONFIG_DIR unset, preserving Claude's default global config.\nThe native default works before ~/.claude exists; Claude creates it as needed.\nExplicit paths always set CLAUDE_CONFIG_DIR and must already exist. Use ./work\nfor a relative path. Omit HOME for the searchable full-screen home picker.\nSubscription limits load in the background; Enter can launch while loading.\nOn macOS the picker may ask for Keychain access (30-second timeout).\nArrows or j/k move, / searches, Tab switches to details, Enter launches,\nand Esc or q cancels. Arguments after -- are forwarded exactly to Claude.\n\nAfter upgrading Jig in an existing repository, refresh its generated launcher\nwith scripts/jig update --recopy. Older launchers change to the repository root\nbefore launching Claude, which also changes how relative HOME paths resolve."
    )]
    Launch(ClaudeLaunchOpts),
}

#[derive(Args, Debug)]
pub(crate) struct ClaudeHomesOpts {
    /// Fetch subscription limits using each home's saved login (no login prompts).
    #[arg(long)]
    pub(crate) usage: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ClaudeLaunchOpts {
    /// Claude home name or path; omit to choose interactively.
    #[arg(value_name = "HOME")]
    pub(crate) home: Option<PathBuf>,
    /// Print the selected home and command without launching Claude.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Arguments forwarded exactly to Claude after --.
    #[arg(last = true, allow_hyphen_values = true, value_name = "CLAUDE_ARGS")]
    pub(crate) claude_args: Vec<OsString>,
}
