//! Pure, repository-agnostic file-budget policy and evaluation.

mod diagnostic;
mod evaluation;
mod measurement;
mod policy;

pub use diagnostic::{
    BudgetDiagnosticCodeV1, BudgetDiagnosticV1, BudgetMetricV1, BudgetSeverityV1,
};
pub use evaluation::{
    BudgetEvaluationV1, ComparisonPolicyV1, CurrentFileStateV1, EvaluateFileV1, EvaluationInputV1,
    ExactCurrentPathFactV1, ExactCurrentPathStateV1, MAX_WAIVER_TARGET_FACTS_V1,
    UnsupportedFileKindV1, evaluate_v1,
};
pub use measurement::{
    MeasurementBudgetV1, MeasurementErrorKindV1, MeasurementErrorV1, MeasurementV1,
    measure_stream_v1,
};
pub use policy::{
    ExclusionKindV1, ExclusionV1, InvalidPolicyV1, MAX_CANDIDATE_PATH_BYTES_V1,
    MAX_CATEGORY_BYTES_V1, MAX_PATTERN_BYTES_V1, MAX_PATTERNS_V1, MAX_POLICY_BYTES_V1,
    MAX_RULES_V1, MAX_WAIVERS_V1, PathDispositionV1, PolicyDateV1, PolicyIdentityV1, PolicyV1,
    RuleV1, WaiverV1, parse_comparison_policy_v1, parse_policy_v1,
};
