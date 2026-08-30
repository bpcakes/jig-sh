use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetSeverityV1 {
    Error,
    Warning,
    Notice,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetMetricV1 {
    Lines,
    Bytes,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum BudgetDiagnosticCodeV1 {
    #[serde(rename = "file_budget.max_lines")]
    MaxLines,
    #[serde(rename = "file_budget.max_bytes")]
    MaxBytes,
    #[serde(rename = "file_budget.debt_growth_lines")]
    DebtGrowthLines,
    #[serde(rename = "file_budget.debt_growth_bytes")]
    DebtGrowthBytes,
    #[serde(rename = "file_budget.legacy_debt")]
    LegacyDebt,
    #[serde(rename = "file_budget.debt_improved")]
    DebtImproved,
    #[serde(rename = "file_budget.notice_lines")]
    NoticeLines,
    #[serde(rename = "file_budget.notice_bytes")]
    NoticeBytes,
    #[serde(rename = "file_budget.warning_lines")]
    WarningLines,
    #[serde(rename = "file_budget.warning_bytes")]
    WarningBytes,
    #[serde(rename = "file_budget.waiver_active")]
    WaiverActive,
    #[serde(rename = "file_budget.waiver_expired")]
    WaiverExpired,
    #[serde(rename = "file_budget.waiver_invalid")]
    WaiverInvalid,
    #[serde(rename = "file_budget.waiver_removed_with_debt")]
    WaiverRemovedWithDebt,
    #[serde(rename = "file_budget.policy_changed")]
    PolicyChanged,
    #[serde(rename = "file_budget.policy_invalid")]
    PolicyInvalid,
    #[serde(rename = "file_budget.rule_ambiguous")]
    RuleAmbiguous,
    #[serde(rename = "file_budget.scope_incomplete")]
    ScopeIncomplete,
    #[serde(rename = "file_budget.baseline_unavailable")]
    BaselineUnavailable,
    #[serde(rename = "file_budget.unsupported_file")]
    UnsupportedFile,
    #[serde(rename = "file_budget.changed_during_read")]
    ChangedDuringRead,
    #[serde(rename = "file_budget.resource_limit")]
    ResourceLimit,
}

impl BudgetDiagnosticCodeV1 {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MaxLines => "file_budget.max_lines",
            Self::MaxBytes => "file_budget.max_bytes",
            Self::DebtGrowthLines => "file_budget.debt_growth_lines",
            Self::DebtGrowthBytes => "file_budget.debt_growth_bytes",
            Self::LegacyDebt => "file_budget.legacy_debt",
            Self::DebtImproved => "file_budget.debt_improved",
            Self::NoticeLines => "file_budget.notice_lines",
            Self::NoticeBytes => "file_budget.notice_bytes",
            Self::WarningLines => "file_budget.warning_lines",
            Self::WarningBytes => "file_budget.warning_bytes",
            Self::WaiverActive => "file_budget.waiver_active",
            Self::WaiverExpired => "file_budget.waiver_expired",
            Self::WaiverInvalid => "file_budget.waiver_invalid",
            Self::WaiverRemovedWithDebt => "file_budget.waiver_removed_with_debt",
            Self::PolicyChanged => "file_budget.policy_changed",
            Self::PolicyInvalid => "file_budget.policy_invalid",
            Self::RuleAmbiguous => "file_budget.rule_ambiguous",
            Self::ScopeIncomplete => "file_budget.scope_incomplete",
            Self::BaselineUnavailable => "file_budget.baseline_unavailable",
            Self::UnsupportedFile => "file_budget.unsupported_file",
            Self::ChangedDuringRead => "file_budget.changed_during_read",
            Self::ResourceLimit => "file_budget.resource_limit",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetDiagnosticV1 {
    pub severity: BudgetSeverityV1,
    pub code: BudgetDiagnosticCodeV1,
    pub message: String,
    pub path: Option<String>,
    pub rule_id: Option<String>,
    pub waiver_id: Option<String>,
    pub metric: Option<BudgetMetricV1>,
    pub current: Option<u64>,
    pub comparison: Option<u64>,
    pub limit: Option<u64>,
    pub debt: Option<u64>,
    pub debt_growth: Option<u64>,
}

impl BudgetDiagnosticV1 {
    pub(crate) fn new(
        severity: BudgetSeverityV1,
        code: BudgetDiagnosticCodeV1,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            code,
            message: message.into(),
            path: None,
            rule_id: None,
            waiver_id: None,
            metric: None,
            current: None,
            comparison: None,
            limit: None,
            debt: None,
            debt_growth: None,
        }
    }

    pub(crate) fn at_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub(crate) fn for_rule(mut self, rule_id: impl Into<String>) -> Self {
        self.rule_id = Some(rule_id.into());
        self
    }

    pub(crate) fn for_waiver(mut self, waiver_id: impl Into<String>) -> Self {
        self.waiver_id = Some(waiver_id.into());
        self
    }
}

pub(crate) fn sort_diagnostics(diagnostics: &mut [BudgetDiagnosticV1]) {
    diagnostics.sort_by(compare_diagnostic);
}

fn compare_diagnostic(left: &BudgetDiagnosticV1, right: &BudgetDiagnosticV1) -> Ordering {
    left.severity
        .cmp(&right.severity)
        .then_with(|| left.path.cmp(&right.path))
        .then_with(|| left.code.cmp(&right.code))
        .then_with(|| left.rule_id.cmp(&right.rule_id))
        .then_with(|| left.waiver_id.cmp(&right.waiver_id))
        .then_with(|| left.metric.cmp(&right.metric))
        .then_with(|| left.message.cmp(&right.message))
}
