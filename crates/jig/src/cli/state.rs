use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::tool_defs;

pub(super) const STATE_ARCHIVE_AFTER_HELP: &str = "\
Archive old receipt records out of .agent/state/receipts.jsonl while retaining
evidence required by open-plan gates. A complete pre-rewrite recovery backup is
written under .agent/.cache/state-backups. --before accepts YYYY-MM-DD
interpreted as UTC midnight, or a Unix millisecond timestamp.

Examples:
  jig state summary
  jig state diagnose --deep
  jig state compact sessions --dry-run
  jig state restore --backup .agent/.cache/state-backups/<id>
  jig state export receipts --before 2026-01-01 --output receipts.jsonl.gz
  jig state archive --before 2026-01-01
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
    /// Archive old receipts while preserving required open-plan evidence.
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

    #[arg(long, help = "Report what would be archived without rewriting state")]
    pub(crate) dry_run: bool,
}
