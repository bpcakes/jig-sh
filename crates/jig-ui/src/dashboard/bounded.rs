use std::{error::Error, fmt};

use serde::{Deserialize, Deserializer, Serialize};

pub const DEFAULT_TIMELINE_ROWS: usize = 120;
pub const MAX_TIMELINE_ROWS: usize = 1_000;
pub const ROOT_LIMIT_KEYS: &[&str] = &[
    "open_plans",
    "history",
    "failures",
    "tool_stats",
    "timeline",
    "plan_decisions",
    "plan_receipts",
];

/// A bounded row collection with explicit information about omitted input.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BoundedRows<T> {
    items: Vec<T>,
    applied: usize,
    omitted: Option<usize>,
}

impl<T> BoundedRows<T> {
    pub fn from_total(
        items: Vec<T>,
        applied: usize,
        total_input: Option<usize>,
    ) -> Result<Self, BoundViolation> {
        let omitted = total_input
            .map(|total| omitted_count(total, items.len(), applied, BoundUnit::Rows))
            .transpose()?;
        Self::validated(items, applied, omitted)
    }

    pub fn for_limit(
        items: Vec<T>,
        total_input: Option<usize>,
        limit_id: LimitId,
    ) -> Result<Self, LimitError> {
        let spec = limit_spec(limit_id, LimitShape::NestedRows)?;
        Self::from_total(items, spec.ceiling, total_input).map_err(LimitError::Bound)
    }

    fn validated(
        items: Vec<T>,
        applied: usize,
        omitted: Option<usize>,
    ) -> Result<Self, BoundViolation> {
        validate_bound(items.len(), applied, omitted, BoundUnit::Rows)?;
        Ok(Self {
            items,
            applied,
            omitted,
        })
    }

    #[must_use]
    pub fn items(&self) -> &[T] {
        &self.items
    }

    #[must_use]
    pub const fn applied(&self) -> usize {
        self.applied
    }

    #[must_use]
    pub const fn omitted(&self) -> Option<usize> {
        self.omitted
    }

    pub(crate) fn validate_for_limit(&self, id: LimitId) -> Result<(), LimitError> {
        let spec = limit_spec(id, LimitShape::NestedRows)?;
        if self.applied != spec.ceiling {
            return Err(LimitError::AppliedMismatch {
                id,
                expected: spec.ceiling,
                actual: self.applied,
            });
        }
        validate_bound(
            self.items.len(),
            self.applied,
            self.omitted,
            BoundUnit::Rows,
        )
        .map_err(LimitError::Bound)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for BoundedRows<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire<T> {
            items: Vec<T>,
            applied: usize,
            omitted: Option<usize>,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::validated(wire.items, wire.applied, wire.omitted).map_err(serde::de::Error::custom)
    }
}

/// Bounded text measured in Unicode scalar values rather than bytes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BoundedText {
    text: String,
    applied_chars: usize,
    omitted_chars: Option<usize>,
}

impl BoundedText {
    pub fn from_total(
        text: impl Into<String>,
        applied_chars: usize,
        total_input_chars: Option<usize>,
    ) -> Result<Self, BoundViolation> {
        let text = text.into();
        let retained = text.chars().count();
        let omitted_chars = total_input_chars
            .map(|total| omitted_count(total, retained, applied_chars, BoundUnit::Characters))
            .transpose()?;
        validate_bound(
            retained,
            applied_chars,
            omitted_chars,
            BoundUnit::Characters,
        )?;
        Ok(Self {
            text,
            applied_chars,
            omitted_chars,
        })
    }

