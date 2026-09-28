use jig_contract::freshness::{
    EffectiveTimeValidityV1, GlobalExecutionProofV1, TARGET_IDENTITY_DOMAIN,
    TARGET_IDENTITY_SCHEMA_VERSION, TargetFreshnessMetadata, TargetFreshnessStateV1,
    supported_freshness_epoch,
};

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
