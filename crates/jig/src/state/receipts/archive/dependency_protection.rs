use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use jig_contract::freshness::{
    TARGET_FRESHNESS_CONTRACT_VERSION, TARGET_IDENTITY_DOMAIN, TARGET_IDENTITY_SCHEMA_VERSION,
    TargetFreshnessMetadata, TargetFreshnessStateV1,
};

use super::{JsonlWriteGuard, parse_raw_receipt, scan_jsonl_raw_locked, target_receipt_status};
use crate::state::{TargetReceiptStatus, time_validity_is_current};

/// Maintenance must be able to shrink journals larger than inspection limits.
/// Stream under the existing writer lock, retaining only the required frontier
/// each pass. Cycles terminate through `visited`; unknown required authority
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
    let mut pinned_bytes = 0_u64;
    while !pending.is_empty() {
        let originals = resolve_frontier(guard, path, &pending, &mut pinned_bytes).with_context(|| {
            format!(
                "Cannot represent archive dependency protection: {} pinned record IDs, {} unresolved IDs, at least {pinned_bytes} pinned journal bytes observed",
                protected.len(), pending.len(),
            )
        })?;
        let frontier = std::mem::take(&mut pending);
        for id in frontier {
            let original = originals
                .get(&id)
                .context("Cannot archive while an original dependency receipt is missing")?;
            if let Some(root) = roots.get(&id) {
                ensure!(
                    original == root,
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
                metadata.contract_epoch == TARGET_FRESHNESS_CONTRACT_VERSION
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

fn resolve_frontier(
    guard: &JsonlWriteGuard,
    path: &Path,
    pending: &BTreeSet<String>,
    pinned_bytes: &mut u64,
) -> Result<BTreeMap<String, TargetReceiptStatus>> {
    let mut originals = BTreeMap::new();
    let mut values = BTreeMap::new();
    let scan = scan_jsonl_raw_locked(guard, path, &|| false, |record| {
        let receipt = parse_raw_receipt(record, path)?;
        if pending.contains(&receipt.id) {
            *pinned_bytes = pinned_bytes.saturating_add(record.bytes.len() as u64);
            let value: serde_json::Value = serde_json::from_slice(record.bytes)?;
            if let Some(previous) = values.insert(receipt.id.clone(), value.clone()) {
                ensure!(
                    previous == value,
                    "Required original receipt has conflicting duplicate IDs"
                );
            }
            if let Some(target) = &receipt.target {
                originals.insert(receipt.id.clone(), target_receipt_status(&receipt, target));
            }
        }
        Ok(())
    })?;
    ensure!(
        !scan.unterminated_final_record,
        "Cannot archive an unterminated original receipt journal"
    );
    Ok(originals)
}