    pub fn for_limit(
        text: impl Into<String>,
        total_input_chars: Option<usize>,
        limit_id: LimitId,
    ) -> Result<Self, LimitError> {
        let spec = limit_spec(limit_id, LimitShape::NestedText)?;
        Self::from_total(text, spec.ceiling, total_input_chars).map_err(LimitError::Bound)
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub const fn applied_chars(&self) -> usize {
        self.applied_chars
    }

    #[must_use]
    pub const fn omitted_chars(&self) -> Option<usize> {
        self.omitted_chars
    }

    pub(crate) fn validate_for_limit(&self, id: LimitId) -> Result<(), LimitError> {
        let spec = limit_spec(id, LimitShape::NestedText)?;
        if self.applied_chars != spec.ceiling {
            return Err(LimitError::AppliedMismatch {
                id,
                expected: spec.ceiling,
                actual: self.applied_chars,
            });
        }
        validate_bound(
            self.text.chars().count(),
            self.applied_chars,
            self.omitted_chars,
            BoundUnit::Characters,
        )
        .map_err(LimitError::Bound)
    }
}

impl<'de> Deserialize<'de> for BoundedText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            text: String,
            applied_chars: usize,
            omitted_chars: Option<usize>,
        }

        let wire = Wire::deserialize(deserializer)?;
        let retained = wire.text.chars().count();
        validate_bound(
            retained,
            wire.applied_chars,
            wire.omitted_chars,
            BoundUnit::Characters,
        )
        .map_err(serde::de::Error::custom)?;
        Ok(Self {
            text: wire.text,
            applied_chars: wire.applied_chars,
            omitted_chars: wire.omitted_chars,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoundUnit {
    Rows,
    Characters,
    Bytes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundViolation {
    pub retained: usize,
    pub applied: usize,
    pub unit: BoundUnit,
    pub provided_total: Option<usize>,
}

fn omitted_count(
    total: usize,
    retained: usize,
    applied: usize,
    unit: BoundUnit,
) -> Result<usize, BoundViolation> {
    if retained > total {
        return Err(BoundViolation {
            retained,
            applied,
            unit,
            provided_total: Some(total),
        });
    }
    let omitted = total - retained;
    validate_bound(retained, applied, Some(omitted), unit)?;
    Ok(omitted)
}

fn validate_bound(
    retained: usize,
    applied: usize,
    omitted: Option<usize>,
    unit: BoundUnit,
) -> Result<(), BoundViolation> {
    if retained > applied || omitted.is_some_and(|omitted| omitted > 0 && retained < applied) {
        return Err(BoundViolation {
            retained,
            applied,
            unit,
            provided_total: None,
        });
    }
    Ok(())
}

impl fmt::Display for BoundViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(total) = self.provided_total {
            return write!(
                formatter,
                "retained {} {:?} exceeds declared input total {total}",
                self.retained, self.unit
            );
        }
        write!(
            formatter,
            "retained {} {:?} exceeds applied limit {}",
            self.retained, self.unit, self.applied
        )
    }
}

impl Error for BoundViolation {}

/// Root-level applied limit metadata in schema-1 recorder documents.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppliedLimit {
    pub applied: usize,
    pub omitted: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecorderLimits {
    pub open_plans: AppliedLimit,
    pub history: AppliedLimit,
    pub failures: AppliedLimit,
    pub tool_stats: AppliedLimit,
    pub timeline: AppliedLimit,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlanLimits {
    pub plan_decisions: AppliedLimit,
    pub plan_receipts: AppliedLimit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitShape {
    RootRows,
    NestedRows,
    NestedText,
    InputBytes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LimitSpec {
    pub id: LimitId,
    pub ceiling: usize,
    pub shape: LimitShape,
    pub serialized_at_root: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum LimitId {
    OpenPlans,
    History,
    Failures,
    FailureStderrChars,
    ToolStats,
    LoopWorkflows,
    LoopLeases,
    LoopAttempts,
    LoopScheduledOccurrences,
    LoopWaitingAttempts,
    LoopExhaustedAttempts,
    Timeline,
    TimelineDecisionRationaleChars,
    GateRows,
    GateChangedPaths,
    GateMatchingPaths,
    GateFindings,
    PlanBodyChars,
    PlanBodyInputBytes,
    PlanDecisions,
    PlanReceipts,
    ReceiptChangedPaths,
    ReceiptStdoutChars,
    ReceiptStderrChars,
}

impl LimitId {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenPlans => "open_plans",
            Self::History => "history",
            Self::Failures => "failures",
            Self::FailureStderrChars => "failure_stderr_chars",
            Self::ToolStats => "tool_stats",
            Self::LoopWorkflows => "loop_workflows",
            Self::LoopLeases => "loop_leases",
            Self::LoopAttempts => "loop_attempts",
            Self::LoopScheduledOccurrences => "loop_scheduled_occurrences",
            Self::LoopWaitingAttempts => "loop_waiting_attempts",
            Self::LoopExhaustedAttempts => "loop_exhausted_attempts",
            Self::Timeline => "timeline",
            Self::TimelineDecisionRationaleChars => "timeline_decision_rationale_chars",
            Self::GateRows => "gate_rows",
            Self::GateChangedPaths => "gate_changed_paths",
            Self::GateMatchingPaths => "gate_matching_paths",
            Self::GateFindings => "gate_findings",
            Self::PlanBodyChars => "plan_body_chars",
            Self::PlanBodyInputBytes => "plan_body_input_bytes",
            Self::PlanDecisions => "plan_decisions",
            Self::PlanReceipts => "plan_receipts",
            Self::ReceiptChangedPaths => "receipt_changed_paths",
            Self::ReceiptStdoutChars => "receipt_stdout_chars",
            Self::ReceiptStderrChars => "receipt_stderr_chars",
        }
    }

    #[must_use]
    pub fn ceiling(self) -> usize {
        LIMIT_SPECS
            .iter()
            .find(|spec| spec.id == self)
            .expect("every LimitId has one LIMIT_SPECS entry")
            .ceiling
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitError {
    WrongShape {
        id: LimitId,
        expected: LimitShape,
        actual: LimitShape,
    },
    AppliedMismatch {
        id: LimitId,
        expected: usize,
        actual: usize,
    },
    Bound(BoundViolation),
}

impl fmt::Display for LimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongShape {
                id,
                expected,
                actual,
            } => write!(
                formatter,
                "limit '{}' has shape {actual:?}, expected {expected:?}",
                id.as_str()
            ),
            Self::AppliedMismatch {
                id,
                expected,
                actual,
            } => write!(
                formatter,
                "limit '{}' uses applied ceiling {actual}, expected {expected}",
                id.as_str()
            ),
            Self::Bound(error) => error.fmt(formatter),
        }
    }
}

impl Error for LimitError {}

fn limit_spec(id: LimitId, shape: LimitShape) -> Result<&'static LimitSpec, LimitError> {
    let spec = LIMIT_SPECS
        .iter()
        .find(|spec| spec.id == id)
        .expect("every LimitId has one LIMIT_SPECS entry");
    if spec.shape == shape {
        Ok(spec)
    } else {
        Err(LimitError::WrongShape {
            id,
            expected: shape,
            actual: spec.shape,
        })
    }
}

pub fn validate_input_bytes(byte_len: usize, id: LimitId) -> Result<(), LimitError> {
    let spec = limit_spec(id, LimitShape::InputBytes)?;
    if byte_len > spec.ceiling {
        Err(LimitError::Bound(BoundViolation {
            retained: byte_len,
            applied: spec.ceiling,
            unit: BoundUnit::Bytes,
            provided_total: None,
        }))
    } else {
        Ok(())
    }
}

pub fn root_limit(id: LimitId, omitted: Option<usize>) -> Result<AppliedLimit, LimitError> {
    let spec = limit_spec(id, LimitShape::RootRows)?;
    Ok(AppliedLimit {
        applied: spec.ceiling,
        omitted,
    })
}

pub const LIMIT_SPECS: &[LimitSpec] = &[
    LimitSpec {
        id: LimitId::OpenPlans,
        ceiling: 1_000,
        shape: LimitShape::RootRows,
        serialized_at_root: true,
    },
    LimitSpec {
        id: LimitId::History,
        ceiling: 10,
        shape: LimitShape::RootRows,
        serialized_at_root: true,
    },
    LimitSpec {
        id: LimitId::Failures,
        ceiling: 10,
        shape: LimitShape::RootRows,
        serialized_at_root: true,
    },
    LimitSpec {
        id: LimitId::FailureStderrChars,
        ceiling: 400,
        shape: LimitShape::NestedText,
        serialized_at_root: false,
    },
    LimitSpec {
        id: LimitId::ToolStats,
        ceiling: 256,
        shape: LimitShape::RootRows,
        serialized_at_root: true,
    },
    LimitSpec {
        id: LimitId::LoopWorkflows,
        ceiling: 1_000,
        shape: LimitShape::NestedRows,
        serialized_at_root: false,
    },
    LimitSpec {
        id: LimitId::LoopLeases,
        ceiling: 1_000,
        shape: LimitShape::NestedRows,
        serialized_at_root: false,
    },
    LimitSpec {
        id: LimitId::LoopAttempts,
        ceiling: 1_000,
        shape: LimitShape::NestedRows,
        serialized_at_root: false,
    },
    LimitSpec {
        id: LimitId::LoopScheduledOccurrences,
        ceiling: 1_000,
        shape: LimitShape::NestedRows,
        serialized_at_root: false,
    },
    LimitSpec {
        id: LimitId::LoopWaitingAttempts,
        ceiling: 1_000,
        shape: LimitShape::NestedRows,
        serialized_at_root: false,
    },
    LimitSpec {
        id: LimitId::LoopExhaustedAttempts,
        ceiling: 1_000,
        shape: LimitShape::NestedRows,
        serialized_at_root: false,
    },
    LimitSpec {
        id: LimitId::Timeline,
        ceiling: MAX_TIMELINE_ROWS,
        shape: LimitShape::RootRows,
        serialized_at_root: true,
    },
    LimitSpec {
        id: LimitId::TimelineDecisionRationaleChars,
        ceiling: 300,
        shape: LimitShape::NestedText,
        serialized_at_root: false,
    },
    LimitSpec {
        id: LimitId::GateRows,
        ceiling: 256,
        shape: LimitShape::NestedRows,
        serialized_at_root: false,
    },
    LimitSpec {
        id: LimitId::GateChangedPaths,
        ceiling: 100,
        shape: LimitShape::NestedRows,
        serialized_at_root: false,
    },
    LimitSpec {
        id: LimitId::GateMatchingPaths,
        ceiling: 100,
        shape: LimitShape::NestedRows,
        serialized_at_root: false,
    },
    LimitSpec {
        id: LimitId::GateFindings,
        ceiling: 100,
        shape: LimitShape::NestedRows,
        serialized_at_root: false,
    },
    LimitSpec {
        id: LimitId::PlanBodyChars,
        ceiling: 20_000,
        shape: LimitShape::NestedText,
        serialized_at_root: false,
    },
    LimitSpec {
        id: LimitId::PlanBodyInputBytes,
        ceiling: 80_004,
        shape: LimitShape::InputBytes,
        serialized_at_root: false,
    },
    LimitSpec {
        id: LimitId::PlanDecisions,
        ceiling: 100,
        shape: LimitShape::RootRows,
        serialized_at_root: true,
    },
    LimitSpec {
        id: LimitId::PlanReceipts,
        ceiling: 50,
        shape: LimitShape::RootRows,
        serialized_at_root: true,
    },
    LimitSpec {
        id: LimitId::ReceiptChangedPaths,
        ceiling: 20,
        shape: LimitShape::NestedRows,
        serialized_at_root: false,
    },
    LimitSpec {
        id: LimitId::ReceiptStdoutChars,
        ceiling: 1_000,
        shape: LimitShape::NestedText,
        serialized_at_root: false,
    },
    LimitSpec {
        id: LimitId::ReceiptStderrChars,
        ceiling: 1_000,
        shape: LimitShape::NestedText,
        serialized_at_root: false,
    },
];
