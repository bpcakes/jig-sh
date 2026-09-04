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
    pub behavioral_test: &'static str,
}

macro_rules! parity {
    ($key:literal, $capability:literal, $area:ident, $test:literal) => {
        ParityEntry {
            key: $key,
            capability: $capability,
            area: ParityArea::$area,
            behavioral_test: $test,
        }
    };
}

/// One entry for every row in project-plan section 5.6.
///
/// The named tests are implemented by the owning delivery tasks. Keeping the
/// registry here makes omissions visible before the terminal cutover.
pub const PARITY_REGISTRY: &[ParityEntry] = &[
    parity!(
        "repository_identity",
        "Repository, harness, and default-branch identity",
        Shared,
        "header_renders_repository_harness_and_default_branch"
    ),
    parity!(
        "current_revision",
        "Current branch or detached revision",
        Shared,
        "header_renders_branch_and_detached_revision"
    ),
    parity!(
        "state_counts",
        "Session, plan, and decision counts",
        Recorder,
        "work_summary_renders_seeded_state_counts"
    ),
    parity!(
        "open_plans",
        "Open plans",
        Recorder,
        "work_list_selects_every_open_plan"
    ),
    parity!(
        "gate_table",
        "Gate table",
        Recorder,
        "gate_detail_exposes_every_gate_field"
    ),
    parity!(
        "gate_remediation",
        "Gate remediation command",
        Recorder,
        "gate_remediation_preserves_inert_argv"
    ),
    parity!(
        "gate_error",
        "Gate collection error",
        Recorder,
        "gate_error_preserves_other_plans"
    ),
    parity!(
        "recent_failures",
        "Recent failures",
        Recorder,
        "health_failures_are_complete_and_newest_first"
    ),
    parity!(
        "failure_stderr",
        "Failure stderr",
        Recorder,
        "failure_stderr_is_bounded_and_scrollable"
    ),
    parity!(
        "closed_history",
        "Closed work history",
        Recorder,
        "closed_history_renders_resolution_and_duration"
    ),
    parity!(
        "tool_statistics",
        "Tool statistics",
        Recorder,
        "tool_health_renders_all_aggregates"
    ),
    parity!(
        "loop_workflows",
        "Loop workflows",
        Recorder,
        "loop_health_renders_kind_and_enabled_state"
    ),
    parity!(
        "loop_leases",
        "Loop leases",
        Recorder,
        "loop_health_renders_lease_key_and_expiry"
    ),
    parity!(
        "exhausted_attempts",
        "Exhausted attempts",
        Recorder,
        "exhausted_attempt_uses_producer_native_identity"
    ),
    parity!(
        "loop_clear_attempt",
        "Loop clear-attempt command",
        Recorder,
        "loop_recovery_preserves_inert_argv"
    ),
    parity!(
        "mixed_timeline",
        "Mixed timeline",
        Recorder,
        "timeline_renders_every_kind_newest_first"
    ),
    parity!(
        "timeline_plan_link",
        "Plan-linked timeline navigation",
        Recorder,
        "timeline_enter_uses_raw_plan_identity"
    ),
    parity!(
        "timeline_filter",
        "Timeline kind filter",
        Recorder,
        "timeline_filter_covers_every_kind"
    ),
    parity!(
        "timeline_limit",
        "Timeline limit",
        Recorder,
        "timeline_limit_enforces_one_and_one_thousand"
    ),
    parity!(
        "plan_body",
        "Plan body",
        Recorder,
        "plan_body_is_bounded_and_sanitized_for_display"
    ),
    parity!(
        "plan_body_error",
        "Plan body error",
        Recorder,
        "plan_body_error_preserves_other_detail"
    ),
    parity!(
        "plan_baseline",
        "Plan baseline",
        Recorder,
        "plan_summary_renders_all_baseline_states"
    ),
    parity!(
        "plan_decisions",
        "Plan decisions",
        Recorder,
        "plan_decisions_render_selection_alternatives_and_rationale"
    ),
    parity!(
        "plan_receipts",
        "Plan receipts",
        Recorder,
        "plan_receipts_cap_and_preserve_selection"
    ),
    parity!(
        "receipt_output",
        "Receipt stdout/stderr",
        Recorder,
        "receipt_output_previews_are_independently_bounded"
    ),
    parity!(
        "receipt_paths",
        "Receipt changed paths",
        Recorder,
        "receipt_paths_render_omission_count"
    ),
    parity!(
        "receipt_diff_duration",
        "Receipt diff and duration",
        Recorder,
        "receipt_detail_renders_diff_and_duration"
    ),
    parity!(
        "dashboard_json",
        "Dashboard JSON route",
        Recorder,
        "ui_json_emits_one_recorder_document"
    ),
    parity!(
        "plan_json",
        "Plan JSON route",
        Recorder,
        "ui_plan_json_handles_found_and_not_found"
    ),
    parity!(
        "auto_refresh",
        "Browser auto-refresh",
        Recorder,
        "recorder_refresh_is_completion_relative"
    ),
    parity!(
        "status_overview",
        "Status overview structure",
        Status,
        "status_overview_migrates_existing_assertions"
    ),
    parity!(
        "repository_cleanliness",
        "Repository cleanliness and revision",
        Status,
        "status_repository_renders_clean_dirty_branch_and_detached"
    ),
    parity!(
        "upstream",
        "Upstream tracking",
        Status,
        "status_upstream_renders_every_state_and_basis"
    ),
    parity!(
        "provider_identity",
        "Provider identity",
        Status,
        "provider_identity_keeps_raw_id_name_and_version"
    ),
    parity!(
        "provider_status",
        "Provider status and duration",
        Status,
        "provider_header_renders_status_and_duration"
    ),
    parity!(
        "provider_failure",
        "Provider failure detail",
        Status,
        "provider_failure_renders_bounded_diagnostics"
    ),
    parity!(
        "provider_progress",
        "Provider progress categories",
        Status,
        "provider_progress_renders_all_totals_and_categories"
    ),
    parity!(
        "provider_freshness",
        "Provider input freshness",
        Status,
        "provider_freshness_renders_all_fields"
    ),
    parity!(
        "provider_diagnostics",
        "Provider diagnostics",
        Status,
        "provider_diagnostics_render_all_fields"
    ),
    parity!(
        "aggregate_errors",
        "Aggregate collection errors",
        Status,
        "status_errors_preserve_provider_and_local_data"
    ),
    parity!(
        "status_local_counts",
        "Status local work and loop counts",
        Status,
        "status_local_summary_renders_all_counts"
    ),
    parity!(
        "partition_age",
        "Status partition observation age",
        Status,
        "status_partitions_age_independently"
    ),
    parity!(
        "provider_switching",
        "Provider switching",
        Status,
        "provider_selection_survives_reorder_by_raw_id"
    ),
    parity!(
        "package_selection",
        "Package list and selection",
        Status,
        "package_selection_survives_refresh_by_raw_id"
    ),
    parity!(
        "blocked_filter",
        "Blocked-only package filter",
        Status,
        "blocked_filter_clamps_raw_selection"
    ),
    parity!(
        "package_preview",
        "Package compact preview",
        Status,
        "package_preview_renders_at_supported_width"
    ),
    parity!(
        "package_facets",
        "Package facet detail",
        Status,
        "package_detail_renders_all_facet_fields"
    ),
    parity!(
        "package_dependencies",
        "Package dependencies",
        Status,
        "package_detail_reaches_every_dependency"
    ),
    parity!(
        "package_acceptance",
        "Package acceptance checks",
        Status,
        "package_detail_renders_acceptance_fields"
    ),
    parity!(
        "package_blockers",
        "Package blockers",
        Status,
        "package_detail_renders_blocker_fields"
    ),
    parity!(
        "package_evidence",
        "Package evidence",
        Status,
        "package_detail_renders_evidence_fields"
    ),
    parity!(
        "package_extensions",
        "Package extensions",
        Status,
        "package_extensions_preserve_namespaced_collisions"
    ),
    parity!(
        "blocker_navigation",
        "Blocker queue navigation",
        Status,
        "blocker_queue_uses_stable_composite_keys"
    ),
    parity!(
        "blocker_detail",
        "Blocker detail",
        Status,
        "blocker_detail_renders_all_fields"
    ),
    parity!(
        "provider_additive_fields",
        "Provider additive fields",
        Status,
        "provider_unknown_fields_round_trip_and_render"
    ),
    parity!(
        "status_refresh",
        "Status refresh lifecycle",
        Status,
        "status_refresh_preserves_and_strengthens_lifecycle"
    ),
    parity!(
        "http_authentication",
        "HTTP authentication",
        Removal,
        "production_tree_contains_no_http_surface"
    ),
];
