use std::collections::BTreeSet;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::diagnostic::{BudgetDiagnosticCodeV1, BudgetDiagnosticV1, BudgetSeverityV1};

use super::{
    CONTRACT_PATH, ExclusionV1, MAX_CATEGORY_BYTES_V1, MAX_IDENTIFIER_BYTES_V1,
    MAX_PATTERN_BYTES_V1, POLICY_PATH, PolicyDateV1, RuleV1, WaiverDtoV1, WaiverV1,
    policy_diagnostic, waiver_diagnostic,
};

pub(super) fn validate_rule(rule: &RuleV1, diagnostics: &mut Vec<BudgetDiagnosticV1>) {
    if !is_identifier(&rule.id) {
        diagnostics.push(policy_diagnostic(format!(
            "rule id `{}` must be 1 to {MAX_IDENTIFIER_BYTES_V1} bytes of lowercase ASCII letters or digits with `-` and `_` only internally",
            rule.id
        )));
    }
    if let Some(category) = &rule.category
        && (category.is_empty()
            || category.trim() != category
            || category.len() > MAX_CATEGORY_BYTES_V1
            || category.chars().any(char::is_control))
    {
        diagnostics.push(policy_diagnostic(format!(
            "rule `{}` category must be 1 to {MAX_CATEGORY_BYTES_V1} UTF-8 bytes with no surrounding whitespace or control characters",
            rule.id
        )));
    }
    if rule.include.is_empty() {
        diagnostics.push(policy_diagnostic(format!(
            "rule `{}` must contain at least one include pattern",
            rule.id
        )));
    }
    if rule.max_lines.is_none() && rule.max_bytes.is_none() {
        diagnostics.push(policy_diagnostic(format!(
            "rule `{}` must enable max_lines, max_bytes, or both",
            rule.id
        )));
    }
    validate_thresholds(
        &rule.id,
        "lines",
        rule.notice_lines,
        rule.warn_lines,
        rule.max_lines,
        diagnostics,
    );
    validate_thresholds(
        &rule.id,
        "bytes",
        rule.notice_bytes,
        rule.warn_bytes,
        rule.max_bytes,
        diagnostics,
    );
    for (field, patterns) in [("include", &rule.include), ("exclude", &rule.exclude)] {
        for pattern in patterns {
            validate_pattern(pattern, field == "exclude", diagnostics, &rule.id, field);
        }
    }
}

fn validate_thresholds(
    rule_id: &str,
    metric: &str,
    notice: Option<u64>,
    warning: Option<u64>,
    maximum: Option<u64>,
    diagnostics: &mut Vec<BudgetDiagnosticV1>,
) {
    if [notice, warning, maximum]
        .into_iter()
        .flatten()
        .any(|value| value == 0)
    {
        diagnostics.push(policy_diagnostic(format!(
            "rule `{rule_id}` {metric} thresholds must be positive integers"
        )));
    }
    if maximum.is_none() && (notice.is_some() || warning.is_some()) {
        diagnostics.push(policy_diagnostic(format!(
            "rule `{rule_id}` cannot configure {metric} notice or warning thresholds without a maximum"
        )));
        return;
    }
    if notice
        .zip(warning)
        .is_some_and(|(left, right)| left > right)
        || notice
            .zip(maximum)
            .is_some_and(|(left, right)| left > right)
        || warning
            .zip(maximum)
            .is_some_and(|(left, right)| left > right)
    {
        diagnostics.push(policy_diagnostic(format!(
            "rule `{rule_id}` {metric} thresholds must satisfy notice <= warning <= maximum for every enabled threshold"
        )));
    }
}

pub(super) fn validate_exclusion(
    exclusion: &ExclusionV1,
    diagnostics: &mut Vec<BudgetDiagnosticV1>,
) {
    if exclusion.reason.trim().is_empty() {
        diagnostics.push(policy_diagnostic(format!(
            "exclusion `{}` must have a non-empty reason",
            exclusion.pattern
        )));
    }
    validate_pattern(
        &exclusion.pattern,
        true,
        diagnostics,
        "top-level",
        "exclusion",
    );
}

