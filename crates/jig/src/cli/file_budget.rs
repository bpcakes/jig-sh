use std::io::Write;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::{ArgGroup, Args, Subcommand};
use jig_contract::{
    ComparisonRequestV1, MissingComparisonV1, NativeFileBudgetConfigV1, RunConclusion,
    StrictInventoryReasonV1,
};

use crate::context::RepoContext;
use crate::repository::{
    FILE_BUDGET_MAX_CANDIDATES_HARD_CAP_V1, FILE_BUDGET_MAX_TOTAL_BYTES_HARD_CAP_V1,
};
use crate::runtime::{FileBudgetEvaluationMode, run_direct_file_budget};

use super::comparison::{CliExactTreeProvenance, comparison_request};
use super::output::print_json;
use super::structured_error::{json_error_payload, json_reported_error};

const DIRECT_FILE_BUDGET_DEADLINE: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Subcommand)]
pub(crate) enum FileBudgetCommand {
    /// Evaluate changed governed files against an explicit comparison.
    Check(FileBudgetCheckOpts),
    /// Inventory and report all current governed files.
    Audit(FileBudgetAuditOpts),
    /// Explain policy, measurements, debt, and waiver disposition for one path.
    Explain(FileBudgetExplainOpts),
    /// Validate policy schema, matching, and waiver targets without measuring content.
    Validate(FileBudgetValidateOpts),
}

#[derive(Args, Clone, Debug, Default)]
#[command(group(
    ArgGroup::new("comparison_selector")
        .args(["base", "exact_tree", "staged", "strict_inventory"])
        .multiple(false)
))]
pub(crate) struct FileBudgetComparisonOpts {
    /// Compare the worktree with the merge base of this ref and HEAD.
    #[arg(long)]
    base: Option<String>,
    /// Compare the worktree directly with this exact commit or tree identity.
    #[arg(long, value_name = "OID", requires = "provenance")]
    exact_tree: Option<String>,
    /// State the authority carried by --exact-tree.
    #[arg(long, value_name = "KIND", requires = "exact_tree")]
    provenance: Option<CliExactTreeProvenance>,
    /// Compare index content with HEAD, or the empty tree in an unborn repository.
    #[arg(long)]
    staged: bool,
    /// Evaluate an exhaustive current inventory with no baseline debt inheritance.
    #[arg(long)]
    strict_inventory: bool,
}

impl FileBudgetComparisonOpts {
    pub(super) fn request(&self) -> Result<Option<ComparisonRequestV1>> {
        comparison_request(
            self.base.as_deref(),
            self.exact_tree.as_deref(),
            self.provenance,
            self.staged,
            self.strict_inventory,
            "",
        )
    }
}

#[derive(Args, Clone, Debug, Default)]
pub(crate) struct FileBudgetLimitsOpts {
    /// Override the direct diagnostic candidate ceiling within Jig's hard cap.
    #[arg(long, value_name = "COUNT")]
    max_candidates: Option<u64>,
    /// Override the direct diagnostic aggregate byte ceiling within Jig's hard cap.
    #[arg(long, value_name = "BYTES")]
    max_total_bytes: Option<u64>,
}

impl FileBudgetLimitsOpts {
    fn configuration(&self) -> std::result::Result<NativeFileBudgetConfigV1, String> {
        let mut configuration = NativeFileBudgetConfigV1::default();
        if let Some(max_candidates) = self.max_candidates {
            if !(1..=FILE_BUDGET_MAX_CANDIDATES_HARD_CAP_V1).contains(&max_candidates) {
                return Err(format!(
                    "--max-candidates must be between 1 and {FILE_BUDGET_MAX_CANDIDATES_HARD_CAP_V1}"
                ));
            }
            configuration.max_candidates = max_candidates;
        }
        if let Some(max_total_bytes) = self.max_total_bytes {
            if !(1..=FILE_BUDGET_MAX_TOTAL_BYTES_HARD_CAP_V1).contains(&max_total_bytes) {
                return Err(format!(
                    "--max-total-bytes must be between 1 and {FILE_BUDGET_MAX_TOTAL_BYTES_HARD_CAP_V1}"
                ));
            }
            configuration.max_total_bytes = max_total_bytes;
        }
        configuration.missing_comparison = MissingComparisonV1::Block;
        Ok(configuration)
    }
}

#[derive(Args, Debug)]
pub(crate) struct FileBudgetCheckOpts {
    #[command(flatten)]
    comparison: FileBudgetComparisonOpts,
    #[command(flatten)]
    limits: FileBudgetLimitsOpts,
}

