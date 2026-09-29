use clap::{Args, Subcommand};

/// Default interval between completed interactive status collections.
pub(crate) const DEFAULT_STATUS_REFRESH_SECONDS: u64 = 30;

/// Output-mode options for the aggregate status command.
#[derive(Args, Debug, Default)]
pub(crate) struct StatusOpts {
    #[command(subcommand)]
    pub(crate) command: Option<StatusCommand>,
    #[arg(long, help = "Open the interactive terminal status dashboard")]
    pub(crate) tui: bool,
    #[arg(
        long,
        value_name = "SECONDS",
        requires = "tui",
        value_parser = clap::value_parser!(u64).range(1..=3600),
        help = "Refresh interval for --tui; defaults to 30 seconds"
    )]
    pub(crate) refresh_seconds: Option<u64>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum StatusCommand {
    /// Show one durable repository run and its target results.
    Run {
        #[arg(value_name = "RUN_ID")]
        run_id: String,
    },
}

impl StatusOpts {
    pub(crate) fn effective_refresh_seconds(&self) -> u64 {
        self.refresh_seconds
            .unwrap_or(DEFAULT_STATUS_REFRESH_SECONDS)
    }
}
