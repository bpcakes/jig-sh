use std::path::PathBuf;

use clap::{Args, Subcommand};

use super::output::HumanOutput;
use super::runtime_dispatch::RuntimeDispatch;
use crate::command::RuntimeCommand;
use crate::tool_defs;

pub(super) const STATE_ARCHIVE_AFTER_HELP: &str = "\
Archive completed run histories that ended before --before.
Apply mode first terminalizes an abandoned
run when its stable worker lease proves that no worker remains. Preview is
strictly read-only. Archival then requires every known run to be terminal so a
live reader's durable journal cursor cannot be shifted.
A complete pre-rewrite recovery backup is written under
.agent/.cache/state-backups. --before accepts YYYY-MM-DD interpreted as UTC
midnight, or a Unix millisecond timestamp.

Examples:
  jig state summary
  jig state diagnose
  jig state restore --backup .agent/.cache/state-backups/<id>
  jig state archive --before 2026-01-01 --dry-run
  jig state archive --before 2026-01-01";

#[derive(Debug, Subcommand)]
pub(crate) enum StateCommand {
    /// Summarize runtime-owned Jig state.
    #[command(name = tool_defs::cli_command::STATE_SUMMARY)]
    Summary,
    /// Diagnose state size, integrity, and legacy storage pathologies.
    #[command(name = tool_defs::cli_command::STATE_DIAGNOSE)]
    Diagnose,
    /// Restore an exact state stream from a Jig maintenance backup.
    #[command(name = tool_defs::cli_command::STATE_RESTORE)]
    Restore(StateRestoreOpts),
    /// Archive completed run histories.
    #[command(
        name = tool_defs::cli_command::STATE_ARCHIVE,
        after_help = STATE_ARCHIVE_AFTER_HELP
    )]
    Archive(StateArchiveOpts),
}

#[derive(Args, Debug)]
pub(crate) struct StateRestoreOpts {
    #[arg(
        long,
        value_name = "PATH",
        help = "Backup directory or manifest.json written by Jig state maintenance"
    )]
    pub(crate) backup: PathBuf,
}

#[derive(Args, Debug)]
pub(crate) struct StateArchiveOpts {
    #[arg(
        long,
        help = "Archive runs that ended before YYYY-MM-DD UTC or a Unix millisecond timestamp"
    )]
    pub(crate) before: String,

    /// Runs are the only stream archived; the flag is accepted so existing
    /// invocations keep working.
    #[arg(long, hide = true)]
    pub(crate) include_runs: bool,

    #[arg(long, help = "Report what would be archived without rewriting state")]
    pub(crate) dry_run: bool,
}

impl StateCommand {
    pub(super) fn into_dispatch(self) -> RuntimeDispatch {
        let human_output = match &self {
            Self::Summary => HumanOutput::StateSummary,
            Self::Diagnose => HumanOutput::StateDiagnose,
            Self::Restore(_) => HumanOutput::StateRestore,
            Self::Archive(_) => HumanOutput::StateArchive,
        };
        RuntimeDispatch::new(RuntimeCommand::State(self.into()), human_output)
    }
}
