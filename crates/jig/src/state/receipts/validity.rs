use jig_contract::freshness::{
    EffectiveTimeValidityV1, GlobalExecutionProofV1, TARGET_IDENTITY_DOMAIN,
    TARGET_IDENTITY_SCHEMA_VERSION, TargetFreshnessMetadata, TargetFreshnessStateV1,
    supported_freshness_epoch,
};
use serde_json::Value;

use super::{ReceiptRecord, evidence_requires_time_validity};

/// Extract the declared effective deadline. This is a time-only constraint;
/// it never grants identity validity or replaces original proof validation.
pub(crate) fn metadata_time(metadata: &TargetFreshnessMetadata) -> EffectiveTimeValidityV1 {
    let unknown = EffectiveTimeValidityV1::new(None, true);
    let TargetFreshnessMetadata::V1(metadata) = metadata else {
        return unknown;
    };
    if !supported_freshness_epoch(metadata.contract_epoch)
        || metadata.schema_version != TARGET_IDENTITY_SCHEMA_VERSION
    {
        return unknown;
    }
    if let TargetFreshnessStateV1::Complete { identity, .. } = &metadata.state
        && (identity.digest_domain != TARGET_IDENTITY_DOMAIN
            || identity.contract_epoch != metadata.contract_epoch
            || identity.schema_version != metadata.schema_version
            || !matches!(&metadata.global_execution_proof, GlobalExecutionProofV1::Unchanged { before_source_digest, after_source_digest }
            if !before_source_digest.is_empty() && before_source_digest == after_source_digest))
    {
        return unknown;
    }
    EffectiveTimeValidityV1::new(
        metadata.effective_valid_until_ms,
        metadata.effective_requires_time_validity,
    )
}

pub(crate) fn effective_time_from_value(value: &Value) -> Option<EffectiveTimeValidityV1> {
    let projected = (value.get("effective_valid_until_ms").is_some()
        || value.get("effective_requires_time_validity").is_some())
    .then(|| {
        serde_json::from_value::<EffectiveTimeValidityV1>(value.clone())
            .unwrap_or_else(|_| EffectiveTimeValidityV1::new(None, true))
    });
    let metadata = value.get("target_freshness").map(|metadata| {
        serde_json::from_value::<TargetFreshnessMetadata>(metadata.clone()).map_or_else(
            |_| EffectiveTimeValidityV1::new(None, true),
            |metadata| metadata_time(&metadata),
        )
    });
    match (projected, metadata) {
        (Some(left), Some(right)) => Some(left.combine(right)),
        (left, right) => left.or(right),
    }
}

pub(crate) fn receipt_effective_time(receipt: &ReceiptRecord) -> Option<EffectiveTimeValidityV1> {
    let own = EffectiveTimeValidityV1::new(
        receipt.valid_until_ms,
        receipt
            .evidence
            .as_ref()
            .is_some_and(evidence_requires_time_validity),
    );
    let metadata = receipt.target_freshness.as_ref().map(metadata_time);
    let evidence = receipt
        .evidence
        .as_ref()
        .and_then(effective_time_from_value);
    metadata
        .or(evidence)
        .map(|first| own.combine(first).combine(evidence.unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn incomplete_freshness_preserves_time_constraints_without_inventing_one() {
        for (boundary, required) in [(None, false), (Some(100), true), (None, true)] {
            let value = json!({"target_freshness": {
                "schema_version": 1, "contract_epoch": 8, "state": "incomplete",
                "global_execution_proof": {"state": "unknown"},
                "effective_valid_until_ms": boundary, "effective_requires_time_validity": required,
                "reasons": [{"code": "collection_limit"}], "reasons_total": 1,
                "reasons_truncated": false
            }});
            assert_eq!(
                effective_time_from_value(&value),
                Some(EffectiveTimeValidityV1::new(boundary, required))
            );
        }
        assert_eq!(
            effective_time_from_value(&json!({"target_freshness": {"schema_version": 99}})),
            Some(EffectiveTimeValidityV1::new(None, true))
        );
    }
}
