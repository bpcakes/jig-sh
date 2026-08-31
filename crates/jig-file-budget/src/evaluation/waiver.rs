use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostic::{
    BudgetDiagnosticCodeV1, BudgetDiagnosticV1, BudgetMetricV1, BudgetSeverityV1,
};
use crate::policy::{PathDispositionV1, WaiverV1};

use super::{
    ComparisonPolicyV1, CurrentFileStateV1, EvaluateFileV1, EvaluationInputV1,
    ExactCurrentPathFactV1, ExactCurrentPathStateV1, FileIndexesV1, MAX_WAIVER_TARGET_FACTS_V1,
    POLICY_PATH, metric_name, unsupported_name,
};

pub(super) fn validate_current_waivers(
    input: &EvaluationInputV1<'_>,
    facts: &BTreeMap<&str, Vec<&ExactCurrentPathFactV1>>,
    diagnostics: &mut Vec<BudgetDiagnosticV1>,
) -> BTreeSet<String> {
    if input.waiver_targets.len() > MAX_WAIVER_TARGET_FACTS_V1 {
        diagnostics.push(BudgetDiagnosticV1::new(
            BudgetSeverityV1::Error,
            BudgetDiagnosticCodeV1::ResourceLimit,
            format!(
                "supplied {} waiver-target facts; version 1 permits at most {MAX_WAIVER_TARGET_FACTS_V1}",
                input.waiver_targets.len()
            ),
        ));
        return input
            .policy
            .waivers()
            .iter()
            .map(|waiver| waiver.id.clone())
            .collect();
    }
    let mut invalid = BTreeSet::new();
    for waiver in input.policy.waivers() {
        let error = if waiver.expires < input.current_date {
            Some(format!(
                "waiver `{}` expired after {} and is not active on {}",
                waiver.id, waiver.expires, input.current_date
            ))
        } else {
            match facts.get(waiver.path.as_str()).map(Vec::as_slice) {
                None | Some([]) => Some(format!(
                    "waiver `{}` has no supplied exact current-view target fact",
                    waiver.id
                )),
                Some([fact]) => match fact.state {
                    ExactCurrentPathStateV1::Regular => {
                        match input.policy.classify_path(&waiver.path) {
                            Ok(PathDispositionV1::Governed(rule)) if rule.id == waiver.rule => None,
                            Ok(PathDispositionV1::Governed(rule)) => Some(format!(
                                "waiver `{}` target matches rule `{}`, not `{}`",
                                waiver.id, rule.id, waiver.rule
                            )),
                            Ok(
                                PathDispositionV1::Outside
                                | PathDispositionV1::Excluded(_)
                                | PathDispositionV1::LocallyExcluded,
                            ) => Some(format!(
                                "waiver `{}` target is no longer governed by rule `{}`",
                                waiver.id, waiver.rule
                            )),
                            Err(_) => Some(format!(
                                "waiver `{}` target does not match exactly one effective rule",
                                waiver.id
                            )),
                        }
                    }
                    ExactCurrentPathStateV1::Missing => Some(format!(
                        "waiver `{}` exact current-view target is missing",
                        waiver.id
                    )),
                    ExactCurrentPathStateV1::Unsupported(kind) => Some(format!(
                        "waiver `{}` exact current-view target is unsupported ({})",
                        waiver.id,
                        unsupported_name(kind)
                    )),
                },
                Some(_) => Some(format!(
                    "waiver `{}` has duplicate supplied exact current-view target facts",
                    waiver.id
                )),
            }
        };
        if let Some(message) = error {
            invalid.insert(waiver.id.clone());
            let code = if waiver.expires < input.current_date {
                BudgetDiagnosticCodeV1::WaiverExpired
            } else {
                BudgetDiagnosticCodeV1::WaiverInvalid
            };
            diagnostics.push(
                BudgetDiagnosticV1::new(BudgetSeverityV1::Error, code, message)
                    .at_path(waiver.path.clone())
                    .for_rule(waiver.rule.clone())
                    .for_waiver(waiver.id.clone()),
            );
        }
    }
    let mut waiver_paths = input
        .policy
        .waivers()
        .iter()
        .map(|waiver| waiver.path.as_str())
        .collect::<BTreeSet<_>>();
    if let ComparisonPolicyV1::Present(comparison_policy) = input.comparison_policy {
        waiver_paths.extend(
            comparison_policy
                .waivers()
                .iter()
                .map(|waiver| waiver.path.as_str()),
        );
    }
    for fact in input.waiver_targets {
        if !waiver_paths.contains(fact.path.as_str()) {
            diagnostics.push(
                BudgetDiagnosticV1::new(
                    BudgetSeverityV1::Error,
                    BudgetDiagnosticCodeV1::ScopeIncomplete,
                    "supplied waiver-target fact does not correspond to a current waiver",
                )
                .at_path(fact.path.clone()),
            );
        }
    }
    invalid
}

