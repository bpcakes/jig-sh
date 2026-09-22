//! Read-only inventory of currently held run worker leases.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;

use anyhow::{Context, Result, anyhow};

use super::{RUN_LEASE_DIR, run_lease_is_idle_at_root, validate_run_id_for_lease};

/// Returns every currently held worker lease, including leases whose journal
/// lifecycle is missing. Restore and read-only recovery diagnostics share this
/// inventory so their destination preflight cannot disagree.
pub(in crate::state) fn active_run_lease_ids(
    root: &Path,
    known_run_ids: impl IntoIterator<Item = String>,
) -> Result<Vec<String>> {
    let mut lease_run_ids = known_run_ids.into_iter().collect::<BTreeSet<_>>();
    let lease_dir = root.join(RUN_LEASE_DIR);
    match fs::read_dir(&lease_dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.with_context(|| {
                    format!(
                        "Failed to inspect run lease directory {}",
                        lease_dir.display()
                    )
                })?;
                let name = entry.file_name().into_string().map_err(|_| {
                    anyhow!(
                        "Run lease directory {} contains a non-UTF-8 entry",
                        lease_dir.display()
                    )
                })?;
                let Some(run_id) = name.strip_suffix(".lock") else {
                    continue;
                };
                validate_run_id_for_lease(run_id)?;
                lease_run_ids.insert(run_id.to_owned());
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Failed to inspect run lease directory {}",
                    lease_dir.display()
                )
            });
        }
    }

    let mut active_run_ids = Vec::new();
    for run_id in lease_run_ids {
        if !run_lease_is_idle_at_root(root, &run_id)? {
            active_run_ids.push(run_id);
        }
    }
    Ok(active_run_ids)
}
