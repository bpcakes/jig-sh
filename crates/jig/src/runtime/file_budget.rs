use std::fs::{File, OpenOptions};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use jig_contract::{
    ComparisonPreparationV1, ComparisonRequestV1, Finding, FindingLocation, FindingSeverity,
    NativeActionResult, NativeFileBudgetConfigV1, PolicyPreparationV1, PreparedNativeInputV1,
    ResolvedComparisonV1, RunConclusion,
};
use jig_file_budget::{
    BudgetDiagnosticCodeV1, BudgetDiagnosticV1, BudgetSeverityV1, ComparisonPolicyV1,
    CurrentFileStateV1, EvaluateFileV1, EvaluationInputV1, ExactCurrentPathFactV1,
    ExactCurrentPathStateV1, MAX_POLICY_BYTES_V1, MeasurementBudgetV1, MeasurementErrorKindV1,
    MeasurementV1, PathDispositionV1, PolicyDateV1, PolicyV1, UnsupportedFileKindV1, evaluate_v1,
    measure_stream_v1, parse_comparison_policy_v1, parse_policy_v1,
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};

use crate::context::RepoContext;
use crate::git_receipts::{
    BaselineFileV1, CurrentSourceV1, ExactCurrentPathStateV1 as GitExactCurrentPathStateV1,
    FileChangeKindV1, ScopeEntryV1, ScopeIssueKindV1, ScopeSnapshotV1,
    capture_all_current_scope_v1_with_cancellation, capture_scope_v1_with_cancellation,
    is_git_receipt_collection_cancellation, observe_exact_paths_v1_with_cancellation,
    read_git_blob_v1_with_cancellation, read_tree_path_blob_v1_with_cancellation,
    resolve_index_blob_oid_v1_with_cancellation, resolve_tree_path_blob_oid_v1_with_cancellation,
};

const FINDING_PREVIEW_LIMIT_V1: usize = 256;
const EVIDENCE_ISSUE_PREVIEW_LIMIT_V1: usize = 64;
const HUMAN_OUTPUT_BYTES_V1: usize = 64 * 1024;
const POLICY_PATH_V1: &str = ".jig/file-budget.toml";

