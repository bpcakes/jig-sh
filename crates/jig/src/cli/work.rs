use std::path::PathBuf;

use clap::{ArgGroup, Args, Subcommand};

use crate::tool_defs::{self, DEFAULT_RECEIPTS_LIMIT};

pub(super) const WORK_START_AFTER_HELP: &str = "\
Use --body for short notes or --body-file for a prepared markdown plan.
Use --print-plan-id when shell scripts only need the new plan id.
Human-readable output is the default. Pass --json for structured automation output.

Examples:
  jig work start --title \"Add auth\" --body \"Implement login flow and validation.\"
  plan_id=\"$(jig work start --title \"Fix signup\" --body-file .agent/notes/signup-plan.md --print-plan-id)\"";

pub(super) const WORK_CHECK_AFTER_HELP: &str = "\
Run all required gates for a plan, or use --tool to run one configured gate.
Human-readable output is the default. Pass --json for structured automation output.

Examples:
  jig work check --plan-id plan_abc123
  jig work check --plan-id plan_abc123 --json
  jig work check --plan-id plan_abc123 --tool jig.test";

pub(super) const WORK_GATES_AFTER_HELP: &str = "\
Human-readable output is the default. Pass --json for structured automation output.

Examples:
  jig work gates --plan-id plan_abc123
  jig work gates --plan-id plan_abc123 --json";

pub(super) const WORK_EVIDENCE_AFTER_HELP: &str = "\
Shows the latest gate evidence, whether receipts match the current worktree,
changed paths covered by the receipt, and exact stale or unknown freshness
reasons. If --plan-id is omitted, exactly one open plan must exist.

Examples:
  jig work evidence
  jig work evidence --plan-id plan_abc123
  jig work evidence --json";

pub(super) const WORK_REVIEW_AFTER_HELP: &str = "\
Run configured codex_review gates for a plan and record structured finding receipts.
Use --gate to run one review gate by id.

Examples:
  jig work review --plan-id plan_abc123
  jig work review --plan-id plan_abc123 --gate rust-error-handling
  jig work review --plan-id plan_abc123 --json";

pub(super) const WORK_REFINE_AFTER_HELP: &str = "\
Run review-driven refinement: review, fix actionable findings, review again,
then rerun normal check gates. Use --gate to limit review gates by id.

Examples:
  jig work refine --plan-id plan_abc123
  jig work refine --plan-id plan_abc123 --max-iterations 2
  jig work refine --plan-id plan_abc123 --json";

pub(super) const WORK_FINISH_AFTER_HELP: &str = "\
Close a plan after required gates pass; use --outcome for a machine-readable result.

Examples:
  jig work finish --plan-id plan_abc123 --resolution \"Auth flow complete\" --outcome success";

pub(super) const WORK_RECEIPTS_AFTER_HELP: &str = "\
Human-readable output is the default. Pass --json for structured automation output.

Examples:
  jig work receipts --failed-only --limit 5
  jig work receipts --plan-id plan_abc123
  jig work receipts --json";

#[derive(Debug, Subcommand)]
pub(crate) enum WorkCommand {
    /// Start a structured goal plan from an objective and validation contract.
    #[command(name = tool_defs::cli_command::WORK_GOAL)]
    Goal(WorkGoalOpts),
    /// Start a structured work plan and session.
    #[command(
        name = tool_defs::cli_command::WORK_START,
        after_help = WORK_START_AFTER_HELP
    )]
    Start(WorkStartOpts),
    /// Append progress text to an open work plan.
    #[command(name = tool_defs::cli_command::WORK_APPEND)]
    Append(WorkAppendOpts),
    /// Run configured or selected work gate checks for a plan.
    #[command(
        name = tool_defs::cli_command::WORK_CHECK,
        after_help = WORK_CHECK_AFTER_HELP
    )]
    Check(WorkCheckOpts),
    /// Show required gate status for a plan.
    #[command(
        name = tool_defs::cli_command::WORK_GATES,
        after_help = WORK_GATES_AFTER_HELP
    )]
    Gates(WorkGatesOpts),
    /// Summarize receipt evidence for configured work gates.
    #[command(
        name = tool_defs::cli_command::WORK_EVIDENCE,
        after_help = WORK_EVIDENCE_AFTER_HELP
    )]
    Evidence(WorkEvidenceOpts),
    /// Run configured Codex review gates for a plan.
    #[command(
        name = tool_defs::cli_command::WORK_REVIEW,
        after_help = WORK_REVIEW_AFTER_HELP
    )]
    Review(WorkReviewOpts),
    /// Run review-driven refinement, then rerun review and check gates.
    #[command(
        name = tool_defs::cli_command::WORK_REFINE,
        after_help = WORK_REFINE_AFTER_HELP
    )]
    Refine(WorkRefineOpts),
    /// Record a durable decision for the current work.
    #[command(name = tool_defs::cli_command::WORK_DECIDE)]
    Decide(WorkDecisionAddOpts),
    /// List recorded command receipts.
    #[command(
        name = tool_defs::cli_command::WORK_RECEIPTS,
        after_help = WORK_RECEIPTS_AFTER_HELP
    )]
    Receipts(WorkReceiptsOpts),
    /// Summarize current structured work state.
    #[command(name = tool_defs::cli_command::WORK_STATUS)]
    Status,
    /// Close a work plan after required gates pass.
    #[command(
        name = tool_defs::cli_command::WORK_FINISH,
        after_help = WORK_FINISH_AFTER_HELP
    )]
    Finish(WorkFinishOpts),
}

