#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParityArea {
    Shared,
    Recorder,
    Status,
    Removal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParityEntry {
    pub key: &'static str,
    pub capability: &'static str,
    pub area: ParityArea,
    /// Repository-relative source that declares `behavioral_test`.
    pub test_source: &'static str,
    /// Exact Rust test function that exercises this capability.
    pub behavioral_test: &'static str,
}

macro_rules! parity {
    ($key:literal, $capability:literal, $area:ident, $source:expr, $test:literal) => {
        ParityEntry {
            key: $key,
            capability: $capability,
            area: ParityArea::$area,
            test_source: $source,
            behavioral_test: $test,
        }
    };
}

const DASHBOARD_LOCAL_TESTS: &str = "crates/jig-ui/src/terminal/tests/local.rs";
const DASHBOARD_LOCAL_PARITY_TESTS: &str = "crates/jig-ui/src/terminal/tests/local/parity.rs";
const DASHBOARD_MODEL_TESTS: &str = "crates/jig-ui/src/terminal/tests.rs";
const DASHBOARD_EVENT_LOOP_TESTS: &str = "crates/jig-ui/src/terminal/runtime/event_loop.rs";
const DASHBOARD_SCHEDULER_TESTS: &str = "crates/jig-ui/src/terminal/runtime/scheduler.rs";
const CLI_CUTOVER_TESTS: &str = "crates/jig/tests/ui_cutover.rs";
const CLI_ARCHITECTURE_TESTS: &str = "crates/jig/tests/ui_architecture.rs";

