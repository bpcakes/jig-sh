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

const DASHBOARD_CONTRACT_TESTS: &str = "crates/jig-ui/tests/dashboard_contract.rs";
const DASHBOARD_LOCAL_TESTS: &str = "crates/jig-ui/src/terminal/tests/local.rs";
const DASHBOARD_LOCAL_PARITY_TESTS: &str = "crates/jig-ui/src/terminal/tests/local/parity.rs";
const DASHBOARD_MODEL_TESTS: &str = "crates/jig-ui/src/terminal/tests.rs";
const DASHBOARD_STATUS_TESTS: &str = "crates/jig-ui/src/terminal/tests/status.rs";
const DASHBOARD_REGRESSION_TESTS: &str = "crates/jig-ui/src/terminal/tests/regressions.rs";
const DASHBOARD_EVENT_LOOP_TESTS: &str = "crates/jig-ui/src/terminal/runtime/event_loop.rs";
const DASHBOARD_SCHEDULER_TESTS: &str = "crates/jig-ui/src/terminal/runtime/scheduler/tests.rs";
const DASHBOARD_WORKER_TESTS: &str = "crates/jig-ui/src/terminal/runtime/worker/tests.rs";
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
        "recorder_json_exits_without_binding_or_running_providers"
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
        "Browser auto-refresh",
        Recorder,
        DASHBOARD_SCHEDULER_TESTS,
        "automatic_work_is_completion_relative_and_does_not_duplicate_pending"
    ),
    parity!(
        "status_overview",
        "Status overview structure",
        Status,
        DASHBOARD_STATUS_TESTS,
        "status_overview_local_counts_and_errors_are_reachable"
    ),
    parity!(
        "repository_cleanliness",
        "Repository cleanliness and revision",
        Status,
        DASHBOARD_STATUS_TESTS,
        "status_repository_and_upstream_variants_are_rendered"
    ),
    parity!(
        "upstream",
        "Upstream tracking",
        Status,
        DASHBOARD_STATUS_TESTS,
        "status_repository_and_upstream_variants_are_rendered"
    ),
    parity!(
        "provider_identity",
        "Provider identity",
        Status,
        DASHBOARD_STATUS_TESTS,
        "provider_identity_progress_and_failure_fields_are_reachable"
    ),
    parity!(
        "provider_status",
        "Provider status and duration",
        Status,
        DASHBOARD_STATUS_TESTS,
        "provider_status_variants_keep_status_and_duration_distinct"
    ),
    parity!(
        "provider_failure",
        "Provider failure detail",
        Status,
        DASHBOARD_STATUS_TESTS,
        "provider_identity_progress_and_failure_fields_are_reachable"
    ),
    parity!(
        "provider_progress",
        "Provider progress categories",
        Status,
        DASHBOARD_STATUS_TESTS,
        "provider_identity_progress_and_failure_fields_are_reachable"
    ),
    parity!(
        "provider_freshness",
        "Provider input freshness",
        Status,
        DASHBOARD_STATUS_TESTS,
        "provider_freshness_and_diagnostics_are_reachable"
    ),
    parity!(
        "provider_diagnostics",
        "Provider diagnostics",
        Status,
        DASHBOARD_STATUS_TESTS,
        "provider_freshness_and_diagnostics_are_reachable"
    ),
    parity!(
        "aggregate_errors",
        "Aggregate collection errors",
        Status,
        DASHBOARD_STATUS_TESTS,
        "status_overview_local_counts_and_errors_are_reachable"
    ),
    parity!(
        "status_local_counts",
        "Status local work and loop counts",
        Status,
        DASHBOARD_REGRESSION_TESTS,
        "recorder_refresh_reprojects_local_status_without_replacing_providers"
    ),
    parity!(
        "partition_age",
        "Status partition observation age",
        Status,
        DASHBOARD_REGRESSION_TESTS,
        "status_and_recorder_partitions_render_independent_ages"
    ),
    parity!(
        "provider_switching",
        "Provider switching",
        Status,
        DASHBOARD_MODEL_TESTS,
        "typed_snapshot_keeps_colliding_raw_identities_distinct_across_refresh"
    ),
    parity!(
        "package_selection",
        "Package list and selection",
        Status,
        DASHBOARD_MODEL_TESTS,
        "navigation_filters_and_preserves_stable_selection_across_refresh"
    ),
    parity!(
        "blocked_filter",
        "Blocked-only package filter",
        Status,
        DASHBOARD_MODEL_TESTS,
        "navigation_filters_and_preserves_stable_selection_across_refresh"
    ),
    parity!(
        "package_preview",
        "Package compact preview",
        Status,
        DASHBOARD_STATUS_TESTS,
        "package_preview_facets_and_dependencies_are_reachable"
    ),
    parity!(
        "package_facets",
        "Package facet detail",
        Status,
        DASHBOARD_STATUS_TESTS,
        "package_preview_facets_and_dependencies_are_reachable"
    ),
    parity!(
        "package_dependencies",
        "Package dependencies",
        Status,
        DASHBOARD_STATUS_TESTS,
        "package_preview_facets_and_dependencies_are_reachable"
    ),
    parity!(
        "package_acceptance",
        "Package acceptance checks",
        Status,
        DASHBOARD_STATUS_TESTS,
        "package_acceptance_blockers_and_evidence_are_reachable"
    ),
    parity!(
        "package_blockers",
        "Package blockers",
        Status,
        DASHBOARD_STATUS_TESTS,
        "package_acceptance_blockers_and_evidence_are_reachable"
    ),
    parity!(
        "package_evidence",
        "Package evidence",
        Status,
        DASHBOARD_STATUS_TESTS,
        "package_acceptance_blockers_and_evidence_are_reachable"
    ),
    parity!(
        "package_extensions",
        "Package extensions",
        Status,
        DASHBOARD_MODEL_TESTS,
        "extension_key_sanitization_preserves_colliding_entries_for_rendering"
    ),
    parity!(
        "blocker_navigation",
        "Blocker queue navigation",
        Status,
        DASHBOARD_MODEL_TESTS,
        "blocker_selection_survives_insertions_duplicate_codes_and_display_changes"
    ),
    parity!(
        "blocker_detail",
        "Blocker detail",
        Status,
        DASHBOARD_STATUS_TESTS,
        "package_acceptance_blockers_and_evidence_are_reachable"
    ),
    parity!(
        "provider_additive_fields",
        "Provider additive fields",
        Status,
        DASHBOARD_CONTRACT_TESTS,
        "accepted_provider_report_serializes_the_exact_raw_document_only"
    ),
    parity!(
        "status_refresh",
        "Status refresh lifecycle",
        Status,
        DASHBOARD_WORKER_TESTS,
        "phase_events_are_generation_tagged_and_preemption_joins_before_local_start"
    ),
    parity!(
        "http_authentication",
        "HTTP authentication",
        Removal,
        CLI_ARCHITECTURE_TESTS,
        "production_tree_contains_no_http_surface"
    ),
];