fn validate_pattern(
    pattern: &str,
    exclusion: bool,
    diagnostics: &mut Vec<BudgetDiagnosticV1>,
    owner: &str,
    field: &str,
) {
    if let Err(message) = validate_pattern_shape(pattern) {
        diagnostics.push(policy_diagnostic(format!(
            "{owner} {field} pattern `{pattern}` is invalid: {message}"
        )));
        return;
    }
    match compile_pattern(pattern) {
        Ok(matcher) => {
            if exclusion && (matcher.is_match(POLICY_PATH) || matcher.is_match(CONTRACT_PATH)) {
                diagnostics.push(policy_diagnostic(format!(
                    "{owner} {field} pattern `{pattern}` may not exclude `{POLICY_PATH}` or `{CONTRACT_PATH}`"
                )));
            }
        }
        Err(error) => diagnostics.push(policy_diagnostic(format!(
            "{owner} {field} pattern `{pattern}` is invalid: {error}"
        ))),
    }
}

fn validate_pattern_shape(pattern: &str) -> Result<(), &'static str> {
    if pattern.is_empty() {
        return Err("patterns must be non-empty");
    }
    if pattern.len() > MAX_PATTERN_BYTES_V1 {
        return Err("pattern exceeds the version 1 UTF-8 byte limit");
    }
    if pattern.contains('\0') {
        return Err("patterns may not contain NUL bytes");
    }
    if pattern.starts_with('/') || pattern.split('/').any(has_windows_drive_prefix) {
        return Err("patterns must be repository-relative");
    }
    if pattern
        .split('/')
        .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err("patterns may not contain empty, dot, or parent-traversal components");
    }
    let first = pattern.split('/').next().unwrap_or_default();
    if explicitly_targets_protected_root(first) {
        return Err("patterns may not explicitly target .agent or .git authority");
    }
    Ok(())
}

fn explicitly_targets_protected_root(first: &str) -> bool {
    if matches!(first, "*" | "**") {
        return false;
    }
    build_glob(first).is_ok_and(|glob| {
        let matcher = glob.compile_matcher();
        matcher.is_match(".agent") || matcher.is_match(".git")
    })
}

pub(super) fn validate_waiver(
    waiver: &WaiverV1,
    rules: &[RuleV1],
    rule_ids: &BTreeSet<&str>,
    current_date: Option<PolicyDateV1>,
    diagnostics: &mut Vec<BudgetDiagnosticV1>,
) {
    if !is_identifier(&waiver.id) {
        diagnostics.push(waiver_diagnostic(
            waiver,
            format!(
                "waiver id `{}` must be 1 to {MAX_IDENTIFIER_BYTES_V1} bytes of lowercase ASCII letters or digits with `-` and `_` only internally",
                waiver.id
            ),
        ));
    }
    if !rule_ids.contains(waiver.rule.as_str()) {
        diagnostics.push(waiver_diagnostic(
            waiver,
            format!("waiver names unknown rule `{}`", waiver.rule),
        ));
    }
    if let Some(rule) = rules.iter().find(|rule| rule.id == waiver.rule) {
        if waiver.ceiling_lines.is_some() && rule.max_lines.is_none() {
            diagnostics.push(waiver_diagnostic(
                waiver,
                "waiver cannot set ceiling_lines when its rule has no max_lines",
            ));
        }
        if waiver.ceiling_bytes.is_some() && rule.max_bytes.is_none() {
            diagnostics.push(waiver_diagnostic(
                waiver,
                "waiver cannot set ceiling_bytes when its rule has no max_bytes",
            ));
        }
    }
    if let Err(message) = validate_exact_path(&waiver.path) {
        diagnostics.push(waiver_diagnostic(waiver, message));
    }
    if waiver.ceiling_lines.is_none() && waiver.ceiling_bytes.is_none() {
        diagnostics.push(waiver_diagnostic(
            waiver,
            "waiver must set ceiling_lines, ceiling_bytes, or both",
        ));
    }
    if waiver
        .ceiling_lines
        .into_iter()
        .chain(waiver.ceiling_bytes)
        .any(|value| value == 0)
    {
        diagnostics.push(waiver_diagnostic(
            waiver,
            "waiver ceilings must be positive integers",
        ));
    }
    if waiver.reason.trim().is_empty() {
        diagnostics.push(waiver_diagnostic(
            waiver,
            "waiver must have a non-empty reason",
        ));
    }
    if current_date.is_some_and(|date| waiver.expires < date) {
        diagnostics.push(
            BudgetDiagnosticV1::new(
                BudgetSeverityV1::Error,
                BudgetDiagnosticCodeV1::WaiverExpired,
                format!(
                    "waiver `{}` expired after {} and is not active on {}",
                    waiver.id,
                    waiver.expires,
                    current_date.expect("date was checked")
                ),
            )
            .at_path(waiver.path.clone())
            .for_rule(waiver.rule.clone())
            .for_waiver(waiver.id.clone()),
        );
    }
}

