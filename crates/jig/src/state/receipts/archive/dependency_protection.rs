use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use jig_contract::freshness::{
    TARGET_IDENTITY_DOMAIN, TARGET_IDENTITY_SCHEMA_VERSION, TargetFreshnessMetadata,
    TargetFreshnessStateV1, supported_freshness_epoch,
};

use super::JsonlWriteGuard;
use crate::state::{TargetReceiptStatus, time_validity_is_current};
use originals::ArchiveOriginalIndex;

/// Maintenance must be able to shrink journals larger than inspection limits.
/// Index record locations once under the existing writer lock, then read only
/// required originals. Cycles terminate through `visited`; unknown required authority
/// still refuses deletion. This never grants freshness or bypasses inspection.
pub(super) fn protect_dependencies(
    guard: &JsonlWriteGuard,
    path: &Path,
    roots: impl Iterator<Item = TargetReceiptStatus>,
    protected: &mut BTreeSet<String>,
    now_ms: u64,
) -> Result<()> {
    let roots: BTreeMap<_, _> = roots.map(|root| (root.receipt_id.clone(), root)).collect();
    let mut pending: BTreeSet<_> = roots.keys().cloned().collect();
    let mut visited = BTreeSet::new();
    if pending.is_empty() {
        return Ok(());
    }
    let mut originals = ArchiveOriginalIndex::open(guard, path)?;
    while !pending.is_empty() {
        let frontier = std::mem::take(&mut pending);
        for id in frontier {
            let original = originals.get(&id).with_context(|| {
                format!(
                    "Cannot represent archive dependency protection for receipt {id}: {} pinned record IDs, {} original bytes loaded",
                    protected.len(), originals.loaded_bytes,
                )
            })?;
            if let Some(root) = roots.get(&id) {
                ensure!(
                    &original == root,
                    "Selected original receipt changed during archive protection"
                );
            }
            visited.insert(id.clone());
            protected.insert(id);
            let metadata = match &original.target_freshness {
                None => continue,
                Some(TargetFreshnessMetadata::Unsupported(_)) => {
                    bail!(
                        "Cannot archive protected dependency evidence with an unsupported freshness schema"
                    );
                }
                Some(TargetFreshnessMetadata::V1(metadata)) => metadata,
            };
            ensure!(
                supported_freshness_epoch(metadata.contract_epoch)
                    && metadata.schema_version == TARGET_IDENTITY_SCHEMA_VERSION,
                "Cannot archive protected dependency evidence with an unsupported authority version"
            );
            if let TargetFreshnessStateV1::Complete {
                identity,
                dependency_execution_proof,
            } = &metadata.state
            {
                ensure!(
                    identity.digest_domain == TARGET_IDENTITY_DOMAIN
                        && identity.contract_epoch == metadata.contract_epoch
                        && identity.schema_version == metadata.schema_version,
                    "Cannot archive protected dependency evidence with an unsupported identity authority"
                );
                if !time_validity_is_current(
                    metadata.effective_valid_until_ms,
                    metadata.effective_requires_time_validity,
                    now_ms,
                ) {
                    continue;
                }
                pending.extend(
                    dependency_execution_proof
                        .iter()
                        .map(|reference| reference.receipt_id.clone()),
                );
            }
        }
        pending.retain(|id| !visited.contains(id));
    }
    Ok(())
}

mod originals;

#[cfg(test)]
mod tests;
