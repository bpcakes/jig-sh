use anyhow::{Result, bail};

pub(super) const PROVIDER_BEADS: &str = "beads";
pub(super) const TRACKER_ROOT_BEADS: &str = ".beads";
pub(super) const MAX_WORKSPACE_ID_BYTES: usize = 128;
pub(super) const MAX_ISSUE_ID_BYTES: usize = 256;

pub(super) fn validate_portable_tracker_issue(
    provider: &str,
    workspace_id: &str,
    issue_id: &str,
    tracker_root: &str,
) -> Result<()> {
    if provider != PROVIDER_BEADS {
        bail!("unsupported tracker provider {provider:?}");
    }
    validate_portable_identifier("tracker workspace id", workspace_id, MAX_WORKSPACE_ID_BYTES)?;
    validate_portable_identifier("issue id", issue_id, MAX_ISSUE_ID_BYTES)?;
    let normalized = crate::repository_path::normalize_portable_repository_directory(
        tracker_root,
        "tracker root",
    )?;
    if tracker_root != TRACKER_ROOT_BEADS || normalized != TRACKER_ROOT_BEADS {
        bail!("tracker root must be exactly {TRACKER_ROOT_BEADS:?}");
    }
    Ok(())
}

pub(super) fn validate_portable_identifier(
    label: &str,
    value: &str,
    max_bytes: usize,
) -> Result<()> {
    if value.is_empty()
        || value.len() > max_bytes
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        bail!(
            "{label} must contain 1 through {max_bytes} ASCII alphanumeric, underscore, hyphen, or dot bytes"
        );
    }
    Ok(())
}
