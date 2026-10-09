//! The doctor check record and its constructor.

use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize)]
pub(super) struct DoctorCheck {
    pub(super) id: String,
    pub(super) label: String,
    pub(super) required: bool,
    /// Optional setup that only the operator can perform, such as choosing a
    /// secret. Never promoted to `next_step`, `next_issue`, or
    /// `optional_setup`; reported through top-level `operator_setup` instead.
    pub(super) operator_only: bool,
    pub(super) ok: bool,
    pub(super) status: String,
    pub(super) detail: String,
    pub(super) fix: Option<String>,
    pub(super) data: Value,
}

pub(super) fn check(
    id: &str,
    label: &str,
    required: bool,
    ok: bool,
    status: &str,
    detail: impl Into<String>,
) -> DoctorCheck {
    DoctorCheck {
        id: id.to_string(),
        label: label.to_string(),
        required,
        operator_only: false,
        ok,
        status: status.to_string(),
        detail: detail.into(),
        fix: None,
        data: json!({}),
    }
}

impl DoctorCheck {
    pub(super) fn with_fix(mut self, fix: &str) -> Self {
        self.fix = Some(fix.to_string());
        self
    }

    pub(super) fn with_optional_fix(mut self, fix: Option<&str>) -> Self {
        self.fix = fix.map(str::to_string);
        self
    }

    pub(super) fn operator_only(mut self) -> Self {
        debug_assert!(
            !self.required,
            "operator-only doctor checks must be optional"
        );
        self.operator_only = true;
        self
    }

    pub(super) fn with_data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }
}