#[derive(Args, Debug)]
pub(crate) struct FileBudgetAuditOpts {
    /// Fail the direct command when the inventory contains ordinary debt.
    #[arg(long)]
    strict: bool,
    /// Exclude nonignored untracked regular files from this diagnostic inventory.
    #[arg(long)]
    tracked_only: bool,
    #[command(flatten)]
    limits: FileBudgetLimitsOpts,
}

#[derive(Args, Debug)]
pub(crate) struct FileBudgetExplainOpts {
    /// Exact repository-relative path to explain.
    path: String,
    #[command(flatten)]
    comparison: FileBudgetComparisonOpts,
    #[command(flatten)]
    limits: FileBudgetLimitsOpts,
}

#[derive(Args, Debug)]
pub(crate) struct FileBudgetValidateOpts {
    /// Validate policy and target matching from index authority.
    #[arg(long)]
    staged: bool,
}

pub(super) fn run_file_budget_command(command: FileBudgetCommand, json_output: bool) -> Result<()> {
    let ctx = RepoContext::load()?;
    let (operation, request, configuration, mode, informational) = match &command {
        FileBudgetCommand::Check(options) => (
            "check",
            options.comparison.request()?,
            direct_configuration(&options.limits, json_output)?,
            FileBudgetEvaluationMode::Check,
            false,
        ),
        FileBudgetCommand::Audit(options) => (
            "audit",
            Some(ComparisonRequestV1::StrictInventory {
                reason: StrictInventoryReasonV1::ExplicitAudit,
            }),
            direct_configuration(&options.limits, json_output)?,
            FileBudgetEvaluationMode::Audit {
                tracked_only: options.tracked_only,
            },
            !options.strict,
        ),
        FileBudgetCommand::Explain(options) => (
            "explain",
            options.comparison.request()?,
            direct_configuration(&options.limits, json_output)?,
            FileBudgetEvaluationMode::Explain {
                path: &options.path,
            },
            false,
        ),
        FileBudgetCommand::Validate(options) => (
            "validate",
            Some(if options.staged {
                ComparisonRequestV1::IndexAgainstHead
            } else {
                ComparisonRequestV1::StrictInventory {
                    reason: StrictInventoryReasonV1::ExplicitAudit,
                }
            }),
            NativeFileBudgetConfigV1::default(),
            FileBudgetEvaluationMode::Validate,
            false,
        ),
    };
    let result = run_direct_file_budget(
        &ctx,
        request,
        configuration,
        mode,
        Instant::now() + DIRECT_FILE_BUDGET_DEADLINE,
        &|| false,
    )?;
    let exit_status = direct_exit_status(operation, informational, &result);
    if json_output {
        print_json(&serde_json::json!({
            "ok": exit_status == 0,
            "command": format!("file-budget {operation}"),
            "schema": "jig.file_budget/report-v1",
            "conclusion": result.conclusion,
            "finding_count": result.finding_count,
            "finding_preview_count": result.findings.len(),
            "findings_truncated": result.findings_truncated,
            "findings_digest": result.findings_digest,
            "findings": result.findings,
            "evaluated_at_ms": result.evaluated_at_ms,
            "valid_until_ms": result.valid_until_ms,
            "report": result.evidence.as_ref().and_then(|evidence| evidence.get("file_budget")),
            "exit_status": exit_status,
        }))?;
    } else {
        let mut stdout = std::io::stdout().lock();
        if result.human_output.trim().is_empty() {
            writeln!(stdout, "file-budget {operation}: no findings")?;
        } else {
            write!(stdout, "{}", result.human_output)?;
            if !result.human_output.ends_with('\n') {
                writeln!(stdout)?;
            }
        }
    }
    if exit_status == 0 {
        Ok(())
    } else {
        Err(super::structured_error::file_budget_exit(exit_status))
    }
}

fn direct_configuration(
    limits: &FileBudgetLimitsOpts,
    json_output: bool,
) -> Result<NativeFileBudgetConfigV1> {
    limits.configuration().map_err(|message| {
        if json_output {
            let _ = print_json(&json_error_payload("invalid_invocation", &message, 2));
            json_reported_error(2)
        } else {
            super::structured_error::file_budget_invocation_error(message)
        }
    })
}