pub(super) fn record_policy_changes(
    input: &EvaluationInputV1<'_>,
    diagnostics: &mut Vec<BudgetDiagnosticV1>,
) {
    let comparison_policy = match input.comparison_policy {
        ComparisonPolicyV1::Absent => {
            if input.files.iter().any(|file| file.comparison.is_some()) {
                diagnostics.push(
                    BudgetDiagnosticV1::new(
                        BudgetSeverityV1::Notice,
                        BudgetDiagnosticCodeV1::PolicyChanged,
                        "current policy is absent from the comparison side",
                    )
                    .at_path(POLICY_PATH),
                );
            }
            return;
        }
        ComparisonPolicyV1::Present(policy) => policy,
        ComparisonPolicyV1::Unavailable => return,
    };
    if input.policy.identity().semantic_sha256() == comparison_policy.identity().semantic_sha256() {
        return;
    }
    diagnostics.push(
        BudgetDiagnosticV1::new(
            BudgetSeverityV1::Notice,
            BudgetDiagnosticCodeV1::PolicyChanged,
            "current semantic policy differs from comparison-side policy",
        )
        .at_path(POLICY_PATH),
    );
    let current_by_id = input
        .policy
        .waivers()
        .iter()
        .map(|waiver| (waiver.id.as_str(), waiver))
        .collect::<BTreeMap<_, _>>();
    let comparison_by_id = comparison_policy
        .waivers()
        .iter()
        .map(|waiver| (waiver.id.as_str(), waiver))
        .collect::<BTreeMap<_, _>>();
    let ids = current_by_id
        .keys()
        .chain(comparison_by_id.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    for id in ids {
        let current = current_by_id.get(id).copied();
        let comparison = comparison_by_id.get(id).copied();
        if current == comparison {
            continue;
        }
        let (path, rule, message) = match (comparison, current) {
            (None, Some(current)) => (
                current.path.as_str(),
                current.rule.as_str(),
                format!("waiver `{id}` was added as current authorization"),
            ),
            (Some(comparison), None) => (
                comparison.path.as_str(),
                comparison.rule.as_str(),
                format!("waiver `{id}` was removed from current authorization"),
            ),
            (Some(comparison), Some(current)) => {
                let kind = if comparison.path != current.path || comparison.rule != current.rule {
                    "target transferred"
                } else if comparison.expires != current.expires {
                    "expiry changed"
                } else if comparison.ceiling_lines != current.ceiling_lines
                    || comparison.ceiling_bytes != current.ceiling_bytes
                {
                    "ceiling changed"
                } else {
                    "reason changed"
                };
                (
                    current.path.as_str(),
                    current.rule.as_str(),
                    format!("waiver `{id}` {kind}"),
                )
            }
            (None, None) => unreachable!(),
        };
        diagnostics.push(
            BudgetDiagnosticV1::new(
                BudgetSeverityV1::Notice,
                BudgetDiagnosticCodeV1::PolicyChanged,
                message,
            )
            .at_path(path)
            .for_rule(rule)
            .for_waiver(id),
        );
    }
}

pub(super) fn record_removed_waiver_debt(
    input: &EvaluationInputV1<'_>,
    files: &FileIndexesV1<'_>,
    facts: &BTreeMap<&str, Vec<&ExactCurrentPathFactV1>>,
    diagnostics: &mut Vec<BudgetDiagnosticV1>,
) {
    let comparison_policy = match input.comparison_policy {
        ComparisonPolicyV1::Present(policy) => policy,
        ComparisonPolicyV1::Absent | ComparisonPolicyV1::Unavailable => return,
    };
    for historical in comparison_policy.waivers() {
        let current = input.policy.waiver(&historical.id);
        let target_continues = current
            .is_some_and(|current| waiver_transfer_follows_ancestry(current, historical, files));
        let removed_lines = historical.ceiling_lines.is_some()
            && (!target_continues || current.is_none_or(|waiver| waiver.ceiling_lines.is_none()));
        let removed_bytes = historical.ceiling_bytes.is_some()
            && (!target_continues || current.is_none_or(|waiver| waiver.ceiling_bytes.is_none()));
        if !removed_lines && !removed_bytes {
            continue;
        }
        let mut affected = BTreeMap::<&str, &EvaluateFileV1>::new();
        if let Some(file) = files.by_current.get(historical.path.as_str()) {
            affected.insert(file.current_path.as_str(), *file);
        }
        if let Some(renamed) = files.by_baseline.get(historical.path.as_str()) {
            for file in renamed {
                affected.insert(file.current_path.as_str(), *file);
            }
        }
        let has_governed_affected_path = affected.values().any(|file| {
            matches!(
                input.policy.classify_path(&file.current_path),
                Ok(PathDispositionV1::Governed(_))
            )
        });
        if !target_continues
            && !has_governed_affected_path
            && matches!(
                input.policy.classify_path(&historical.path),
                Ok(PathDispositionV1::Outside
                    | PathDispositionV1::Excluded(_)
                    | PathDispositionV1::LocallyExcluded)
            )
        {
            continue;
        }
        if target_continues {
            if affected.is_empty() {
                diagnostics.push(historical_waiver_diagnostic(
                    BudgetDiagnosticCodeV1::ScopeIncomplete,
                    historical,
                    "removed waiver metric target has no supplied measurement fact",
                ));
            }
        } else {
            validate_historical_waiver_fact(historical, &affected, facts, diagnostics);
        }
        for file in affected.into_values() {
            let CurrentFileStateV1::Regular(measurement) = file.current else {
                continue;
            };
            let Ok(PathDispositionV1::Governed(rule)) =
                input.policy.classify_path(&file.current_path)
            else {
                continue;
            };
            for (removed, metric, current, comparison, maximum) in [
                (
                    removed_lines,
                    BudgetMetricV1::Lines,
                    measurement.lines,
                    file.comparison.map(|value| value.lines),
                    rule.max_lines,
                ),
                (
                    removed_bytes,
                    BudgetMetricV1::Bytes,
                    measurement.bytes,
                    file.comparison.map(|value| value.bytes),
                    rule.max_bytes,
                ),
            ] {
                let Some(maximum) = maximum else {
                    continue;
                };
                if removed && current > maximum {
                    let mut diagnostic = BudgetDiagnosticV1::new(
                        BudgetSeverityV1::Error,
                        BudgetDiagnosticCodeV1::WaiverRemovedWithDebt,
                        format!(
                            "historical waiver `{}` no longer authorizes {} debt",
                            historical.id,
                            metric_name(metric)
                        ),
                    )
                    .at_path(file.current_path.clone())
                    .for_rule(rule.id.clone())
                    .for_waiver(historical.id.clone());
                    diagnostic.metric = Some(metric);
                    diagnostic.current = Some(current);
                    diagnostic.comparison = comparison;
                    diagnostic.limit = Some(maximum);
                    diagnostic.debt = Some(current - maximum);
                    diagnostics.push(diagnostic);
                }
            }
        }
    }
}

fn waiver_transfer_follows_ancestry(
    current: &WaiverV1,
    historical: &WaiverV1,
    files: &FileIndexesV1<'_>,
) -> bool {
    if current.path == historical.path {
        return true;
    }
    files
        .by_baseline
        .get(historical.path.as_str())
        .is_some_and(|renamed| renamed.iter().any(|file| file.current_path == current.path))
}

pub(super) fn index_files(files: &[EvaluateFileV1]) -> FileIndexesV1<'_> {
    let mut counts = BTreeMap::<&str, usize>::new();
    for file in files {
        *counts.entry(&file.current_path).or_default() += 1;
    }
    let duplicate_paths = counts
        .into_iter()
        .filter_map(|(path, count)| (count > 1).then_some(path))
        .collect::<BTreeSet<_>>();
    let mut baseline_counts = BTreeMap::<&str, usize>::new();
    for file in files {
        if duplicate_paths.contains(file.current_path.as_str()) {
            continue;
        }
        if let Some(path) = file.baseline_path.as_deref() {
            *baseline_counts.entry(path).or_default() += 1;
        }
    }
    let duplicate_baseline_paths = baseline_counts
        .into_iter()
        .filter_map(|(path, count)| (count > 1).then_some(path))
        .collect::<BTreeSet<_>>();
    let mut by_current = BTreeMap::new();
    let mut by_baseline = BTreeMap::<&str, Vec<&EvaluateFileV1>>::new();
    for file in files {
        if duplicate_paths.contains(file.current_path.as_str())
            || file
                .baseline_path
                .as_deref()
                .is_some_and(|path| duplicate_baseline_paths.contains(path))
        {
            continue;
        }
        by_current.insert(file.current_path.as_str(), file);
        if let Some(path) = file.baseline_path.as_deref() {
            by_baseline.entry(path).or_default().push(file);
        }
    }
    FileIndexesV1 {
        duplicate_paths,
        duplicate_baseline_paths,
        by_current,
        by_baseline,
    }
}