pub(super) struct FileBudgetEngineContext<'a> {
    pub(super) repository: &'a RepoContext,
    pub(super) prepared_input: &'a PreparedNativeInputV1,
    pub(super) deadline: Instant,
    pub(super) cancelled: &'a dyn Fn() -> bool,
    pub(super) mode: FileBudgetEvaluationMode<'a>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum FileBudgetEvaluationMode<'a> {
    Check,
    Audit { tracked_only: bool },
    Explain { path: &'a str },
    Validate,
}

pub(crate) fn run_direct_file_budget(
    repository: &RepoContext,
    request: Option<ComparisonRequestV1>,
    configuration: NativeFileBudgetConfigV1,
    mode: FileBudgetEvaluationMode<'_>,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeActionResult> {
    let prepared =
        crate::repository::prepare_file_budget_input_v1(repository, request, configuration, None)?;
    if let PolicyPreparationV1::InvalidPolicy {
        diagnostics_count,
        diagnostics_digest,
        diagnostics_preview,
        ..
    } = &prepared.policy
    {
        let findings = diagnostics_preview
            .iter()
            .map(|diagnostic| {
                file_budget_finding(
                    diagnostic.severity,
                    &diagnostic.code,
                    &diagnostic.message,
                    diagnostic.path.as_deref(),
                )
            })
            .collect::<Vec<_>>();
        let evaluated_at_ms = crate::state::now_ms();
        let mut result = result_with_findings(
            RunConclusion::Failure,
            findings,
            evaluated_at_ms,
            None,
            Some(json!({
                "file_budget": {
                    "schema": "jig.file_budget/evidence-v1",
                    "policy_preparation": prepared.policy,
                    "comparison_preparation": prepared.comparison,
                    "request": prepared.request,
                    "view": prepared.view,
                    "configuration": prepared.configuration,
                    "complete": false,
                    "evaluated_at_ms": evaluated_at_ms,
                }
            })),
        );
        result.finding_count = *diagnostics_count;
        result.findings_truncated = *diagnostics_count > result.findings.len() as u64;
        result.findings_digest.clone_from(diagnostics_digest);
        return Ok(result);
    }
    if let ComparisonPreparationV1::ComparisonUnavailable { reason, .. } = &prepared.comparison {
        let evaluated_at_ms = crate::state::now_ms();
        return Ok(terminal_result(
            RunConclusion::Blocked,
            "file_budget.baseline_unavailable",
            &reason.message,
            evaluated_at_ms,
            Some(json!({
                "file_budget": {
                    "schema": "jig.file_budget/evidence-v1",
                    "policy_preparation": prepared.policy,
                    "comparison_preparation": prepared.comparison,
                    "request": prepared.request,
                    "view": prepared.view,
                    "configuration": prepared.configuration,
                    "complete": false,
                    "evaluated_at_ms": evaluated_at_ms,
                }
            })),
        ));
    }
    execute_prepared_file_budget(FileBudgetEngineContext {
        repository,
        prepared_input: &prepared,
        deadline,
        cancelled,
        mode,
    })
}

#[derive(Clone, Debug, Serialize)]
struct CandidateDigestFactV1 {
    change_kind: &'static str,
    current_path: String,
    baseline_path: Option<String>,
    disposition: String,
    current_content_digest: Option<String>,
    current: Option<MeasurementV1>,
    comparison_content_digest: Option<String>,
    comparison: Option<MeasurementV1>,
}

#[derive(Clone, Debug)]
struct MeasuredContentV1 {
    measurement: MeasurementV1,
    digest: String,
}

#[derive(Clone, Copy, Debug)]
struct MeasurementProgressV1 {
    candidate_count: u64,
    measured_total_bytes: u64,
}

#[derive(Clone, Copy, Debug)]
enum EngineStopV1 {
    Cancelled,
    TimedOut,
}

pub(super) fn execute_prepared_file_budget(
    context: FileBudgetEngineContext<'_>,
) -> Result<NativeActionResult> {
    match execute_ready_file_budget(&context) {
        Ok(result) => Ok(result),
        Err(error) => Ok(engine_error_result(&context, error)),
    }
}

mod engine;
mod report;

use engine::execute_ready_file_budget;
use report::{engine_error_result, file_budget_finding, result_with_findings, terminal_result};
#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::Duration;

    use tempfile::{TempDir, tempdir};

    use super::*;
    use crate::test_env::TestRepoBuilder;

    const SIMPLE_POLICY: &str = r#"version = 1

[[rules]]
id = "rust"
include = ["src/**"]
max_lines = 1
max_bytes = 8
"#;

    fn run_git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    fn initialized_repo(policy: &str, files: &[(&str, &[u8])]) -> (TempDir, RepoContext) {
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init", "-q", "-b", "main"]);
        run_git(temp.path(), &["config", "user.name", "Jig Test"]);
        run_git(
            temp.path(),
            &["config", "user.email", "jig@example.invalid"],
        );
        TestRepoBuilder::new(temp.path()).write();
        std::fs::create_dir_all(temp.path().join(".jig")).unwrap();
        std::fs::write(temp.path().join(POLICY_PATH_V1), policy).unwrap();
        for (path, bytes) in files {
            let full = temp.path().join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, bytes).unwrap();
        }
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-q", "-m", "fixture"]);
        let context = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
        (temp, context)
    }

    fn direct(
        context: &RepoContext,
        request: Option<ComparisonRequestV1>,
        configuration: NativeFileBudgetConfigV1,
        mode: FileBudgetEvaluationMode<'_>,
    ) -> NativeActionResult {
        run_direct_file_budget(
            context,
            request,
            configuration,
            mode,
            Instant::now() + Duration::from_secs(10),
            &|| false,
        )
        .unwrap()
    }

    fn finding_codes(result: &NativeActionResult) -> Vec<&str> {
        result
            .findings
            .iter()
            .filter_map(|finding| finding.code.as_deref())
            .collect()
    }

    #[test]
    fn worktree_engine_measures_arbitrary_bytes_and_returns_complete_evidence() {
        let (temp, context) = initialized_repo(SIMPLE_POLICY, &[("src/lib.rs", b"a\n")]);
        std::fs::write(temp.path().join("src/lib.rs"), b"a\0b\nc\n").unwrap();

        let result = direct(
            &context,
            Some(ComparisonRequestV1::MergeBaseRef {
                requested_ref: "main".into(),
            }),
            NativeFileBudgetConfigV1::default(),
            FileBudgetEvaluationMode::Check,
        );

        assert_eq!(result.conclusion, RunConclusion::Failure);
        assert!(finding_codes(&result).contains(&"file_budget.debt_growth_lines"));
        assert!(result.findings_digest.starts_with("sha256:"));
        let evidence = &result.evidence.as_ref().unwrap()["file_budget"];
        assert_eq!(evidence["complete"], true);
        assert_eq!(evidence["evaluated_file_count"], 1);
        assert!(
            evidence["evaluation_digest"]
                .as_str()
                .unwrap()
                .starts_with("sha256:")
        );
    }

    #[test]
    fn strict_inventory_includes_nonignored_untracked_files_and_tracked_only_can_narrow_it() {
        let (temp, context) = initialized_repo(SIMPLE_POLICY, &[("src/lib.rs", b"a\n")]);
        std::fs::write(temp.path().join("src/untracked.rs"), b"one\ntwo\n").unwrap();
        let request = Some(ComparisonRequestV1::StrictInventory {
            reason: jig_contract::StrictInventoryReasonV1::ExplicitAudit,
        });

        let all = direct(
            &context,
            request.clone(),
            NativeFileBudgetConfigV1::default(),
            FileBudgetEvaluationMode::Audit {
                tracked_only: false,
            },
        );
        assert_eq!(all.conclusion, RunConclusion::Failure);
        assert!(all.findings.iter().any(|finding| {
            finding
                .location
                .as_ref()
                .map(|location| location.path.as_str())
                == Some("src/untracked.rs")
        }));

        let tracked = direct(
            &context,
            request,
            NativeFileBudgetConfigV1::default(),
            FileBudgetEvaluationMode::Audit { tracked_only: true },
        );
        assert_eq!(tracked.conclusion, RunConclusion::Success);
    }

    #[test]
    fn unrelated_changes_cannot_hide_a_missing_waiver_target() {
        let policy = r#"version = 1

[[rules]]
id = "rust"
include = ["src/**"]
max_lines = 1

[[waivers]]
id = "legacy-large"
rule = "rust"
path = "src/waived.rs"
ceiling_lines = 3
reason = "temporary split"
expires = 2099-12-31
"#;
        let (temp, context) = initialized_repo(policy, &[("src/waived.rs", b"one\ntwo\n")]);
        std::fs::remove_file(temp.path().join("src/waived.rs")).unwrap();
        std::fs::write(temp.path().join("README.md"), "unrelated\n").unwrap();

        let result = direct(
            &context,
            Some(ComparisonRequestV1::MergeBaseRef {
                requested_ref: "main".into(),
            }),
            NativeFileBudgetConfigV1::default(),
            FileBudgetEvaluationMode::Check,
        );

        assert_eq!(result.conclusion, RunConclusion::Failure);
        assert!(finding_codes(&result).contains(&"file_budget.waiver_invalid"));
    }

    #[test]
    fn semantic_policy_changes_evaluate_the_whole_governed_set() {
        let baseline = r#"version = 1
[[rules]]
id = "rust"
include = ["src/**"]
max_lines = 100
"#;
        let (temp, context) = initialized_repo(
            baseline,
            &[("src/a.rs", b"one\ntwo\n"), ("src/b.rs", b"one\ntwo\n")],
        );
        std::fs::write(
            temp.path().join(POLICY_PATH_V1),
            "version = 1\n[[rules]]\nid = \"rust\"\ninclude = [\"src/**\"]\nmax_lines = 1\n",
        )
        .unwrap();

        let result = direct(
            &context,
            Some(ComparisonRequestV1::MergeBaseRef {
                requested_ref: "main".into(),
            }),
            NativeFileBudgetConfigV1::default(),
            FileBudgetEvaluationMode::Check,
        );

        assert_eq!(result.conclusion, RunConclusion::Success);
        assert_eq!(
            result.evidence.as_ref().unwrap()["file_budget"]["evaluated_file_count"],
            2
        );
        assert!(finding_codes(&result).contains(&"file_budget.policy_changed"));
    }

    #[test]
    fn candidate_ceiling_blocks_without_turning_incomplete_work_into_success() {
        let (_temp, context) =
            initialized_repo(SIMPLE_POLICY, &[("src/a.rs", b"a\n"), ("src/b.rs", b"b\n")]);
        let result = direct(
            &context,
            Some(ComparisonRequestV1::StrictInventory {
                reason: jig_contract::StrictInventoryReasonV1::ExplicitCheck,
            }),
            NativeFileBudgetConfigV1 {
                max_candidates: 1,
                ..NativeFileBudgetConfigV1::default()
            },
            FileBudgetEvaluationMode::Check,
        );

        assert_eq!(result.conclusion, RunConclusion::Blocked);
        assert_eq!(finding_codes(&result), ["file_budget.resource_limit"]);
    }

    #[test]
    fn aggregate_byte_ceiling_blocks_and_excluded_content_is_not_charged() {
        let (_temp, context) = initialized_repo(SIMPLE_POLICY, &[("src/lib.rs", b"abc\n")]);
        let request = Some(ComparisonRequestV1::StrictInventory {
            reason: jig_contract::StrictInventoryReasonV1::ExplicitCheck,
        });
        let blocked = direct(
            &context,
            request,
            NativeFileBudgetConfigV1 {
                max_total_bytes: 3,
                ..NativeFileBudgetConfigV1::default()
            },
            FileBudgetEvaluationMode::Check,
        );
        assert_eq!(blocked.conclusion, RunConclusion::Blocked);
        assert_eq!(finding_codes(&blocked), ["file_budget.resource_limit"]);

        let policy = r#"version = 1
[[rules]]
id = "rust"
include = ["src/**"]
max_lines = 1
[[exclusions]]
pattern = "src/vendor/**"
kind = "vendored"
reason = "fixture dependency"
"#;
        let (_temp, context) = initialized_repo(
            policy,
            &[("src/vendor/large.rs", b"content larger than one byte\n")],
        );
        let excluded = direct(
            &context,
            Some(ComparisonRequestV1::StrictInventory {
                reason: jig_contract::StrictInventoryReasonV1::ExplicitCheck,
            }),
            NativeFileBudgetConfigV1 {
                max_total_bytes: 1,
                ..NativeFileBudgetConfigV1::default()
            },
            FileBudgetEvaluationMode::Check,
        );
        assert_eq!(excluded.conclusion, RunConclusion::Success);
        let evidence = &excluded.evidence.as_ref().unwrap()["file_budget"];
        assert_eq!(evidence["excluded_file_count"], 1);
        assert_eq!(evidence["measured_total_bytes"], 0);
    }

    #[test]
    fn cancellation_and_deadline_remain_typed_engine_outcomes() {
        let (_temp, context) = initialized_repo(SIMPLE_POLICY, &[("src/lib.rs", b"a\n")]);
        let cancelled = run_direct_file_budget(
            &context,
            Some(ComparisonRequestV1::StrictInventory {
                reason: jig_contract::StrictInventoryReasonV1::ExplicitCheck,
            }),
            NativeFileBudgetConfigV1::default(),
            FileBudgetEvaluationMode::Check,
            Instant::now() + Duration::from_secs(10),
            &|| true,
        )
        .unwrap();
        assert_eq!(cancelled.conclusion, RunConclusion::Cancelled);

        let timed_out = run_direct_file_budget(
            &context,
            Some(ComparisonRequestV1::StrictInventory {
                reason: jig_contract::StrictInventoryReasonV1::ExplicitCheck,
            }),
            NativeFileBudgetConfigV1::default(),
            FileBudgetEvaluationMode::Check,
            Instant::now(),
            &|| false,
        )
        .unwrap();
        assert_eq!(timed_out.conclusion, RunConclusion::TimedOut);
    }

    #[test]
    fn finding_preview_is_bounded_without_hiding_complete_count_or_digest() {
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init", "-q", "-b", "main"]);
        TestRepoBuilder::new(temp.path()).write();
        std::fs::create_dir_all(temp.path().join(".jig")).unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join(POLICY_PATH_V1), SIMPLE_POLICY).unwrap();
        for index in 0..(FINDING_PREVIEW_LIMIT_V1 + 4) {
            std::fs::write(
                temp.path().join(format!("src/example-{index:03}.rs")),
                b"one\ntwo\n",
            )
            .unwrap();
        }
        let context = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
        let result = direct(
            &context,
            Some(ComparisonRequestV1::StrictInventory {
                reason: jig_contract::StrictInventoryReasonV1::ExplicitCheck,
            }),
            NativeFileBudgetConfigV1::default(),
            FileBudgetEvaluationMode::Check,
        );

        assert_eq!(result.conclusion, RunConclusion::Failure);
        assert_eq!(result.finding_count, (FINDING_PREVIEW_LIMIT_V1 + 4) as u64);
        assert_eq!(result.findings.len(), FINDING_PREVIEW_LIMIT_V1);
        assert!(result.findings_truncated);
        assert!(result.findings_digest.starts_with("sha256:"));
        assert!(result.human_output.contains("omitted=4"));
    }

    #[test]
    fn zero_selector_in_an_unborn_repository_uses_exact_empty_tree_authority() {
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init", "-q", "-b", "main"]);
        TestRepoBuilder::new(temp.path()).write();
        std::fs::create_dir_all(temp.path().join(".jig")).unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join(POLICY_PATH_V1), SIMPLE_POLICY).unwrap();
        std::fs::write(temp.path().join("src/lib.rs"), b"a\n").unwrap();
        let context = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let result = direct(
            &context,
            None,
            NativeFileBudgetConfigV1::default(),
            FileBudgetEvaluationMode::Check,
        );

        assert_eq!(result.conclusion, RunConclusion::Success);
        let comparison = &result.evidence.as_ref().unwrap()["file_budget"]["comparison"];
        assert_eq!(comparison["kind"], "exact_tree");
        assert_eq!(comparison["provenance"], "unborn_worktree");
    }

    #[test]
    fn staged_mode_reads_index_bytes_and_ignores_later_worktree_content() {
        let (temp, context) = initialized_repo(SIMPLE_POLICY, &[("src/lib.rs", b"a\n")]);
        std::fs::write(temp.path().join("src/lib.rs"), b"one\ntwo\n").unwrap();
        run_git(temp.path(), &["add", "src/lib.rs"]);
        std::fs::write(temp.path().join("src/lib.rs"), b"a\n").unwrap();

        let result = direct(
            &context,
            Some(ComparisonRequestV1::IndexAgainstHead),
            NativeFileBudgetConfigV1::default(),
            FileBudgetEvaluationMode::Check,
        );

        assert_eq!(result.conclusion, RunConclusion::Failure);
        assert!(finding_codes(&result).contains(&"file_budget.debt_growth_lines"));
        assert_eq!(
            result.evidence.as_ref().unwrap()["file_budget"]["view"],
            "index"
        );
    }

    #[test]
    fn explain_adds_an_unchanged_exact_path_with_current_and_baseline_measurements() {
        let (_temp, context) = initialized_repo(SIMPLE_POLICY, &[("src/lib.rs", b"a\n")]);
        let result = direct(
            &context,
            Some(ComparisonRequestV1::MergeBaseRef {
                requested_ref: "main".into(),
            }),
            NativeFileBudgetConfigV1::default(),
            FileBudgetEvaluationMode::Explain { path: "src/lib.rs" },
        );

        assert_eq!(result.conclusion, RunConclusion::Success);
        let details = result.evidence.as_ref().unwrap()["file_budget"]["candidate_details"]
            .as_array()
            .unwrap();
        assert_eq!(details.len(), 1);
        assert_eq!(details[0]["current_path"], "src/lib.rs");
        assert_eq!(details[0]["disposition"], "governed:rust");
        assert_eq!(details[0]["current"]["lines"], 1);
        assert_eq!(details[0]["comparison"]["lines"], 1);
        assert!(result.human_output.contains("disposition=governed:rust"));
        assert!(result.human_output.contains("current_lines=1"));
        assert!(result.human_output.contains("comparison_lines=1"));
    }

    #[test]
    fn validate_checks_real_inventory_paths_for_rule_ambiguity_without_measurement() {
        let ambiguous = r#"version = 1
[[rules]]
id = "first"
include = ["src/**"]
max_lines = 1
[[rules]]
id = "second"
include = ["src/**"]
max_lines = 1
"#;
        let (_temp, context) = initialized_repo(ambiguous, &[("src/lib.rs", b"a\n")]);
        let result = direct(
            &context,
            Some(ComparisonRequestV1::StrictInventory {
                reason: jig_contract::StrictInventoryReasonV1::ExplicitAudit,
            }),
            NativeFileBudgetConfigV1::default(),
            FileBudgetEvaluationMode::Validate,
        );

        assert_eq!(result.conclusion, RunConclusion::Failure);
        assert!(finding_codes(&result).contains(&"file_budget.rule_ambiguous"));
        assert_eq!(
            result.evidence.as_ref().unwrap()["file_budget"]["operation"],
            "validate"
        );
    }
}
