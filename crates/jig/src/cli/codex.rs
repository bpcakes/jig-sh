use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::tool_defs;

pub(super) const CODEX_AFTER_HELP: &str = "\
Codex homes are separate CODEX_HOME directories, each with its own account and
local state. Codex configuration profiles selected with codex --profile remain
independent layers inside the selected home.

Examples:
  jig codex homes
  jig codex homes --usage
  jig codex launch
  jig codex launch codex-1 -- --search";

pub(super) const CODEX_LAUNCH_AFTER_HELP: &str = "\
With no HOME, open a searchable terminal picker immediately. Account and usage
details load in the background; arrows or j/k move, / searches, Enter launches,
and Esc or q cancels.
A bare name such as work resolves as ~/.codex-work; use ./work to select a relative directory.
The aliases codex and default both select ~/.codex.
Arguments after -- are forwarded to Codex without shell parsing.

Examples:
  jig codex launch
  jig codex launch codex-1
  jig codex launch ~/.codex-work -- --profile deep-review
  jig codex launch codex-1 --dry-run -- --search";

#[derive(Debug, Subcommand)]
pub(crate) enum CodexCommand {
    /// List discovered Codex homes and their authenticated accounts.
    #[command(name = tool_defs::cli_command::CODEX_HOMES)]
    Homes(CodexHomesOpts),
    /// Select a Codex home and launch the interactive Codex CLI.
    #[command(
        name = tool_defs::cli_command::CODEX_LAUNCH,
        after_help = CODEX_LAUNCH_AFTER_HELP
    )]
    Launch(CodexLaunchOpts),
}

#[derive(Args, Debug)]
pub(crate) struct CodexHomesOpts {
    #[arg(
        long,
        help = "Also fetch current rate-limit windows for every discovered home"
    )]
    pub(crate) usage: bool,
}

#[derive(Args, Debug)]
pub(crate) struct CodexLaunchOpts {
    #[arg(
        value_name = "HOME",
        help = "Codex home name or path; omit to choose interactively"
    )]
    pub(crate) home: Option<PathBuf>,
    #[arg(
        long,
        help = "Print the selected home and Codex command without launching"
    )]
    pub(crate) dry_run: bool,
    #[arg(
        last = true,
        allow_hyphen_values = true,
        value_name = "CODEX_ARGS",
        help = "Arguments forwarded exactly to Codex after --"
    )]
    pub(crate) codex_args: Vec<OsString>,
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::Path;

    use clap::Parser;

    use super::CodexCommand;
    use crate::cli::{Cli, CommandKind};

    #[test]
    fn parses_codex_homes_and_launch_commands() {
        let homes = Cli::try_parse_from(["jig", "codex", "homes", "--usage", "--json"]).unwrap();
        assert!(homes.json);
        match homes.command {
            CommandKind::Codex(CodexCommand::Homes(opts)) => assert!(opts.usage),
            other => panic!("expected codex homes command, got {other:?}"),
        }

        let launch = Cli::try_parse_from([
            "jig",
            "codex",
            "launch",
            "codex-1",
            "--dry-run",
            "--",
            "--search",
            "-c",
            "model_reasoning_effort=high",
        ])
        .unwrap();
        match launch.command {
            CommandKind::Codex(CodexCommand::Launch(opts)) => {
                assert_eq!(opts.home.as_deref(), Some(Path::new("codex-1")));
                assert!(opts.dry_run);
                assert_eq!(
                    opts.codex_args,
                    ["--search", "-c", "model_reasoning_effort=high"]
                        .into_iter()
                        .map(OsString::from)
                        .collect::<Vec<_>>()
                );
            }
            other => panic!("expected codex launch command, got {other:?}"),
        }

        assert!(
            Cli::try_parse_from(["jig", "codex", "launch", "codex-1", "--search"]).is_err(),
            "Codex arguments must be separated from Jig arguments with --"
        );
    }
}
