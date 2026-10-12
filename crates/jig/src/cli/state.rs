use std::path::PathBuf;

use clap::{Args, Subcommand};
use jig_commands::tool_defs;

use super::output;
use super::runtime_dispatch::RuntimeDispatch;
use crate::command::{self, RuntimeCommand};

pub(super) mod render;

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
        let render: output::Render = match &self {
            Self::Summary => render::format_state_summary,
            Self::Diagnose => render::format_state_diagnose_summary,
            Self::Restore(_) => render::format_state_restore_summary,
            Self::Archive(_) => render::format_state_archive_summary,
        };
        RuntimeDispatch::new(RuntimeCommand::State(self.into()), render)
    }
}

impl From<StateCommand> for command::StateCommand {
    fn from(command: StateCommand) -> Self {
        match command {
            StateCommand::Summary => Self::Summary,
            StateCommand::Diagnose => Self::Diagnose,
            StateCommand::Restore(opts) => Self::Restore(opts.into()),
            StateCommand::Archive(opts) => Self::Archive(opts.into()),
        }
    }
}

impl From<StateRestoreOpts> for command::StateRestoreRequest {
    fn from(opts: StateRestoreOpts) -> Self {
        Self {
            backup: opts.backup,
        }
    }
}

impl From<StateArchiveOpts> for command::StateArchiveRequest {
    fn from(opts: StateArchiveOpts) -> Self {
        Self {
            before: opts.before,
            dry_run: opts.dry_run,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_archive_conversion_preserves_cutoff_and_dry_run() {
        let request: command::StateCommand = StateCommand::Archive(StateArchiveOpts {
            before: "2026-01-01".into(),
            include_runs: true,
            dry_run: true,
        })
        .into();

        match request {
            command::StateCommand::Archive(request) => {
                assert_eq!(request.before, "2026-01-01");
                assert!(request.dry_run);
            }
            other => panic!("expected state archive request, got {other:?}"),
        }
    }

    #[test]
    fn state_maintenance_conversion_preserves_arguments() {
        let request: command::StateCommand = StateCommand::Diagnose.into();
        assert!(matches!(request, command::StateCommand::Diagnose));

        let backup = std::path::PathBuf::from("backup/manifest.json");
        let request: command::StateCommand = StateCommand::Restore(StateRestoreOpts {
            backup: backup.clone(),
        })
        .into();
        match request {
            command::StateCommand::Restore(request) => {
                assert_eq!(request.backup, backup);
            }
            other => panic!("expected state restore request, got {other:?}"),
        }
    }
}
