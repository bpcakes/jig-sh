use serde_json::json;

use super::{DoctorCheck, DoctorProcessControl, check};
use crate::context::RepoContext;
use crate::tracker::{
    BeadsAdapter, TrackerCapability, TrackerDiscovery, TrackerError, TrackerProcessPolicy,
    TrackerProfile,
};

pub(super) fn tracker_check(
    ctx: &RepoContext,
    process_control: DoctorProcessControl<'_>,
) -> DoctorCheck {
    let Some(config) = ctx.work_tracker() else {
        return check(
            "tracker",
            "Work tracker",
            false,
            true,
            "not configured",
            "no external work tracker is configured",
        )
        .with_data(json!({ "configured": false }));
    };

    if let Some(reason) = process_control.unavailable_reason {
        return tracker_failure(
            "unavailable",
            format!("configured Beads diagnostics are unavailable because {reason}"),
            None,
        )
        .with_fix(
            "Rerun `scripts/jig doctor` in a session where subprocess diagnostics are available.",
        );
    }

    let mut cancelled = || {
        process_control
            .cancellation
            .is_some_and(|cancelled| cancelled())
    };
    let (adapter, discovery) = match BeadsAdapter::discover(
        ctx.root(),
        config.workspace_id(),
        TrackerProcessPolicy::default(),
        &mut cancelled,
    ) {
        Ok(discovery) => discovery,
        Err(error) => return tracker_error(error, None),
    };

    if discovery.profile == TrackerProfile::Unsupported {
        return tracker_failure(
            "unsupported",
            format!(
                "configured Beads version {} has no supported Jig adapter profile",
                discovery.version
            ),
            Some(&discovery),
        )
        .with_fix(
            "Install supported `br 0.5.7`, or remove `[work.tracker]` if this repository no longer uses Beads, then rerun `scripts/jig doctor`.",
        );
    }

    if let Err(error) = adapter.check_storage_readiness(&mut cancelled) {
        return tracker_error(error, Some(&discovery));
    }

    check(
        "tracker",
        "Work tracker",
        true,
        true,
        "ready",
        format!(
            "configured Beads workspace is readable through profile {}",
            profile_label(discovery.profile)
        ),
    )
    .with_data(discovery_data(&discovery))
}