fn direct_exit_status(
    operation: &str,
    informational: bool,
    result: &jig_contract::NativeActionResult,
) -> i32 {
    if operation == "validate" && result.conclusion == RunConclusion::Failure {
        return 2;
    }
    if result.findings.iter().any(|finding| {
        matches!(
            finding.code.as_deref(),
            Some(
                "file_budget.policy_invalid"
                    | "file_budget.waiver_invalid"
                    | "file_budget.waiver_expired"
            )
        )
    }) {
        return 2;
    }
    match result.conclusion {
        RunConclusion::Success => 0,
        RunConclusion::Failure if informational => 0,
        RunConclusion::Failure => 1,
        RunConclusion::Blocked | RunConclusion::Cancelled | RunConclusion::TimedOut => 3,
        RunConclusion::Skipped => 0,
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use jig_contract::ExactTreeProvenanceV1;

    use super::*;
    use crate::cli::{Cli, CommandKind};

    fn check_options(args: &[&str]) -> FileBudgetCheckOpts {
        match Cli::try_parse_from(args).unwrap().command {
            CommandKind::FileBudget(FileBudgetCommand::Check(options)) => options,
            other => panic!("unexpected parsed command: {other:?}"),
        }
    }

    #[test]
    fn check_comparison_selector_grammar_is_closed_and_unambiguous() {
        for args in [
            vec!["jig", "file-budget", "check", "--base", "main", "--staged"],
            vec!["jig", "file-budget", "check", "--exact-tree", "abcd"],
            vec!["jig", "file-budget", "check", "--provenance", "explicit"],
            vec![
                "jig",
                "file-budget",
                "check",
                "--strict-inventory",
                "--base",
                "main",
            ],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }

        assert_eq!(
            check_options(&["jig", "file-budget", "check", "--base", "origin/main"])
                .comparison
                .request()
                .unwrap(),
            Some(ComparisonRequestV1::MergeBaseRef {
                requested_ref: "origin/main".into(),
            })
        );
        assert_eq!(
            check_options(&[
                "jig",
                "file-budget",
                "check",
                "--exact-tree",
                "0000000000000000000000000000000000000000",
                "--provenance",
                "push_before",
            ])
            .comparison
            .request()
            .unwrap(),
            Some(ComparisonRequestV1::ExactTree {
                requested_oid: "0000000000000000000000000000000000000000".into(),
                provenance: ExactTreeProvenanceV1::PushBefore,
            })
        );
        assert_eq!(
            check_options(&["jig", "file-budget", "check", "--staged"])
                .comparison
                .request()
                .unwrap(),
            Some(ComparisonRequestV1::IndexAgainstHead)
        );
        assert_eq!(
            check_options(&["jig", "file-budget", "check", "--strict-inventory"])
                .comparison
                .request()
                .unwrap(),
            Some(ComparisonRequestV1::StrictInventory {
                reason: StrictInventoryReasonV1::ExplicitCheck,
            })
        );
        assert_eq!(
            check_options(&["jig", "file-budget", "check"])
                .comparison
                .request()
                .unwrap(),
            None
        );
    }

    #[test]
    fn direct_limits_use_defaults_and_reject_values_outside_hard_caps() {
        assert_eq!(
            FileBudgetLimitsOpts::default().configuration().unwrap(),
            NativeFileBudgetConfigV1::default()
        );
        assert!(
            FileBudgetLimitsOpts {
                max_candidates: Some(0),
                max_total_bytes: None,
            }
            .configuration()
            .is_err()
        );
        assert!(
            FileBudgetLimitsOpts {
                max_candidates: None,
                max_total_bytes: Some(FILE_BUDGET_MAX_TOTAL_BYTES_HARD_CAP_V1 + 1),
            }
            .configuration()
            .is_err()
        );
    }

    #[test]
    fn direct_exit_contract_distinguishes_policy_invalid_violation_and_blocked() {
        let result = |conclusion: RunConclusion, code: &str| jig_contract::NativeActionResult {
            conclusion,
            findings: vec![jig_contract::Finding {
                severity: jig_contract::FindingSeverity::Error,
                message: "example".into(),
                code: Some(code.into()),
                source: Some(jig_contract::tool::FILE_BUDGET.into()),
                location: None,
            }],
            finding_count: 1,
            findings_truncated: false,
            findings_digest: "sha256:example".into(),
            human_output: String::new(),
            evidence: None,
            evaluated_at_ms: 1,
            valid_until_ms: None,
        };
        assert_eq!(
            direct_exit_status(
                "check",
                false,
                &result(RunConclusion::Failure, "file_budget.max_lines")
            ),
            1
        );
        assert_eq!(
            direct_exit_status(
                "check",
                false,
                &result(RunConclusion::Failure, "file_budget.policy_invalid")
            ),
            2
        );
        assert_eq!(
            direct_exit_status(
                "check",
                false,
                &result(RunConclusion::Blocked, "file_budget.resource_limit")
            ),
            3
        );
        assert_eq!(
            direct_exit_status(
                "audit",
                true,
                &result(RunConclusion::Failure, "file_budget.max_lines")
            ),
            0
        );
    }
}
