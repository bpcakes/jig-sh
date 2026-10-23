use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, Subcommand};

pub(super) mod render;
pub(super) mod run;

pub(super) const AFTER_HELP: &str = "\
Claude homes are separate CLAUDE_CONFIG_DIR directories.
Discover ~/.claude, ~/.claude-*, and the current CLAUDE_CONFIG_DIR.

Examples:
  jig claude homes
  jig claude homes --usage
  jig claude launch
  jig claude launch work -- --model sonnet
  jig claude launch default -- auth login
  jig claude resume 2f4c6e8a-1b3d-4f5a-8c7e-9d0b1a2c3e4f";

pub(super) const RESUME_AFTER_HELP: &str = "\
Jig looks for SESSION_ID among the transcripts of every discovered Claude home,
then launches `claude --resume SESSION_ID` with the single matching home. A session
in ~/.claude keeps Claude's native default (CLAUDE_CONFIG_DIR unset) unless
CLAUDE_CONFIG_DIR currently selects that directory. Use --home to select a home
without lookup. Interactive terminals show lookup progress. The working directory
is preserved. Arguments after -- are forwarded to Claude after --resume SESSION_ID
without shell parsing.

Examples:
  jig claude resume 2f4c6e8a-1b3d-4f5a-8c7e-9d0b1a2c3e4f
  jig claude resume 2f4c6e8a-1b3d-4f5a-8c7e-9d0b1a2c3e4f -- --model sonnet
  jig claude resume 2f4c6e8a-1b3d-4f5a-8c7e-9d0b1a2c3e4f --home work
  jig claude resume 2f4c6e8a-1b3d-4f5a-8c7e-9d0b1a2c3e4f --dry-run -- --fork-session";

#[derive(Debug, Subcommand)]
pub(crate) enum ClaudeCommand {
    /// List Claude homes; optionally read subscription usage limits.
    Homes(ClaudeHomesOpts),
    /// Select a Claude home and launch Claude Code.
    #[command(
        after_help = "A bare HOME such as work selects ~/.claude-work; claude and default select\n~/.claude with CLAUDE_CONFIG_DIR unset, preserving Claude's default global config.\nThe native default works before ~/.claude exists; Claude creates it as needed.\nExplicit paths always set CLAUDE_CONFIG_DIR and must already exist. Use ./work\nfor a relative path. Omit HOME and --auto for the searchable full-screen home picker.\nSubscription limits load in the background; Enter can launch while loading.\nOn macOS the picker may ask for Keychain access (30-second timeout).\nArrows or j/k move, / searches, Tab switches to details, r refreshes usage,\nEnter launches, and Esc or q cancels. Click a home to select it, double-click to\nlaunch it, and scroll with the wheel. Arguments after -- are forwarded exactly to Claude.\nUse --auto to select by remaining headroom across all reported quota windows,\nincluding model-specific allowances, without a picker or waiting on credential\nprompts; on macOS a Keychain dialog may appear briefly (2-second timeout).\nInspection has a 10-second cooperative deadline. If no home is eligible, launch fails.\n--auto --dry-run --json explains the decision and still inspects usage.\nUse --auto for new sessions; resume and account setup need a fixed home.\nExample: jig claude launch --auto -- -p \"Review the changes\" --output-format json\n\nAfter upgrading Jig in an existing repository, refresh its generated launcher\nwith scripts/jig update --recopy. Older launchers change to the repository root\nbefore launching Claude, which also changes how relative HOME paths resolve."
    )]
    Launch(ClaudeLaunchOpts),
    /// Find the Claude home containing a session and resume it.
    #[command(after_help = RESUME_AFTER_HELP)]
    Resume(ClaudeResumeOpts),
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
    /// Select the home with most remaining subscription headroom without waiting on prompts.
    #[arg(long = "auto", conflicts_with = "home")]
    pub(crate) auto_select: bool,
    /// Print the selected home and command without launching Claude.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Arguments forwarded exactly to Claude after --.
    #[arg(last = true, allow_hyphen_values = true, value_name = "CLAUDE_ARGS")]
    pub(crate) claude_args: Vec<OsString>,
}

#[derive(Args, Debug)]
pub(crate) struct ClaudeResumeOpts {
    /// Claude session UUID to resume.
    #[arg(value_name = "SESSION_ID")]
    pub(crate) session_id: String,
    /// Use this Claude home instead of looking up the session.
    #[arg(long, value_name = "HOME")]
    pub(crate) home: Option<PathBuf>,
    /// Print the resolved home and command without launching Claude.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Arguments forwarded exactly to Claude after --resume SESSION_ID.
    #[arg(last = true, allow_hyphen_values = true, value_name = "CLAUDE_ARGS")]
    pub(crate) claude_args: Vec<OsString>,
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::Path;

    use clap::Parser;

    use super::ClaudeCommand;
    use crate::cli::{Cli, CommandKind};

    #[test]
    fn parses_claude_resume_with_home_and_forwarded_arguments() {
        let resume = Cli::try_parse_from([
            "jig",
            "claude",
            "resume",
            "2f4c6e8a-1b3d-4f5a-8c7e-9d0b1a2c3e4f",
            "--home",
            "work",
            "--dry-run",
            "--",
            "--model",
            "sonnet",
        ])
        .unwrap();
        match resume.command {
            CommandKind::Claude(ClaudeCommand::Resume(opts)) => {
                assert_eq!(opts.session_id, "2f4c6e8a-1b3d-4f5a-8c7e-9d0b1a2c3e4f");
                assert_eq!(opts.home.as_deref(), Some(Path::new("work")));
                assert!(opts.dry_run);
                assert_eq!(opts.claude_args, ["--model", "sonnet"].map(OsString::from));
            }
            other => panic!("expected claude resume command, got {other:?}"),
        }

        assert!(
            Cli::try_parse_from([
                "jig",
                "claude",
                "resume",
                "2f4c6e8a-1b3d-4f5a-8c7e-9d0b1a2c3e4f",
                "--model",
            ])
            .is_err(),
            "Claude arguments must be separated from Jig arguments with --"
        );
    }
}
