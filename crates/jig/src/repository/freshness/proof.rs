use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use jig_contract::RunConclusion;
use jig_contract::freshness::{
    DependencyExecutionProofV1, FreshnessReason, FreshnessReasonCode as Code, FreshnessReasons,
    GlobalExecutionProofV1, TARGET_FRESHNESS_CONTRACT_VERSION, TARGET_IDENTITY_DOMAIN,
    TARGET_IDENTITY_SCHEMA_VERSION, TargetFreshness, TargetFreshnessMetadata,
    TargetFreshnessStateV1, TargetFreshnessStatus as Status, TargetFreshnessV1, TargetIdentityV1,
};

use super::{CollectionBudget, CollectionFailure, CollectionResult};
use crate::state::{OriginalReceiptIndex, TargetReceiptStatus};

mod comparison;
#[cfg(test)]
mod tests;

/// The cache key is the original receipt ID. Different executions of a shared
/// target are distinct proof nodes, even when their identity tokens are equal.
pub(crate) struct OriginalProofValidator {
    plan_id: Option<String>,
    now_ms: u64,
    originals: OriginalReceiptIndex,
    loaded: BTreeMap<String, Rc<TargetReceiptStatus>>,
    validated: BTreeMap<String, TargetFreshness>,
}

impl OriginalProofValidator {
    pub(crate) fn new(originals: OriginalReceiptIndex, plan_id: &str, now_ms: u64) -> Self {
        Self::for_receipt_plan(originals, Some(plan_id), now_ms)
    }

    pub(crate) fn for_receipt_plan(
        originals: OriginalReceiptIndex,
        plan_id: Option<&str>,
        now_ms: u64,
    ) -> Self {
        Self {
            plan_id: plan_id.map(str::to_owned),
            now_ms,
            originals,
            loaded: BTreeMap::new(),
            validated: BTreeMap::new(),
        }
    }

    pub(crate) fn evaluate(
        &mut self,
        selected: &TargetReceiptStatus,
        expected: &CollectionResult<TargetIdentityV1>,
        budget: &mut CollectionBudget<'_>,
    ) -> TargetFreshness {
        let mut result = self.evaluate_original(selected, budget);
        compare_current_identity(&mut result, selected, expected);
        result
    }

    pub(crate) fn evaluate_original(
        &mut self,
        selected: &TargetReceiptStatus,
        budget: &mut CollectionBudget<'_>,
    ) -> TargetFreshness {
        if !self.originals.selected_is_current(selected) {
            return unverified(
                selected,
                CollectionFailure::new(
                    Code::SourceRaced,
                    "a newer original receipt appeared during target selection",
                )
                .reason,
                self.now_ms,
            );
        }
        let mut result = match self.resolve(&selected.receipt_id, budget) {
            Ok(result) => result,
            Err(error) => unverified(selected, error.reason, self.now_ms),
        };
        if self
            .loaded
            .get(&selected.receipt_id)
            .is_some_and(|original| original.as_ref() != selected)
        {
            add(&mut result, Status::Unknown, Code::SourceRaced);
        }
        result
    }

    pub(crate) fn revalidate(&self, budget: &CollectionBudget<'_>) -> CollectionResult<()> {
        self.originals.revalidate(budget)
    }