pub(super) fn index_waiver_facts(
    facts: &[ExactCurrentPathFactV1],
) -> BTreeMap<&str, Vec<&ExactCurrentPathFactV1>> {
    let mut by_path = BTreeMap::<&str, Vec<&ExactCurrentPathFactV1>>::new();
    for fact in facts {
        by_path.entry(&fact.path).or_default().push(fact);
    }
    by_path
}

fn validate_historical_waiver_fact(
    historical: &WaiverV1,
    affected: &BTreeMap<&str, &EvaluateFileV1>,
    facts: &BTreeMap<&str, Vec<&ExactCurrentPathFactV1>>,
    diagnostics: &mut Vec<BudgetDiagnosticV1>,
) {
    let fact = match facts.get(historical.path.as_str()).map(Vec::as_slice) {
        Some([fact]) => *fact,
        None | Some([]) => {
            diagnostics.push(historical_waiver_diagnostic(
                BudgetDiagnosticCodeV1::WaiverInvalid,
                historical,
                "removed historical waiver has no supplied exact current-view target fact",
            ));
            return;
        }
        Some(_) => {
            diagnostics.push(historical_waiver_diagnostic(
                BudgetDiagnosticCodeV1::WaiverInvalid,
                historical,
                "removed historical waiver has duplicate exact current-view target facts",
            ));
            return;
        }
    };
    let current_at_historical_path = affected.get(historical.path.as_str());
    match fact.state {
        ExactCurrentPathStateV1::Regular => match current_at_historical_path {
            Some(file) if matches!(file.current, CurrentFileStateV1::Regular(_)) => {}
            Some(_) => diagnostics.push(historical_waiver_diagnostic(
                BudgetDiagnosticCodeV1::ScopeIncomplete,
                historical,
                "historical waiver target is reported regular but its file fact is unsupported",
            )),
            None => diagnostics.push(historical_waiver_diagnostic(
                BudgetDiagnosticCodeV1::ScopeIncomplete,
                historical,
                "historical waiver target is regular but has no supplied measurement fact",
            )),
        },
        ExactCurrentPathStateV1::Missing => {
            if current_at_historical_path.is_some() {
                diagnostics.push(historical_waiver_diagnostic(
                    BudgetDiagnosticCodeV1::ScopeIncomplete,
                    historical,
                    "historical waiver target is reported missing but also has a current file fact",
                ));
            }
        }
        ExactCurrentPathStateV1::Unsupported(kind) => {
            diagnostics.push(historical_waiver_diagnostic(
                BudgetDiagnosticCodeV1::WaiverInvalid,
                historical,
                format!(
                    "removed historical waiver target is unsupported ({})",
                    unsupported_name(kind)
                ),
            ))
        }
    }
}

fn historical_waiver_diagnostic(
    code: BudgetDiagnosticCodeV1,
    waiver: &WaiverV1,
    message: impl Into<String>,
) -> BudgetDiagnosticV1 {
    BudgetDiagnosticV1::new(BudgetSeverityV1::Error, code, message)
        .at_path(waiver.path.clone())
        .for_rule(waiver.rule.clone())
        .for_waiver(waiver.id.clone())
}