/// One entry for every row in project-plan section 5.6.
///
/// Every entry points to a real test in `test_source`. The owning dashboard
/// contract suite resolves those references before release validation.
pub const PARITY_REGISTRY: &[ParityEntry] = &[
    parity!(
        "repository_identity",
        "Repository, harness, and default-branch identity",
        Shared,
        DASHBOARD_LOCAL_PARITY_TESTS,
        "repository_open_plans_failures_and_tool_times_keep_exact_semantics"
    ),
    parity!(
        "current_revision",
        "Current branch or detached revision",
        Shared,
        DASHBOARD_LOCAL_TESTS,
        "local_header_reports_default_branch_age_and_detached_state_on_every_local_tab"
    ),
    parity!(
        "state_counts",
        "Session, plan, and decision counts",
        Recorder,
        DASHBOARD_LOCAL_PARITY_TESTS,
        "work_timeline_and_health_render_typed_parity_fields"
    ),
    parity!(
        "open_plans",
        "Open plans",
        Recorder,
        DASHBOARD_LOCAL_PARITY_TESTS,
        "repository_open_plans_failures_and_tool_times_keep_exact_semantics"
    ),
    parity!(
        "gate_table",
        "Gate table",
        Recorder,
        DASHBOARD_LOCAL_PARITY_TESTS,
        "plan_detail_preserves_sections_errors_and_inert_argv"
    ),
    parity!(
        "gate_remediation",
        "Gate remediation command",
        Recorder,
        DASHBOARD_LOCAL_PARITY_TESTS,
        "plan_detail_preserves_sections_errors_and_inert_argv"
    ),
    parity!(
        "gate_error",
        "Gate collection error",
        Recorder,
        DASHBOARD_LOCAL_TESTS,
        "gate_error_preserves_other_plans"
    ),
    parity!(
        "recent_failures",
        "Recent failures",
        Recorder,
        DASHBOARD_LOCAL_PARITY_TESTS,
        "repository_open_plans_failures_and_tool_times_keep_exact_semantics"
    ),
    parity!(
        "failure_stderr",
        "Failure stderr",
        Recorder,
        DASHBOARD_LOCAL_TESTS,
        "failure_stderr_is_bounded_and_scrollable"
    ),
    parity!(
        "closed_history",
        "Closed work history",
        Recorder,
        DASHBOARD_LOCAL_PARITY_TESTS,
        "work_timeline_and_health_render_typed_parity_fields"
    ),
    parity!(
        "tool_statistics",
        "Tool statistics",
        Recorder,
        DASHBOARD_LOCAL_TESTS,
        "tool_health_renders_all_aggregates"
    ),
    parity!(
        "loop_workflows",
        "Loop workflows",
        Recorder,
        DASHBOARD_LOCAL_TESTS,
        "loop_health_renders_workflow_and_lease_fields"
    ),
    parity!(
        "loop_leases",
        "Loop leases",
        Recorder,
        DASHBOARD_LOCAL_TESTS,
        "loop_health_renders_workflow_and_lease_fields"
    ),
    parity!(
        "exhausted_attempts",
        "Exhausted attempts",
        Recorder,
        DASHBOARD_LOCAL_TESTS,
        "exhausted_attempt_keeps_identity_and_inert_recovery_argv"
    ),
    parity!(
        "loop_clear_attempt",
        "Loop clear-attempt command",
        Recorder,
        DASHBOARD_LOCAL_TESTS,
        "exhausted_attempt_keeps_identity_and_inert_recovery_argv"
    ),
    parity!(
        "mixed_timeline",
        "Mixed timeline",
        Recorder,
        DASHBOARD_LOCAL_PARITY_TESTS,
        "mixed_timeline_is_newest_first_and_every_plan_row_opens_its_raw_id"
    ),
    parity!(
        "timeline_plan_link",
        "Plan-linked timeline navigation",
        Recorder,
        DASHBOARD_LOCAL_PARITY_TESTS,
        "mixed_timeline_is_newest_first_and_every_plan_row_opens_its_raw_id"
    ),
    parity!(
        "timeline_filter",
        "Timeline kind filter",
        Recorder,
        DASHBOARD_LOCAL_PARITY_TESTS,
        "timeline_filters_cover_every_kind_and_preserve_raw_identity"
    ),
    parity!(
        "timeline_limit",
        "Timeline limit",
        Recorder,
        DASHBOARD_EVENT_LOOP_TESTS,
        "timeline_limit_endpoints_and_plus_minus_controls_are_enforced"
    ),
    parity!(
        "plan_body",
        "Plan body",
        Recorder,
        DASHBOARD_LOCAL_PARITY_TESTS,
        "bounded_plan_body_and_fifty_receipts_remain_reachable"
    ),
    parity!(
        "plan_body_error",
        "Plan body error",
        Recorder,
        DASHBOARD_LOCAL_PARITY_TESTS,
        "plan_detail_preserves_sections_errors_and_inert_argv"
    ),
    parity!(
        "plan_baseline",
        "Plan baseline",
        Recorder,
        DASHBOARD_LOCAL_TESTS,
        "plan_summary_renders_baseline_values_and_errors"
    ),
    parity!(
        "plan_decisions",
        "Plan decisions",
        Recorder,
        DASHBOARD_LOCAL_PARITY_TESTS,
        "plan_detail_leaf_navigation_preserves_parent_state"
    ),
    parity!(
        "plan_receipts",
        "Plan receipts",
        Recorder,
        DASHBOARD_LOCAL_PARITY_TESTS,
        "bounded_plan_body_and_fifty_receipts_remain_reachable"
    ),
    parity!(
        "receipt_output",
        "Receipt stdout/stderr",
        Recorder,
        DASHBOARD_LOCAL_PARITY_TESTS,
        "receipt_output_and_paths_keep_independent_bounds"
    ),
    parity!(
        "receipt_paths",
        "Receipt changed paths",
        Recorder,
        DASHBOARD_LOCAL_PARITY_TESTS,
        "receipt_output_and_paths_keep_independent_bounds"
    ),
    parity!(
        "receipt_diff_duration",
        "Receipt diff and duration",
        Recorder,
        DASHBOARD_LOCAL_PARITY_TESTS,
        "plan_detail_leaf_navigation_preserves_parent_state"
    ),
    parity!(
        "dashboard_json",
        "Dashboard JSON route",
        Recorder,
        CLI_CUTOVER_TESTS,
        "recorder_json_emits_one_local_snapshot"
    ),
    parity!(
        "plan_json",
        "Plan JSON route",
        Recorder,
        CLI_CUTOVER_TESTS,
        "plan_json_uses_the_plan_schema_and_missing_plans_use_standard_errors"
    ),
    parity!(
        "auto_refresh",
        "Terminal auto-refresh",
        Recorder,
        DASHBOARD_SCHEDULER_TESTS,
        "automatic_refresh_is_completion_relative_and_single_domain"
    ),
    parity!(
        "status_overview",
        "Status overview structure",
        Status,
        DASHBOARD_MODEL_TESTS,
        "status_view_surfaces_local_repository_work_loops_and_errors"
    ),
    parity!(
        "repository_cleanliness",
        "Repository cleanliness and revision",
        Status,
        DASHBOARD_MODEL_TESTS,
        "status_view_surfaces_local_repository_work_loops_and_errors"
    ),
    parity!(
        "upstream",
        "Upstream tracking",
        Status,
        DASHBOARD_MODEL_TESTS,
        "status_view_surfaces_local_repository_work_loops_and_errors"
    ),
    parity!(
        "aggregate_errors",
        "Aggregate collection errors",
        Status,
        DASHBOARD_MODEL_TESTS,
        "status_view_surfaces_local_repository_work_loops_and_errors"
    ),
    parity!(
        "status_local_counts",
        "Status local work and loop counts",
        Status,
        DASHBOARD_MODEL_TESTS,
        "status_view_surfaces_local_repository_work_loops_and_errors"
    ),
    parity!(
        "status_refresh",
        "Single-domain status refresh lifecycle",
        Status,
        DASHBOARD_EVENT_LOOP_TESTS,
        "every_view_refreshes_the_single_recorder_domain"
    ),
    parity!(
        "http_authentication",
        "HTTP authentication",
        Removal,
        CLI_ARCHITECTURE_TESTS,
        "production_tree_contains_no_http_surface"
    ),
];