    fn resolve(
        &mut self,
        root: &str,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<TargetFreshness> {
        let mut pending = vec![(root.to_owned(), false)];
        let mut visiting = BTreeSet::new();
        while let Some((id, exiting)) = pending.pop() {
            budget.ensure_active()?;
            if self.validated.contains_key(&id) {
                continue;
            }
            if exiting {
                let receipt = self.loaded[&id].clone();
                let result = self.validate_original(&receipt, self.plan_id.as_deref(), self.now_ms);
                self.validated.insert(id.clone(), result);
                visiting.remove(&id);
                continue;
            }
            if !visiting.insert(id.clone()) {
                return Err(CollectionFailure::new(
                    Code::DependencyProofInvalid,
                    "dependency execution proof contains a cycle",
                ));
            }
            if !self.loaded.contains_key(&id) {
                budget.target()?;
                let receipt = self.originals.get(&id, budget)?.ok_or_else(|| {
                    CollectionFailure::new(
                        Code::DependencyProofMissing,
                        "an original target receipt required by dependency proof is missing",
                    )
                })?;
                self.loaded.insert(id.clone(), Rc::new(receipt));
            }
            pending.push((id.clone(), true));
            if let Some((_, references)) = complete(&self.loaded[&id]) {
                budget.edges(references.len() as u64)?;
                for reference in references.iter().rev() {
                    if visiting.contains(&reference.receipt_id) {
                        return Err(CollectionFailure::new(
                            Code::DependencyProofInvalid,
                            "dependency execution proof contains a cycle",
                        ));
                    }
                    if !self.validated.contains_key(&reference.receipt_id) {
                        pending.push((reference.receipt_id.clone(), false));
                    }
                }
            }
        }
        budget.ensure_active()?;
        Ok(self.validated[root].clone())
    }

    fn validate_original(
        &self,
        receipt: &TargetReceiptStatus,
        plan_id: Option<&str>,
        now_ms: u64,
    ) -> TargetFreshness {
        let mut result = empty();
        if receipt.plan_id.as_deref() != plan_id
            || receipt.run_id.as_deref().is_none_or(str::is_empty)
            || plan_id.is_some_and(str::is_empty)
            || receipt.ended_at_ms < receipt.started_at_ms
        {
            add(&mut result, Status::Unknown, Code::DependencyProofInvalid);
        }
        let metadata = match &receipt.target_freshness {
            None => {
                add(&mut result, Status::Unknown, Code::LegacyMetadata);
                return result;
            }
            Some(TargetFreshnessMetadata::Unsupported(_)) => {
                add(&mut result, Status::Unsupported, Code::UnsupportedAuthority);
                return result;
            }
            Some(TargetFreshnessMetadata::V1(metadata)) => metadata,
        };
        check_version(
            &mut result,
            metadata.contract_epoch,
            metadata.schema_version,
            TARGET_IDENTITY_DOMAIN,
        );
        result.effective_valid_until_ms = metadata.effective_valid_until_ms;
        result.effective_requires_time_validity = metadata.effective_requires_time_validity;
        match &metadata.global_execution_proof {
            GlobalExecutionProofV1::Unchanged {
                before_source_digest,
                after_source_digest,
            } if !before_source_digest.is_empty()
                && before_source_digest == after_source_digest => {}
            GlobalExecutionProofV1::Mutated => {
                add(&mut result, Status::Unknown, Code::ExecutionMutated)
            }
            _ => add(&mut result, Status::Unknown, Code::CollectionFailed),
        }
        match &metadata.state {
            TargetFreshnessStateV1::Incomplete { reasons } => {
                result.status = result.status.combine(Status::Unknown);
                for reason in &reasons.reasons {
                    result.reasons.push(reason.clone());
                }
                result.reasons.reasons_total = result.reasons.reasons_total.saturating_add(
                    reasons
                        .reasons_total
                        .saturating_sub(reasons.reasons.len() as u64),
                );
                result.reasons.reasons_truncated |= reasons.reasons_truncated;
                if reasons.reasons.is_empty() {
                    add(&mut result, Status::Unknown, Code::CollectionFailed);
                }
            }
            TargetFreshnessStateV1::Complete {
                identity,
                dependency_execution_proof,
            } => {
                if bounded_identity(identity) {
                    result.recorded_identity = Some(identity.as_ref().into());
                } else {
                    add(&mut result, Status::Unknown, Code::DependencyProofInvalid);
                }
                check_version(
                    &mut result,
                    identity.contract_epoch,
                    identity.schema_version,
                    &identity.digest_domain,
                );
                if identity.target != receipt.target
                    || identity.contract_epoch != metadata.contract_epoch
                {
                    add(&mut result, Status::Unknown, Code::DependencyProofInvalid);
                }
                if identity.contract_epoch == TARGET_FRESHNESS_CONTRACT_VERSION
                    && identity.schema_version == TARGET_IDENTITY_SCHEMA_VERSION
                    && identity.digest_domain == TARGET_IDENTITY_DOMAIN
                    && !identity_encoding_is_consistent(identity)
                {
                    add(&mut result, Status::Unknown, Code::DependencyProofInvalid);
                }
                self.validate_dependencies(
                    &mut result,
                    receipt,
                    metadata,
                    identity,
                    dependency_execution_proof,
                    plan_id,
                );
            }
        }
        apply_time(&mut result, now_ms);
        result
    }

    fn validate_dependencies(
        &self,
        result: &mut TargetFreshness,
        receipt: &TargetReceiptStatus,
        metadata: &TargetFreshnessV1,
        identity: &TargetIdentityV1,
        references: &[DependencyExecutionProofV1],
        plan_id: Option<&str>,
    ) {
        let mut time = jig_contract::freshness::EffectiveTimeValidityV1::new(
            receipt.valid_until_ms,
            receipt.requires_time_validity,
        );
        if receipt.requires_time_validity && receipt.valid_until_ms.is_none() {
            add(result, Status::Unknown, Code::TimeBoundaryMissing);
        }
        if identity.dependencies.len() != references.len() {
            add(result, Status::Unknown, Code::DependencyProofMissing);
        }
        if !references
            .windows(2)
            .all(|pair| pair[0].target < pair[1].target)
            || !identity
                .dependencies
                .windows(2)
                .all(|pair| pair[0].target < pair[1].target)
        {
            add(result, Status::Unknown, Code::DependencyProofInvalid);
        }
        for (index, reference) in references.iter().enumerate() {
            let dependency = identity.dependencies.get(index);
            if dependency.is_none_or(|dependency| {
                dependency.target != reference.target
                    || dependency.identity_digest != reference.identity_digest
            }) {
                add(result, Status::Unknown, Code::DependencyProofInvalid);
            }
            let Some(original) = self.loaded.get(&reference.receipt_id) else {
                add(result, Status::Unknown, Code::DependencyProofMissing);
                continue;
            };
            let Some(validated) = self.validated.get(&reference.receipt_id) else {
                add(result, Status::Unknown, Code::DependencyProofInvalid);
                continue;
            };
            result.status = result.status.combine(validated.status);
            for reason in &validated.reasons.reasons {
                let mut reason = reason.clone();
                if reason.target.is_none() {
                    reason.target = Some(reference.target.clone());
                }
                result.reasons.push(reason);
            }
            result.reasons.reasons_total = result.reasons.reasons_total.saturating_add(
                validated
                    .reasons
                    .reasons_total
                    .saturating_sub(validated.reasons.reasons.len() as u64),
            );
            result.reasons.reasons_truncated |= validated.reasons.reasons_truncated;
            if !reference_matches(reference, original, plan_id)
                || original.ended_at_ms > receipt.started_at_ms
                || reference
                    .effective_valid_until_ms
                    .is_some_and(|boundary| receipt.started_at_ms >= boundary)
                || (reference.effective_requires_time_validity
                    && reference.effective_valid_until_ms.is_none())
            {
                add(result, Status::Unknown, Code::DependencyProofInvalid);
            }
            time = time.combine(jig_contract::freshness::EffectiveTimeValidityV1::new(
                validated.effective_valid_until_ms,
                validated.effective_requires_time_validity,
            ));
        }
        if time.effective_valid_until_ms != metadata.effective_valid_until_ms
            || time.effective_requires_time_validity != metadata.effective_requires_time_validity
        {
            add(result, Status::Unknown, Code::DependencyProofInvalid);
        }
        // Expose the verified calculation, not an extended boundary from a
        // malformed parent. Raw receipt fields remain unchanged in read models.
        result.effective_valid_until_ms = time.effective_valid_until_ms;
        result.effective_requires_time_validity = time.effective_requires_time_validity;
    }
}

pub(crate) fn compare_current_identity(
    result: &mut TargetFreshness,
    selected: &TargetReceiptStatus,
    expected: &CollectionResult<TargetIdentityV1>,
) {
    comparison::compare(result, selected, expected);
}

fn complete(
    receipt: &TargetReceiptStatus,
) -> Option<(&TargetIdentityV1, &[DependencyExecutionProofV1])> {
    let TargetFreshnessMetadata::V1(metadata) = receipt.target_freshness.as_ref()? else {
        return None;
    };
    let TargetFreshnessStateV1::Complete {
        identity,
        dependency_execution_proof,
    } = &metadata.state
    else {
        return None;
    };
    Some((identity, dependency_execution_proof))
}

fn reference_matches(
    reference: &DependencyExecutionProofV1,
    original: &TargetReceiptStatus,
    plan_id: Option<&str>,
) -> bool {
    let Some((identity, _)) = complete(original) else {
        return false;
    };
    let Some(TargetFreshnessMetadata::V1(metadata)) = &original.target_freshness else {
        return false;
    };
    reference.receipt_id == original.receipt_id
        && reference.target == original.target
        && original.run_id.as_deref() == Some(reference.run_id.as_str())
        && !reference.run_id.is_empty()
        && Some(reference.plan_id.as_str()) == plan_id
        && original.plan_id.as_deref() == plan_id
        && reference.identity_digest == identity.identity_digest
        && reference.conclusion == RunConclusion::Success
        && original.exit_status == 0
        && reference.effective_valid_until_ms == metadata.effective_valid_until_ms
        && reference.effective_requires_time_validity == metadata.effective_requires_time_validity
}

fn identity_encoding_is_consistent(identity: &TargetIdentityV1) -> bool {
    use super::encoding::IdentityEncoder;
    let mut dependencies =
        IdentityEncoder::new("jig-target-dependencies-v1", identity.contract_epoch);
    dependencies.number(identity.dependencies.len() as u64);
    for dependency in &identity.dependencies {
        dependencies.target(&dependency.target);
        dependencies.text(&dependency.identity_digest);
    }
    let mut complete = IdentityEncoder::new(TARGET_IDENTITY_DOMAIN, identity.contract_epoch);
    complete.target(&identity.target);
    complete.text(&identity.source_digest);
    complete.text(&identity.authority_digest);
    complete.text(&identity.dependency_digest);
    dependencies.finish() == identity.dependency_digest
        && complete.finish() == identity.identity_digest
}

fn bounded_identity(identity: &TargetIdentityV1) -> bool {
    use jig_contract::freshness::{MAX_FRESHNESS_DIAGNOSTIC_BYTES, MAX_FRESHNESS_REASON_PREVIEWS};
    [
        &identity.digest_domain,
        &identity.source_digest,
        &identity.authority_digest,
        &identity.dependency_digest,
        &identity.identity_digest,
        &identity.configuration_digest,
        &identity.runner_digest,
        &identity.invocation_digest,
    ]
    .into_iter()
    .all(|value| value.len() <= 128)
        && identity.source_preview.len() <= MAX_FRESHNESS_REASON_PREVIEWS
        && identity.source_preview.iter().all(|entry| {
            entry.path.len() <= MAX_FRESHNESS_DIAGNOSTIC_BYTES && entry.digest.len() <= 128
        })
        && serde_json::to_vec(&identity.source_preview)
            .is_ok_and(|bytes| bytes.len() <= MAX_FRESHNESS_DIAGNOSTIC_BYTES + 2)
}

pub(crate) fn empty() -> TargetFreshness {
    TargetFreshness {
        status: Status::Fresh,
        reasons: FreshnessReasons::default(),
        recorded_identity: None,
        current_identity: None,
        effective_valid_until_ms: None,
        effective_requires_time_validity: false,
    }
}

pub(crate) fn unknown(reason: FreshnessReason) -> TargetFreshness {
    let mut result = empty();
    result.status = Status::Unknown;
    result.reasons.push(reason);
    result
}

pub(crate) fn unverified(
    receipt: &TargetReceiptStatus,
    reason: FreshnessReason,
    now_ms: u64,
) -> TargetFreshness {
    let mut result = unknown(reason);
    // A failed proof lookup cannot erase a known deadline or a missing
    // required boundary. These constraints do not grant identity validity.
    let own = jig_contract::freshness::EffectiveTimeValidityV1::new(
        receipt.valid_until_ms,
        receipt.requires_time_validity,
    );
    let time = own.combine(
        receipt
            .target_freshness
            .as_ref()
            .map(crate::state::metadata_time)
            .unwrap_or_default(),
    );
    result.effective_valid_until_ms = time.effective_valid_until_ms;
    result.effective_requires_time_validity = time.effective_requires_time_validity;
    match &receipt.target_freshness {
        None => add(&mut result, Status::Unknown, Code::LegacyMetadata),
        Some(TargetFreshnessMetadata::Unsupported(_)) => {
            add(&mut result, Status::Unsupported, Code::UnsupportedAuthority)
        }
        Some(TargetFreshnessMetadata::V1(metadata)) => {
            check_version(
                &mut result,
                metadata.contract_epoch,
                metadata.schema_version,
                TARGET_IDENTITY_DOMAIN,
            );
            if let TargetFreshnessStateV1::Complete { identity, .. } = &metadata.state {
                check_version(
                    &mut result,
                    identity.contract_epoch,
                    identity.schema_version,
                    &identity.digest_domain,
                );
            }
            match &metadata.global_execution_proof {
                GlobalExecutionProofV1::Mutated => {
                    add(&mut result, Status::Unknown, Code::ExecutionMutated)
                }
                GlobalExecutionProofV1::Unchanged {
                    before_source_digest,
                    after_source_digest,
                } if !before_source_digest.is_empty()
                    && before_source_digest == after_source_digest => {}
                _ => add(&mut result, Status::Unknown, Code::CollectionFailed),
            }
        }
    }
    apply_time(&mut result, now_ms);
    result
}

fn add(result: &mut TargetFreshness, status: Status, code: Code) {
    result.status = result.status.combine(status);
    result.reasons.push(FreshnessReason {
        code,
        target: None,
        path: None,
    });
}

pub(crate) fn apply_time(result: &mut TargetFreshness, now_ms: u64) {
    if result
        .effective_valid_until_ms
        .is_some_and(|boundary| now_ms >= boundary)
    {
        add(result, Status::Stale, Code::TimeExpired);
    } else if result.effective_requires_time_validity && result.effective_valid_until_ms.is_none() {
        add(result, Status::Unknown, Code::TimeBoundaryMissing);
    }
}

fn check_version(result: &mut TargetFreshness, epoch: u32, schema: u32, domain: &str) {
    if epoch == TARGET_FRESHNESS_CONTRACT_VERSION
        && schema == TARGET_IDENTITY_SCHEMA_VERSION
        && domain == TARGET_IDENTITY_DOMAIN
    {
        return;
    }
    if (1..TARGET_FRESHNESS_CONTRACT_VERSION).contains(&epoch)
        && schema == TARGET_IDENTITY_SCHEMA_VERSION
        && domain == TARGET_IDENTITY_DOMAIN
    {
        add(result, Status::Stale, Code::AuthorityVersionChanged);
    } else {
        add(result, Status::Unsupported, Code::UnsupportedAuthority);
    }
}
