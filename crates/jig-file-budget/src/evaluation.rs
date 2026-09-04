use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::diagnostic::{
    BudgetDiagnosticCodeV1, BudgetDiagnosticV1, BudgetMetricV1, BudgetSeverityV1, sort_diagnostics,
};
use crate::measurement::MeasurementV1;
use crate::policy::{MAX_WAIVERS_V1, PathDispositionV1, PolicyDateV1, PolicyV1, RuleV1, WaiverV1};

use self::waiver::{
    index_files, index_waiver_facts, record_policy_changes, record_removed_waiver_debt,
    validate_current_waivers,
};

mod waiver;

const POLICY_PATH: &str = ".jig/file-budget.toml";
pub const MAX_WAIVER_TARGET_FACTS_V1: usize = MAX_WAIVERS_V1 * 2;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedFileKindV1 {
    Symlink,
    Gitlink,
    Special,
    ChangedDuringRead,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExactCurrentPathStateV1 {
    Regular,
    Missing,
    Unsupported(UnsupportedFileKindV1),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExactCurrentPathFactV1 {
    pub path: String,
    pub state: ExactCurrentPathStateV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CurrentFileStateV1 {
    Regular(MeasurementV1),
    Unsupported(UnsupportedFileKindV1),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluateFileV1 {
    pub current_path: String,
    pub baseline_path: Option<String>,
    pub current: CurrentFileStateV1,
    pub comparison: Option<MeasurementV1>,
}

#[derive(Clone, Copy, Debug)]
pub enum ComparisonPolicyV1<'a> {
    Absent,
    Present(&'a PolicyV1),
    Unavailable,
}

#[derive(Clone, Copy, Debug)]
pub struct EvaluationInputV1<'a> {
    pub policy: &'a PolicyV1,
    pub comparison_policy: ComparisonPolicyV1<'a>,
    pub current_date: PolicyDateV1,
    pub waiver_targets: &'a [ExactCurrentPathFactV1],
    pub files: &'a [EvaluateFileV1],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BudgetEvaluationV1 {
    pub diagnostics: Vec<BudgetDiagnosticV1>,
    pub evaluated_files: u64,
    pub excluded_files: u64,
    pub waived_files: u64,
}

pub(super) struct FileIndexesV1<'a> {
    pub(super) duplicate_paths: BTreeSet<&'a str>,
    pub(super) duplicate_baseline_paths: BTreeSet<&'a str>,
    pub(super) by_current: BTreeMap<&'a str, &'a EvaluateFileV1>,
    pub(super) by_baseline: BTreeMap<&'a str, Vec<&'a EvaluateFileV1>>,
}

impl BudgetEvaluationV1 {
    #[must_use]
    pub fn passed(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == BudgetSeverityV1::Error)
    }
}

pub fn evaluate_v1(input: EvaluationInputV1<'_>) -> BudgetEvaluationV1 {
    let mut diagnostics = Vec::new();
    if matches!(input.comparison_policy, ComparisonPolicyV1::Unavailable) {
        diagnostics.push(
            BudgetDiagnosticV1::new(
                BudgetSeverityV1::Error,
                BudgetDiagnosticCodeV1::BaselineUnavailable,
                "comparison-side policy authority is unavailable",
            )
            .at_path(POLICY_PATH),
        );
    }
    let file_indexes = index_files(input.files);
    let waiver_fact_limit_exceeded = input.waiver_targets.len() > MAX_WAIVER_TARGET_FACTS_V1;
    let waiver_facts = if waiver_fact_limit_exceeded {
        BTreeMap::new()
    } else {
        index_waiver_facts(input.waiver_targets)
    };
    let invalid_waivers = validate_current_waivers(&input, &waiver_facts, &mut diagnostics);
    record_policy_changes(&input, &mut diagnostics);
    if !waiver_fact_limit_exceeded {
        record_removed_waiver_debt(&input, &file_indexes, &waiver_facts, &mut diagnostics);
    }

    for path in &file_indexes.duplicate_paths {
        diagnostics.push(
            BudgetDiagnosticV1::new(
                BudgetSeverityV1::Error,
                BudgetDiagnosticCodeV1::ScopeIncomplete,
                format!("current evaluation facts contain duplicate path `{path}`"),
            )
            .at_path(*path),
        );
    }
    for path in &file_indexes.duplicate_baseline_paths {
        diagnostics.push(
            BudgetDiagnosticV1::new(
                BudgetSeverityV1::Error,
                BudgetDiagnosticCodeV1::ScopeIncomplete,
                format!("comparison path `{path}` is claimed by multiple current files"),
            )
            .at_path(*path),
        );
    }

    let mut evaluated_files = 0_u64;
    let mut excluded_files = 0_u64;
    let mut waived_files = 0_u64;
    for file in input.files {
        if file_indexes
            .duplicate_paths
            .contains(file.current_path.as_str())
            || file
                .baseline_path
                .as_deref()
                .is_some_and(|path| file_indexes.duplicate_baseline_paths.contains(path))
        {
            continue;
        }
        let disposition = match input.policy.classify_path(&file.current_path) {
            Ok(disposition) => disposition,
            Err(diagnostic) => {
                diagnostics.push(*diagnostic);
                continue;
            }
        };
        let rule = match disposition {
            PathDispositionV1::Outside => continue,
            PathDispositionV1::Excluded(_) | PathDispositionV1::LocallyExcluded => {
                excluded_files = excluded_files.saturating_add(1);
                continue;
            }
            PathDispositionV1::Governed(rule) => rule,
        };
        evaluated_files = evaluated_files.saturating_add(1);
        let current = match file.current {
            CurrentFileStateV1::Regular(current) => current,
            CurrentFileStateV1::Unsupported(kind) => {
                diagnostics.push(unsupported_diagnostic(&file.current_path, &rule.id, kind));
                continue;
            }
        };
        if file.baseline_path.is_some() != file.comparison.is_some() {
            diagnostics.push(
                BudgetDiagnosticV1::new(
                    BudgetSeverityV1::Error,
                    BudgetDiagnosticCodeV1::BaselineUnavailable,
                    "baseline path and comparison measurement must either both be supplied or both be absent",
                )
                .at_path(file.current_path.clone())
                .for_rule(rule.id.clone()),
            );
            continue;
        }
        let waiver = input
            .policy
            .waiver_for(&rule.id, &file.current_path)
            .filter(|waiver| !invalid_waivers.contains(waiver.id.as_str()));
        let mut exercised_waiver = false;
        evaluate_metric(
            &mut diagnostics,
            file,
            rule,
            waiver,
            BudgetMetricV1::Lines,
            current.lines,
            file.comparison.map(|measurement| measurement.lines),
            rule.notice_lines,
            rule.warn_lines,
            rule.max_lines,
            waiver.and_then(|waiver| waiver.ceiling_lines),
            &mut exercised_waiver,
        );
        evaluate_metric(
            &mut diagnostics,
            file,
            rule,
            waiver,
            BudgetMetricV1::Bytes,
            current.bytes,
            file.comparison.map(|measurement| measurement.bytes),
            rule.notice_bytes,
            rule.warn_bytes,
            rule.max_bytes,
            waiver.and_then(|waiver| waiver.ceiling_bytes),
            &mut exercised_waiver,
        );
        if exercised_waiver {
            let waiver = waiver.expect("an exercised waiver exists");
            waived_files = waived_files.saturating_add(1);
            diagnostics.push(
                BudgetDiagnosticV1::new(
                    BudgetSeverityV1::Warning,
                    BudgetDiagnosticCodeV1::WaiverActive,
                    format!(
                        "waiver `{}` authorizes bounded debt through {}",
                        waiver.id, waiver.expires
                    ),
                )
                .at_path(file.current_path.clone())
                .for_rule(rule.id.clone())
                .for_waiver(waiver.id.clone()),
            );
        }
    }
    sort_diagnostics(&mut diagnostics);
    BudgetEvaluationV1 {
        diagnostics,
        evaluated_files,
        excluded_files,
        waived_files,
    }
}

#[allow(clippy::too_many_arguments)]
fn evaluate_metric(
    diagnostics: &mut Vec<BudgetDiagnosticV1>,
    file: &EvaluateFileV1,
    rule: &RuleV1,
    waiver: Option<&WaiverV1>,
    metric: BudgetMetricV1,
    current: u64,
    comparison: Option<u64>,
    notice: Option<u64>,
    warning: Option<u64>,
    maximum: Option<u64>,
    waiver_ceiling: Option<u64>,
    exercised_waiver: &mut bool,
) {
    if let Some(notice) = notice
        && current > notice
    {
        diagnostics.push(metric_diagnostic(
            BudgetSeverityV1::Notice,
            notice_code(metric),
            format!(
                "{} measurement {current} exceeds notice threshold {notice}",
                metric_name(metric)
            ),
            file,
            rule,
            metric,
            current,
            comparison,
            notice,
        ));
    }
    if let Some(warning) = warning
        && current > warning
    {
        diagnostics.push(metric_diagnostic(
            BudgetSeverityV1::Warning,
            warning_code(metric),
            format!(
                "{} measurement {current} exceeds warning threshold {warning}",
                metric_name(metric)
            ),
            file,
            rule,
            metric,
            current,
            comparison,
            warning,
        ));
    }
    let Some(maximum) = maximum else {
        return;
    };
    if let Some(ceiling) = waiver_ceiling
        && current > ceiling
    {
        let mut diagnostic = metric_diagnostic(
            BudgetSeverityV1::Error,
            maximum_code(metric),
            format!(
                "{} measurement {current} exceeds waiver ceiling {ceiling}",
                metric_name(metric)
            ),
            file,
            rule,
            metric,
            current,
            comparison,
            ceiling,
        );
        diagnostic.debt = Some(current.saturating_sub(maximum));
        if let Some(waiver) = waiver {
            diagnostic.waiver_id = Some(waiver.id.clone());
        }
        diagnostics.push(diagnostic);
        return;
    }

    let debt = current.saturating_sub(maximum);
    if debt == 0 {
        if let Some(comparison) = comparison {
            let retired = comparison.saturating_sub(maximum);
            if retired > 0 {
                let mut diagnostic = metric_diagnostic(
                    BudgetSeverityV1::Notice,
                    BudgetDiagnosticCodeV1::DebtImproved,
                    format!(
                        "{} debt improved by {retired} and is now retired",
                        metric_name(metric)
                    ),
                    file,
                    rule,
                    metric,
                    current,
                    Some(comparison),
                    maximum,
                );
                diagnostic.debt = Some(0);
                diagnostics.push(diagnostic);
            }
        }
        return;
    }

    if waiver_ceiling.is_some() {
        *exercised_waiver = true;
        return;
    }

    let Some(comparison) = comparison else {
        let mut diagnostic = metric_diagnostic(
            BudgetSeverityV1::Error,
            maximum_code(metric),
            format!(
                "new file {} measurement {current} exceeds maximum {maximum}",
                metric_name(metric)
            ),
            file,
            rule,
            metric,
            current,
            None,
            maximum,
        );
        diagnostic.debt = Some(debt);
        diagnostics.push(diagnostic);
        return;
    };
    let comparison_debt = comparison.saturating_sub(maximum);
    if debt > comparison_debt {
        let growth = debt - comparison_debt;
        let mut diagnostic = metric_diagnostic(
            BudgetSeverityV1::Error,
            debt_growth_code(metric),
            format!(
                "{} debt grew by {growth}, from {comparison_debt} to {debt}",
                metric_name(metric)
            ),
            file,
            rule,
            metric,
            current,
            Some(comparison),
            maximum,
        );
        diagnostic.debt = Some(debt);
        diagnostic.debt_growth = Some(growth);
        diagnostics.push(diagnostic);
    } else if debt == comparison_debt {
        let mut diagnostic = metric_diagnostic(
            BudgetSeverityV1::Warning,
            BudgetDiagnosticCodeV1::LegacyDebt,
            format!(
                "{} legacy debt remains unchanged at {debt}",
                metric_name(metric)
            ),
            file,
            rule,
            metric,
            current,
            Some(comparison),
            maximum,
        );
        diagnostic.debt = Some(debt);
        diagnostics.push(diagnostic);
    } else {
        let improvement = comparison_debt - debt;
        let mut diagnostic = metric_diagnostic(
            BudgetSeverityV1::Notice,
            BudgetDiagnosticCodeV1::DebtImproved,
            format!(
                "{} debt improved by {improvement}, from {comparison_debt} to {debt}",
                metric_name(metric)
            ),
            file,
            rule,
            metric,
            current,
            Some(comparison),
            maximum,
        );
        diagnostic.debt = Some(debt);
        diagnostics.push(diagnostic);
    }
}

#[allow(clippy::too_many_arguments)]
fn metric_diagnostic(
    severity: BudgetSeverityV1,
    code: BudgetDiagnosticCodeV1,
    message: String,
    file: &EvaluateFileV1,
    rule: &RuleV1,
    metric: BudgetMetricV1,
    current: u64,
    comparison: Option<u64>,
    limit: u64,
) -> BudgetDiagnosticV1 {
    let mut diagnostic = BudgetDiagnosticV1::new(severity, code, message)
        .at_path(file.current_path.clone())
        .for_rule(rule.id.clone());
    diagnostic.metric = Some(metric);
    diagnostic.current = Some(current);
    diagnostic.comparison = comparison;
    diagnostic.limit = Some(limit);
    diagnostic
}

fn unsupported_diagnostic(
    path: &str,
    rule_id: &str,
    kind: UnsupportedFileKindV1,
) -> BudgetDiagnosticV1 {
    let code = if kind == UnsupportedFileKindV1::ChangedDuringRead {
        BudgetDiagnosticCodeV1::ChangedDuringRead
    } else {
        BudgetDiagnosticCodeV1::UnsupportedFile
    };
    BudgetDiagnosticV1::new(
        BudgetSeverityV1::Error,
        code,
        format!("governed path is unsupported ({})", unsupported_name(kind)),
    )
    .at_path(path)
    .for_rule(rule_id)
}

const fn maximum_code(metric: BudgetMetricV1) -> BudgetDiagnosticCodeV1 {
    match metric {
        BudgetMetricV1::Lines => BudgetDiagnosticCodeV1::MaxLines,
        BudgetMetricV1::Bytes => BudgetDiagnosticCodeV1::MaxBytes,
    }
}

const fn debt_growth_code(metric: BudgetMetricV1) -> BudgetDiagnosticCodeV1 {
    match metric {
        BudgetMetricV1::Lines => BudgetDiagnosticCodeV1::DebtGrowthLines,
        BudgetMetricV1::Bytes => BudgetDiagnosticCodeV1::DebtGrowthBytes,
    }
}

const fn notice_code(metric: BudgetMetricV1) -> BudgetDiagnosticCodeV1 {
    match metric {
        BudgetMetricV1::Lines => BudgetDiagnosticCodeV1::NoticeLines,
        BudgetMetricV1::Bytes => BudgetDiagnosticCodeV1::NoticeBytes,
    }
}

const fn warning_code(metric: BudgetMetricV1) -> BudgetDiagnosticCodeV1 {
    match metric {
        BudgetMetricV1::Lines => BudgetDiagnosticCodeV1::WarningLines,
        BudgetMetricV1::Bytes => BudgetDiagnosticCodeV1::WarningBytes,
    }
}

pub(super) const fn metric_name(metric: BudgetMetricV1) -> &'static str {
    match metric {
        BudgetMetricV1::Lines => "line",
        BudgetMetricV1::Bytes => "byte",
    }
}

pub(super) const fn unsupported_name(kind: UnsupportedFileKindV1) -> &'static str {
    match kind {
        UnsupportedFileKindV1::Symlink => "symlink",
        UnsupportedFileKindV1::Gitlink => "gitlink",
        UnsupportedFileKindV1::Special => "special file",
        UnsupportedFileKindV1::ChangedDuringRead => "changed during read",
    }
}