#[derive(Args, Debug)]
pub(crate) struct WorkGoalOpts {
    #[arg(long, help = "Plain-language objective for the work")]
    pub(crate) objective: String,
    #[arg(long, help = "Observable condition that means the goal is complete")]
    pub(crate) success: String,
    #[arg(
        long = "validation",
        required = true,
        help = "Validation command or check that must pass; may be repeated"
    )]
    pub(crate) validations: Vec<String>,
    #[arg(
        long = "constraint",
        help = "Constraint to preserve while working; may be repeated"
    )]
    pub(crate) constraints: Vec<String>,
    #[arg(
        long = "checkpoint",
        help = "Progress checkpoint to include in the plan; may be repeated"
    )]
    pub(crate) checkpoints: Vec<String>,
    #[arg(long, help = "Optional plan title; defaults from the objective")]
    pub(crate) title: Option<String>,
    #[arg(long, help = "Additional notes to include in the generated plan")]
    pub(crate) notes: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct WorkStartOpts {
    #[arg(long, help = "Short human-readable plan title")]
    pub(crate) title: String,
    #[arg(long, conflicts_with = "body_file", help = "Initial plan body text")]
    pub(crate) body: Option<String>,
    #[arg(long, help = "Path to read the initial plan body from")]
    pub(crate) body_file: Option<PathBuf>,
    #[arg(
        long,
        help = "Print only the new plan id (for shell capture); conflicts with --json"
    )]
    pub(crate) print_plan_id: bool,
}

#[derive(Args, Debug)]
#[command(group(
    ArgGroup::new("body_source")
        .required(true)
        .multiple(false)
        .args(["body", "body_file"])
))]
pub(crate) struct WorkAppendOpts {
    #[arg(long, help = "Open plan id to append to")]
    pub(crate) plan_id: String,
    #[arg(long, help = "Progress text to append")]
    pub(crate) body: Option<String>,
    #[arg(long, help = "Path to read progress text from")]
    pub(crate) body_file: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub(crate) struct WorkCheckOpts {
    #[arg(long, help = "Open plan id to check")]
    pub(crate) plan_id: String,

    #[arg(
        long = "tool",
        help = "Specific gate tool to run; defaults to configured gates"
    )]
    pub(crate) tools: Vec<String>,
}

#[derive(Args, Debug)]
pub(crate) struct WorkGatesOpts {
    #[arg(long, help = "Plan id to inspect; defaults to the single open plan")]
    pub(crate) plan_id: Option<String>,
}

#[derive(Args, Debug, Default)]
pub(crate) struct WorkEvidenceOpts {
    #[arg(long, help = "Open plan id whose evidence should be summarized")]
    pub(crate) plan_id: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct WorkReviewOpts {
    #[arg(long, help = "Open plan id to review")]
    pub(crate) plan_id: String,

    #[arg(long = "gate", help = "Review gate id to run; may be repeated")]
    pub(crate) gates: Vec<String>,
}

#[derive(Args, Debug)]
pub(crate) struct WorkRefineOpts {
    #[arg(long, help = "Open plan id to refine")]
    pub(crate) plan_id: String,

    #[arg(
        long = "gate",
        help = "Review gate id to refine against; may be repeated"
    )]
    pub(crate) gates: Vec<String>,

    #[arg(
        long,
        default_value_t = crate::command::DEFAULT_REFINE_MAX_ITERATIONS,
        help = "Maximum fixer attempts before stopping; default is 1 (fix once, then verify)"
    )]
    pub(crate) max_iterations: usize,
}

#[derive(Args, Debug)]
pub(crate) struct WorkFinishOpts {
    #[arg(long, help = "Open plan id to close")]
    pub(crate) plan_id: String,
    #[arg(long, help = "Resolution summary recorded on the plan")]
    pub(crate) resolution: Option<String>,
    #[arg(long, help = "Optional session outcome; defaults to the resolution")]
    pub(crate) outcome: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct WorkReceiptsOpts {
    #[arg(long, help = "Filter by session id")]
    pub(crate) session_id: Option<String>,
    #[arg(long, help = "Filter by plan id")]
    pub(crate) plan_id: Option<String>,
    #[arg(long, help = "Filter by Jig tool name")]
    pub(crate) tool_name: Option<String>,
    #[arg(long, help = "Only show failed receipts")]
    pub(crate) failed_only: bool,
    #[arg(long, default_value_t = DEFAULT_RECEIPTS_LIMIT, help = "Maximum receipts to show")]
    pub(crate) limit: usize,
}

impl Default for WorkReceiptsOpts {
    fn default() -> Self {
        Self {
            session_id: None,
            plan_id: None,
            tool_name: None,
            failed_only: false,
            limit: DEFAULT_RECEIPTS_LIMIT,
        }
    }
}

#[derive(Args, Debug)]
pub(crate) struct WorkDecisionAddOpts {
    #[arg(long, help = "Short decision title")]
    pub(crate) title: String,
    #[arg(long, help = "Chosen option or approach")]
    pub(crate) selected_option: String,
    #[arg(long, help = "Reason the selected option was chosen")]
    pub(crate) rationale: String,
    #[arg(long, help = "Alternative considered; may be repeated")]
    pub(crate) alternatives: Vec<String>,
    #[arg(long, help = "Plan id to associate with the decision")]
    pub(crate) plan_id: Option<String>,
}
