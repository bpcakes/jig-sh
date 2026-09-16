use serde_json::json;

use super::{DoctorCheck, DoctorProcessControl, check};
use crate::context::RepoContext;
use crate::tracker::{BeadsExport, BeadsJsonlError, INPUT_PROFILE, LEGACY_EXPORT, PRIMARY_EXPORT};

pub(super) fn tracker_check(
    ctx: &RepoContext,
    _process_control: DoctorProcessControl<'_>,
) -> DoctorCheck {
    let Some(config) = ctx.work_tracker() else {
        return check(
            "tracker",
            "Work tracker",
            false,
            true,
            "not configured",
            "no task snapshot is configured",
        )
        .with_data(json!({ "configured": false }));
    };

    match BeadsExport::open(ctx.root(), config.workspace_id()) {
        Ok(export) => check(
            "tracker",
            "Work tracker",
            true,
            true,
            "ready",
            format!("configured Beads JSONL snapshot is readable through profile {INPUT_PROFILE}"),
        )
        .with_data(json!({
            "configured": true,
            "root": config.root(),
            "export": export.relative_path(),
            "profile": INPUT_PROFILE,
            "issues": export.len(),
            "supported_operations": ["read_issue_snapshot"],
            "write_authority": false,
        })),
        Err(error) => tracker_error(error, config.manual_export_guidance()),
    }
}

fn tracker_error(error: BeadsJsonlError, manual_guidance: Option<&str>) -> DoctorCheck {
    let (status, default_fix) = match error {
        BeadsJsonlError::MissingExport => (
            "missing export",
            format!(
                "Create exactly one current Beads export at `{PRIMARY_EXPORT}` (or the legacy `{LEGACY_EXPORT}`), then rerun `scripts/jig doctor`."
            ),
        ),
        BeadsJsonlError::AmbiguousExport => (
            "ambiguous export",
            format!(
                "Keep only the current Beads export at `{PRIMARY_EXPORT}` or `{LEGACY_EXPORT}`, then rerun `scripts/jig doctor`."
            ),
        ),
        BeadsJsonlError::InvalidWorkspace | BeadsJsonlError::UnsafeExport => (
            "unsafe export",
            "Repair the repository-local `.beads` directory and JSONL export so they are real, private files rather than links, then rerun `scripts/jig doctor`.".to_string(),
        ),
        BeadsJsonlError::ChangedDuringRead => (
            "export busy",
            "Wait for the Beads export to finish changing, then rerun `scripts/jig doctor`.".to_string(),
        ),
        BeadsJsonlError::ExportTooLarge
        | BeadsJsonlError::LineTooLong { .. }
        | BeadsJsonlError::TooManyIssues => (
            "export too large",
            "Reduce or partition the Beads export to the supported bounded profile, then rerun `scripts/jig doctor`.".to_string(),
        ),
        BeadsJsonlError::InvalidUtf8
        | BeadsJsonlError::InvalidRecord { .. }
        | BeadsJsonlError::DuplicateIssueId { .. } => (
            "invalid export",
            "Regenerate or repair the Beads JSONL export, then rerun `scripts/jig doctor`.".to_string(),
        ),
        BeadsJsonlError::InvalidIssueId
        | BeadsJsonlError::IssueMissing
        | BeadsJsonlError::IssueTombstoned => (
            "invalid export",
            "Regenerate or repair the Beads JSONL export, then rerun `scripts/jig doctor`.".to_string(),
        ),
    };
    let fix = manual_guidance.unwrap_or(&default_fix);
    check(
        "tracker",
        "Work tracker",
        true,
        false,
        status,
        format!("configured Beads JSONL diagnostics failed: {error}"),
    )
    .with_data(json!({
        "configured": true,
        "root": ".beads",
        "profile": INPUT_PROFILE,
        "write_authority": false,
    }))
    .with_fix(fix)
}