pub(super) fn convert_waiver(dto: WaiverDtoV1) -> Result<WaiverV1, Box<BudgetDiagnosticV1>> {
    let Some(date) = dto.expires.date else {
        return Err(Box::new(
            BudgetDiagnosticV1::new(
                BudgetSeverityV1::Error,
                BudgetDiagnosticCodeV1::WaiverInvalid,
                format!("waiver `{}` expiry must be an ISO calendar date", dto.id),
            )
            .at_path(dto.path)
            .for_rule(dto.rule)
            .for_waiver(dto.id),
        ));
    };
    if dto.expires.time.is_some() || dto.expires.offset.is_some() {
        return Err(Box::new(
            BudgetDiagnosticV1::new(
                BudgetSeverityV1::Error,
                BudgetDiagnosticCodeV1::WaiverInvalid,
                format!(
                    "waiver `{}` expiry must be a date without time or offset",
                    dto.id
                ),
            )
            .at_path(dto.path)
            .for_rule(dto.rule)
            .for_waiver(dto.id),
        ));
    }
    let expires = PolicyDateV1::new(date.year, date.month, date.day).map_err(|message| {
        Box::new(
            BudgetDiagnosticV1::new(
                BudgetSeverityV1::Error,
                BudgetDiagnosticCodeV1::WaiverInvalid,
                format!("waiver `{}` has {message}", dto.id),
            )
            .at_path(dto.path.clone())
            .for_rule(dto.rule.clone())
            .for_waiver(dto.id.clone()),
        )
    })?;
    Ok(WaiverV1 {
        id: dto.id,
        rule: dto.rule,
        path: dto.path,
        ceiling_lines: dto.ceiling_lines,
        ceiling_bytes: dto.ceiling_bytes,
        reason: dto.reason,
        expires,
    })
}

fn validate_exact_path(path: &str) -> Result<(), String> {
    validate_candidate_path(path)?;
    if path.contains(['*', '?', '[', ']', '{', '}']) {
        return Err("waiver path must not contain glob syntax".to_owned());
    }
    Ok(())
}

pub(super) fn validate_candidate_path(path: &str) -> Result<(), String> {
    validate_candidate_path_shape(path)?;
    if matches!(path.split('/').next(), Some(".agent" | ".git")) {
        return Err("path targets protected repository authority".to_owned());
    }
    Ok(())
}

pub(super) fn validate_candidate_path_shape(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("path must be non-empty".to_owned());
    }
    if path.len() > super::MAX_CANDIDATE_PATH_BYTES_V1 {
        return Err(format!(
            "path exceeds the version 1 {}-byte limit",
            super::MAX_CANDIDATE_PATH_BYTES_V1
        ));
    }
    if path.contains('\0') {
        return Err("path must not contain NUL bytes".to_owned());
    }
    if path.starts_with('/') || path.split('/').any(has_windows_drive_prefix) {
        return Err("path must be repository-relative".to_owned());
    }
    if path
        .split('/')
        .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err("path contains an empty, dot, or traversal component".to_owned());
    }
    Ok(())
}

fn is_identifier(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_IDENTIFIER_BYTES_V1 {
        return false;
    }
    let bytes = value.as_bytes();
    let is_alphanumeric = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    is_alphanumeric(bytes[0])
        && is_alphanumeric(bytes[bytes.len() - 1])
        && bytes
            .iter()
            .all(|byte| is_alphanumeric(*byte) || matches!(*byte, b'-' | b'_'))
}

pub(super) fn is_outside_candidate_universe(path: &str) -> bool {
    matches!(path.split('/').next(), Some(".agent" | ".git"))
}

fn has_windows_drive_prefix(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

pub(super) fn compile_patterns(patterns: &[String]) -> Result<GlobSet, globset::Error> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(build_glob(pattern)?);
    }
    builder.build()
}

fn build_glob(pattern: &str) -> Result<globset::Glob, globset::Error> {
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(false)
        .build()
}

fn compile_pattern(pattern: &str) -> Result<globset::GlobMatcher, globset::Error> {
    Ok(build_glob(pattern)?.compile_matcher())
}
