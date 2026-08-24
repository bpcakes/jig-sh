use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::tool_defs;

pub(super) const STATE_ARCHIVE_AFTER_HELP: &str = "\
Archive old receipt records and, with --include-runs, completed run histories
while retaining open-plan evidence. Apply mode first terminalizes an abandoned
run when its stable worker lease proves that no worker remains. Preview is
strictly read-only. Archival then requires every known run to be terminal so a
live reader's durable journal cursor cannot be shifted.
Complete per-stream pre-rewrite recovery backups are written under
.agent/.cache/state-backups. --before accepts YYYY-MM-DD interpreted as UTC
midnight, or a Unix millisecond timestamp.

Examples:
  jig state summary
  jig state diagnose --deep
  jig state compact sessions --dry-run
  jig state restore --backup .agent/.cache/state-backups/<id>
  jig state export receipts --before 2026-01-01 --output receipts.jsonl.gz
  jig state archive --before 2026-01-01
  jig state archive --before 2026-01-01 --include-runs
  jig state archive --before 2026-01-01 --dry-run";

#[derive(Debug, Subcommand)]
pub(crate) enum StateCommand {
    /// Summarize runtime-owned Jig state.
    #[command(name = tool_defs::cli_command::STATE_SUMMARY)]
    Summary,
    /// Diagnose state size, integrity, and legacy storage pathologies.
    #[command(name = tool_defs::cli_command::STATE_DIAGNOSE)]
    Diagnose(StateDiagnoseOpts),
    /// Compact a canonical state stream without discarding logical facts.
    #[command(name = tool_defs::cli_command::STATE_COMPACT)]
    Compact {
        #[command(subcommand)]
        command: StateCompactCommand,
    },
    /// Restore an exact state stream from a Jig maintenance backup.
    #[command(name = tool_defs::cli_command::STATE_RESTORE)]
    Restore(StateRestoreOpts),
    /// Export state records without changing canonical state.
    #[command(name = tool_defs::cli_command::STATE_EXPORT)]
    Export {
        #[command(subcommand)]
        command: StateExportCommand,
    },
    /// Archive old receipts and completed runs while preserving open-plan state.
    #[command(
        name = tool_defs::cli_command::STATE_ARCHIVE,
        after_help = STATE_ARCHIVE_AFTER_HELP
    )]
    Archive(StateArchiveOpts),
}

#[derive(Args, Debug)]
pub(crate) struct StateDiagnoseOpts {
    #[arg(
        long,
        help = "Parse stream-specific payloads to find recursive summaries and storage waste"
    )]
    pub(crate) deep: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum StateCompactCommand {
    /// Normalize legacy recursive session summaries.
    #[command(name = tool_defs::cli_command::STATE_SESSIONS)]
    Sessions(StateCompactSessionsOpts),
}

#[derive(Args, Debug)]
pub(crate) struct StateCompactSessionsOpts {
    #[arg(long, help = "Validate and report the rewrite without changing state")]
    pub(crate) dry_run: bool,
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

#[derive(Debug, Subcommand)]
pub(crate) enum StateExportCommand {
    /// Export old receipt records to an exact gzip JSONL stream.
    #[command(name = tool_defs::cli_command::STATE_RECEIPTS)]
    Receipts(StateExportReceiptsOpts),
}

#[derive(Args, Debug)]
pub(crate) struct StateExportReceiptsOpts {
    #[arg(
        long,
        help = "Export receipts older than YYYY-MM-DD UTC or a Unix millisecond timestamp"
    )]
    pub(crate) before: String,

    #[arg(long, value_name = "PATH", help = "Destination .jsonl.gz file")]
    pub(crate) output: PathBuf,
}

#[derive(Args, Debug)]
pub(crate) struct StateArchiveOpts {
    #[arg(
        long,
        help = "Archive receipts older than YYYY-MM-DD UTC or a Unix millisecond timestamp"
    )]
    pub(crate) before: String,

    #[arg(
        long,
        help = "Also archive completed run histories not linked to open work plans"
    )]
    pub(crate) include_runs: bool,

    #[arg(long, help = "Report what would be archived without rewriting state")]
    pub(crate) dry_run: bool,
}