pub(super) fn tracker_error(
    error: TrackerError,
    discovery: Option<&TrackerDiscovery>,
) -> DoctorCheck {
    let fix = match &error {
        TrackerError::BinaryMissing | TrackerError::UnsupportedBinary { .. } => {
            Some("Install supported `br 0.5.7`, then rerun `scripts/jig doctor`.")
        }
        TrackerError::ExecutableCandidateInvalid => Some(
            "Remove or repair the earlier unusable `br` candidate on `PATH`, or install supported `br 0.5.7`, then rerun `scripts/jig doctor`.",
        ),
        TrackerError::ExecutableSnapshotUnavailable => Some(
            "Run Jig in an environment that permits private immutable executable snapshots, then rerun `scripts/jig doctor`.",
        ),
        TrackerError::UnsupportedPlatform => Some(
            "Run the configured Beads adapter on Linux or macOS, or remove `[work.tracker]` on this host.",
        ),
        TrackerError::StoreChangedDuringSnapshot => {
            Some("Wait for concurrent Beads activity to finish, then rerun `scripts/jig doctor`.")
        }
        TrackerError::StoreSnapshotTimedOut => Some(
            "Quiesce Beads activity or reduce the repository-local tracker store, then rerun `scripts/jig doctor`.",
        ),
        TrackerError::ExecutableChanged => Some(
            "Restore the discovered `br` executable or rerun `scripts/jig doctor` to establish a new trusted executable snapshot.",
        ),
        TrackerError::InvalidWorkspace {
            reason: crate::tracker::InvalidWorkspaceReason::HardLinkedAuthority,
        } => Some(
            "Replace the hard-linked tracker authority file with an independent repository-local file, then rerun `scripts/jig doctor`.",
        ),
        TrackerError::InvalidWorkspace { .. } => Some(
            "Repair the repository-local `.beads` workspace boundary, then rerun `scripts/jig doctor`.",
        ),
        TrackerError::StaleStorage => Some(
            "Reconcile the repository-local Beads database and JSONL export, then rerun `scripts/jig doctor`.",
        ),
        TrackerError::StoreSnapshotTooLarge { .. } => Some(
            "Reduce the repository-local tracker store below the supported snapshot limit, then rerun `scripts/jig doctor`.",
        ),
        TrackerError::UnsafeTemporaryDirectory { .. } => Some(
            "Set `TMPDIR` to a private directory outside the repository, then rerun `scripts/jig doctor`.",
        ),
        TrackerError::InvalidInput { .. }
        | TrackerError::UnsupportedResponse { .. }
        | TrackerError::IssueMissing { .. }
        | TrackerError::IssueTombstoned { .. }
        | TrackerError::BlockedTransition { .. }
        | TrackerError::AssignmentConflict { .. }
        | TrackerError::AmbiguousIssueId { .. }
        | TrackerError::TimedOut { .. }
        | TrackerError::CancelledBeforeStart { .. }
        | TrackerError::Cancelled { .. }
        | TrackerError::OutputLimit { .. }
        | TrackerError::ProcessFailure { .. }
        | TrackerError::IndeterminateWrite { .. } => None,
    };
    let status = match &error {
        TrackerError::BinaryMissing => "missing",
        TrackerError::ExecutableCandidateInvalid => "invalid executable",
        TrackerError::ExecutableSnapshotUnavailable => "unsupported environment",
        TrackerError::ExecutableChanged => "changed binary",
        TrackerError::UnsupportedBinary { .. } => "unsupported",
        TrackerError::UnsupportedPlatform => "unsupported platform",
        TrackerError::InvalidWorkspace { .. } => "invalid workspace",
        TrackerError::StaleStorage => "stale storage",
        TrackerError::StoreSnapshotTooLarge { .. } => "snapshot too large",
        TrackerError::StoreChangedDuringSnapshot => "store busy",
        TrackerError::StoreSnapshotTimedOut => "snapshot timed out",
        TrackerError::UnsafeTemporaryDirectory { .. } => "unsafe temporary directory",
        TrackerError::TimedOut { .. } => "timed out",
        TrackerError::CancelledBeforeStart { .. } | TrackerError::Cancelled { .. } => "cancelled",
        TrackerError::OutputLimit { .. } => "output limit",
        TrackerError::UnsupportedResponse { .. } => "unsupported response",
        TrackerError::InvalidInput { .. }
        | TrackerError::IssueMissing { .. }
        | TrackerError::IssueTombstoned { .. }
        | TrackerError::BlockedTransition { .. }
        | TrackerError::AssignmentConflict { .. }
        | TrackerError::AmbiguousIssueId { .. }
        | TrackerError::ProcessFailure { .. }
        | TrackerError::IndeterminateWrite { .. } => "error",
    };
    let check = tracker_failure(
        status,
        format!("configured Beads diagnostics failed: {error}"),
        discovery,
    );
    if let Some(fix) = fix {
        check.with_fix(fix)
    } else {
        check
    }
}

fn tracker_failure(
    status: &str,
    detail: String,
    discovery: Option<&TrackerDiscovery>,
) -> DoctorCheck {
    check("tracker", "Work tracker", true, false, status, detail).with_data(discovery.map_or_else(
        || json!({ "configured": true, "root": ".beads" }),
        discovery_data,
    ))
}

fn discovery_data(discovery: &TrackerDiscovery) -> serde_json::Value {
    json!({
        "configured": true,
        "root": ".beads",
        "version": discovery.version,
        "profile": profile_label(discovery.profile),
        "supported_operations": discovery
            .capabilities
            .iter()
            .map(|capability| capability_label(*capability))
            .collect::<Vec<_>>(),
    })
}

const fn profile_label(profile: TrackerProfile) -> &'static str {
    match profile {
        TrackerProfile::Beads0_5_7 => "beads_0_5_7",
        TrackerProfile::Unsupported => "unsupported",
    }
}

const fn capability_label(capability: TrackerCapability) -> &'static str {
    match capability {
        TrackerCapability::ShowIssue => "show_issue",
        TrackerCapability::ListComments => "list_comments",
        TrackerCapability::AddComment => "add_comment",
        TrackerCapability::ClaimIssue => "claim_issue",
        TrackerCapability::CloseIssue => "close_issue",
    }
}
