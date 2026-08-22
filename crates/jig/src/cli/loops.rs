use clap::{Args, Subcommand};

use crate::tool_defs;

pub(super) const LOOP_AFTER_HELP: &str = "\
`jig loop` runs runtime-owned orchestration ticks. Workflow kinds are compiled
into Jig; .jig.toml can parameterize them but cannot define arbitrary step
graphs.

Examples:
  jig loop tick --workflow noop-status
  jig loop dispatch
  jig loop status --workflow noop-status
  jig loop run --workflow noop-status --until idle
  jig loop clear-attempt --workflow pr-status --item pr-123
  jig loop acknowledge-occurrence --occurrence nightly@1787364000000";

pub(super) const LOOP_TICK_AFTER_HELP: &str = "\
Run one idempotent reconcile pass for a workflow and record receipt evidence.

Examples:
  jig loop tick --workflow noop-status";

pub(super) const LOOP_DISPATCH_AFTER_HELP: &str = "\
Run each configured workflow occurrence that is due now. Missed occurrences are
coalesced to the most recent due time; durable claims prevent duplicate runs.
Invoke this command periodically from cron, systemd, launchd, or CI.

Examples:
  jig loop dispatch";

pub(super) const LOOP_RUN_AFTER_HELP: &str = "\
Call tick until the workflow is idle, waiting, or max ticks is reached. This is
a reconcile helper for workflows such as pr_manager. Scheduled codex_task
workflows run exactly once through `jig loop tick` or when due through
`jig loop dispatch`; `loop run` rejects them to prevent repeated execution.

Examples:
  jig loop run --workflow noop-status --until idle
  jig loop run --workflow noop-status --until idle --max-ticks 5";

pub(super) const LOOP_CLEAR_ATTEMPT_AFTER_HELP: &str = "\
Clear one cached attempt-budget record after a human resolves or accepts the
item that needed attention.

Examples:
  jig loop clear-attempt --workflow pr-status --item pr-123";

pub(super) const LOOP_ACKNOWLEDGE_OCCURRENCE_AFTER_HELP: &str = "\
Acknowledge one scheduled occurrence after a human has inspected its ambiguous
result. The occurrence remains recorded, so acknowledgement cannot cause the
same schedule instant to run again.

Examples:
  jig loop acknowledge-occurrence --occurrence nightly@1787364000000";

#[derive(Debug, Subcommand)]
pub(crate) enum LoopCommand {
    /// Run one idempotent orchestration tick.
    #[command(
        name = tool_defs::cli_command::LOOP_TICK,
        after_help = LOOP_TICK_AFTER_HELP
    )]
    Tick(LoopTickOpts),
    /// Run configured workflow occurrences that are due now.
    #[command(
        name = tool_defs::cli_command::LOOP_DISPATCH,
        after_help = LOOP_DISPATCH_AFTER_HELP
    )]
    Dispatch(LoopDispatchOpts),
    /// Show configured loop workflows, live leases, and attempt state.
    #[command(name = tool_defs::cli_command::LOOP_STATUS)]
    Status(LoopStatusOpts),
    /// Run repeated ticks until the workflow is idle or waiting.
    #[command(name = tool_defs::cli_command::LOOP_RUN, after_help = LOOP_RUN_AFTER_HELP)]
    Run(LoopRunOpts),
    /// Clear one workflow attempt-budget record.
    #[command(
        name = tool_defs::cli_command::LOOP_CLEAR_ATTEMPT,
        after_help = LOOP_CLEAR_ATTEMPT_AFTER_HELP
    )]
    ClearAttempt(LoopClearAttemptOpts),
    /// Acknowledge one scheduled occurrence that needs attention.
    #[command(
        name = tool_defs::cli_command::LOOP_ACKNOWLEDGE_OCCURRENCE,
        after_help = LOOP_ACKNOWLEDGE_OCCURRENCE_AFTER_HELP
    )]
    AcknowledgeOccurrence(LoopAcknowledgeOccurrenceOpts),
}

#[derive(Args, Debug)]
pub(crate) struct LoopDispatchOpts {}

#[derive(Args, Debug)]
pub(crate) struct LoopTickOpts {
    #[arg(
        long,
        default_value = "noop-status",
        help = "Workflow id to tick; defaults to the built-in noop-status workflow"
    )]
    pub(crate) workflow: String,
    #[command(flatten)]
    pub(crate) tuning: LoopTuningOpts,
}

#[derive(Args, Debug)]
pub(crate) struct LoopStatusOpts {
    #[arg(long, help = "Workflow id to inspect; defaults to all workflows")]
    pub(crate) workflow: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct LoopRunOpts {
    #[arg(
        long,
        default_value = "noop-status",
        help = "Workflow id to run; defaults to the built-in noop-status workflow"
    )]
    pub(crate) workflow: String,
    #[arg(
        long,
        default_value = "idle",
        value_parser = ["idle"],
        help = "Stop condition; currently only 'idle' is supported"
    )]
    pub(crate) until: String,
    #[arg(long, default_value_t = 10, help = "Maximum ticks before stopping")]
    pub(crate) max_ticks: u32,
    #[command(flatten)]
    pub(crate) tuning: LoopTuningOpts,
}

#[derive(Args, Debug)]
pub(crate) struct LoopClearAttemptOpts {
    #[arg(long, help = "Workflow id whose attempt record should be cleared")]
    pub(crate) workflow: String,
    #[arg(
        long,
        help = "Workflow item key whose attempt record should be cleared"
    )]
    pub(crate) item: String,
}

#[derive(Args, Debug)]
pub(crate) struct LoopAcknowledgeOccurrenceOpts {
    #[arg(long, help = "Scheduled occurrence id reported by loop status")]
    pub(crate) occurrence: String,
}

#[derive(Args, Clone, Debug, Default)]
pub(crate) struct LoopTuningOpts {
    #[arg(long, help = "Override the workflow lease TTL in seconds")]
    pub(crate) lease_ttl_seconds: Option<u64>,
    #[arg(
        long,
        help = "Override the per-work-item attempt budget for workflows that record attempts"
    )]
    pub(crate) max_attempts: Option<u32>,
    #[arg(
        long,
        help = "Override attempt backoff in seconds for workflows that record attempts"
    )]
    pub(crate) backoff_seconds: Option<u64>,
}
