use jig_contract::TargetId;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TargetReceiptStatus {
    pub(crate) receipt_id: String,
    pub(crate) run_id: Option<String>,
    pub(crate) plan_id: Option<String>,
    pub(crate) target_freshness: Option<jig_contract::freshness::TargetFreshnessMetadata>,
    pub(crate) target: TargetId,
    pub(crate) config_digest: Option<String>,
    pub(crate) input_digest: Option<String>,
    pub(crate) exit_status: i32,
    pub(crate) started_at_ms: u64,
    pub(crate) ended_at_ms: u64,
    pub(crate) changed_paths: Vec<String>,
    pub(crate) changed_path_count: usize,
    pub(crate) changed_paths_truncated: bool,
    pub(crate) changed_paths_digest: Option<String>,
    pub(crate) diff_summary: String,
    pub(crate) worktree_fingerprint: Option<String>,
    pub(crate) worktree_fingerprint_error: Option<String>,
    pub(crate) valid_until_ms: Option<u64>,
    pub(crate) requires_time_validity: bool,
}
