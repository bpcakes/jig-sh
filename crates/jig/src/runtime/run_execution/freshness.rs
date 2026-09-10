use std::cell::RefCell;
use std::sync::TryLockError;

use jig_contract::freshness::{
    DependencyExecutionProofV1, EffectiveTimeValidityV1, FreshnessReason,
    FreshnessReasonCode as Code, FreshnessReasons, GlobalExecutionProofV1,
    TARGET_FRESHNESS_CONTRACT_VERSION, TargetFreshnessMetadata, TargetFreshnessStateV1,
    TargetFreshnessV1,
};

use super::*;
use crate::repository::freshness::{
    CollectionBudget, CollectionFailure, CollectionLimits, CollectionResult,
    ExecutionAuthorityGuard, RECORDING_TIMEOUT_MS, TargetIdentityCollection,
    collect_target_identities,
};

pub(super) struct ExecutionFreshness {
    collected: Mutex<CollectionResult<TargetIdentityCollection>>,
    completed: Mutex<BTreeMap<TargetId, CompletedProof>>,
}

struct CompletedProof {
    reference: DependencyExecutionProofV1,
    ended_at_ms: u64,
}

impl ExecutionFreshness {
    pub(super) fn prepare(
        ctx: &RepoContext,
        catalog: &RepositoryCatalog,
        run: &crate::state::DurableRun,
        control: &mut dyn RepositoryRunControl,
    ) -> Self {
        let control = RefCell::new(control);
        let cancelled = || control.borrow_mut().cancelled().unwrap_or(true);
        let mut budget = CollectionBudget::new(
            CollectionLimits::with_timeout(Duration::from_millis(RECORDING_TIMEOUT_MS)),
            &cancelled,
        );
        // Submitted optional proof is never consumed. This collection belongs
        // to the live worker and is reused only under the original global guards.
        let collected = collect_target_identities(
            ctx,
            catalog,
            &run.plan.targets,
            &run.plan.source.worktree_fingerprint,
            &mut budget,
        );
        Self {
            collected: Mutex::new(collected),
            completed: Mutex::new(BTreeMap::new()),
        }
    }

    pub(super) fn before_target(
        &self,
        ctx: &RepoContext,
        catalog: &RepositoryCatalog,
        planned: &PlannedTarget,
        control: &mut dyn RepositoryRunControl,
    ) -> CollectionResult<ExecutionAuthorityGuard> {
        self.observe(control, |collected, budget| {
            if let Some(prepared) = &planned.prepared_native_input {
                crate::repository::revalidate_freshness_native_input(ctx, prepared, budget)?;
            }
            collected.execution_authority(ctx, catalog, planned, budget)
        })
    }

    pub(super) fn after_target(
        &self,
        ctx: &RepoContext,
        catalog: &RepositoryCatalog,
        planned: &PlannedTarget,
        guard: ExecutionAuthorityGuard,
        control: &mut dyn RepositoryRunControl,
    ) -> CollectionResult<()> {
        self.observe(control, |collected, budget| {
            guard.revalidate(ctx, budget)?;
            if let Some(prepared) = &planned.prepared_native_input {
                crate::repository::revalidate_freshness_native_input(ctx, prepared, budget)?;
            }
            // Re-resolve PATH candidates and the bound invocation after cleanup.
            // Full source hashing remains the existing global postcondition.
            collected
                .execution_authority(ctx, catalog, planned, budget)?
                .revalidate(ctx, budget)
        })
    }

    fn observe<T>(
        &self,
        control: &mut dyn RepositoryRunControl,
        collect: impl FnOnce(
            &mut TargetIdentityCollection,
            &mut CollectionBudget<'_>,
        ) -> CollectionResult<T>,
    ) -> CollectionResult<T> {
        let control = RefCell::new(control);
        let cancelled = || control.borrow_mut().cancelled().unwrap_or(true);
        // Bound cancellation-aware lock waits separately. Once acquired, resume
        // the one shared observation allowance; target execution does not spend it.
        let budget = CollectionBudget::new(
            CollectionLimits::with_timeout(Duration::from_millis(RECORDING_TIMEOUT_MS)),
            &cancelled,
        );
        let mut retained = loop {
            budget.ensure_active()?;
            match self.collected.try_lock() {
                Ok(retained) => break retained,
                Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(5)),
                Err(TryLockError::Poisoned(_)) => return Err(failure(Code::CollectionFailed)),
            }
        };
        let collected = retained.as_mut().map_err(|error| error.clone())?;
        let mut budget = CollectionBudget::resume(
            CollectionLimits::with_timeout(Duration::from_millis(RECORDING_TIMEOUT_MS)),
            &cancelled,
            collected.stats.clone(),
        );
        let result = collect(collected, &mut budget);
        collected.stats = budget.finish_stats();
        result
    }

    pub(super) fn metadata(
        &self,
        run: &crate::state::DurableRun,
        planned: &PlannedTarget,
        completed: &CompletedTargetCapture,
        fingerprint: &std::result::Result<String, String>,
    ) -> TargetFreshnessMetadata {
        let mut reasons = FreshnessReasons::default();
        let global_execution_proof =
            if !completed.was_started() || !completed.capture.execution_safety_proved {
                reasons.push(failure(Code::CollectionFailed).reason);
                GlobalExecutionProofV1::Unknown
            } else {
                match fingerprint {
                    Ok(after) if after == &run.plan.source.worktree_fingerprint => {
                        GlobalExecutionProofV1::Unchanged {
                            before_source_digest: run.plan.source.worktree_fingerprint.clone(),
                            after_source_digest: after.clone(),
                        }
                    }
                    Ok(_) => {
                        reasons.push(failure(Code::ExecutionMutated).reason);
                        GlobalExecutionProofV1::Mutated
                    }
                    Err(_) => {
                        reasons.push(failure(Code::CollectionFailed).reason);
                        GlobalExecutionProofV1::Unknown
                    }
                }
            };
        match &completed.capture.freshness_authority {
            Some(Ok(())) => {}
            Some(Err(error)) => reasons.push(error.reason.clone()),
            None => reasons.push(failure(Code::CollectionFailed).reason),
        }
        let identity = match self.collected.lock() {
            Ok(collected) => match collected.as_ref() {
                Ok(collected) => collected
                    .targets
                    .get(&planned.target)
                    .cloned()
                    .unwrap_or_else(|| Err(failure(Code::CollectionFailed))),
                Err(error) => Err(error.clone()),
            },
            Err(_) => Err(failure(Code::CollectionFailed)),
        };
        if let Err(error) = &identity {
            reasons.push(error.reason.clone());
        }
        let own_requires = completed
            .capture
            .native_evidence
            .as_ref()
            .is_some_and(crate::state::evidence_requires_time_validity);
        let mut time = EffectiveTimeValidityV1::new(completed.capture.valid_until_ms, own_requires);
        if own_requires && completed.capture.valid_until_ms.is_none() {
            reasons.push(failure(Code::TimeBoundaryMissing).reason);
        }
        let mut references = Vec::new();
        let mut dependencies = planned.depends_on.clone();
        dependencies.sort();
        dependencies.dedup();
        match self.completed.lock() {
            Ok(proofs) => {
                for dependency in dependencies {
                    let Some(proof) = proofs.get(&dependency) else {
                        reasons.push(FreshnessReason {
                            code: Code::DependencyProofMissing,
                            target: Some(dependency),
                            path: None,
                        });
                        continue;
                    };
                    let reference = &proof.reference;
                    let expected = identity.as_ref().ok().and_then(|identity| {
                        identity
                            .dependencies
                            .iter()
                            .find(|entry| entry.target == dependency)
                    });
                    if reference.plan_id.is_empty()
                        || run.work_plan_id.as_deref() != Some(reference.plan_id.as_str())
                        || expected.is_none_or(|expected| {
                            expected.identity_digest != reference.identity_digest
                        })
                        || completed.started_at_ms.is_none_or(|started| {
                            proof.ended_at_ms > started
                                || reference
                                    .effective_valid_until_ms
                                    .is_some_and(|boundary| started >= boundary)
                        })
                        || (reference.effective_requires_time_validity
                            && reference.effective_valid_until_ms.is_none())
                    {
                        reasons.push(FreshnessReason {
                            code: Code::DependencyProofInvalid,
                            target: Some(dependency),
                            path: None,
                        });
                    }
                    time = time.combine(EffectiveTimeValidityV1::new(
                        reference.effective_valid_until_ms,
                        reference.effective_requires_time_validity,
                    ));
                    references.push(reference.clone());
                }
            }
            Err(_) => reasons.push(failure(Code::DependencyProofInvalid).reason),
        }
        let state = if reasons.reasons_total == 0 {
            TargetFreshnessStateV1::Complete {
                identity: Box::new(identity.expect("complete collection has no failure reason")),
                dependency_execution_proof: references,
            }
        } else {
            TargetFreshnessStateV1::Incomplete { reasons }
        };
        TargetFreshnessMetadata::V1(Box::new(TargetFreshnessV1 {
            schema_version: 1,
            contract_epoch: TARGET_FRESHNESS_CONTRACT_VERSION,
            effective_valid_until_ms: time.effective_valid_until_ms,
            effective_requires_time_validity: time.effective_requires_time_validity,
            global_execution_proof,
            state,
        }))
    }

    pub(super) fn recorded(
        &self,
        run: &crate::state::DurableRun,
        planned: &PlannedTarget,
        receipt_id: &str,
        ended_at_ms: u64,
        conclusion: RunConclusion,
        metadata: &TargetFreshnessMetadata,
    ) {
        if conclusion != RunConclusion::Success {
            return;
        }
        let TargetFreshnessMetadata::V1(metadata) = metadata else {
            return;
        };
        let TargetFreshnessStateV1::Complete { identity, .. } = &metadata.state else {
            return;
        };
        if let Ok(mut proofs) = self.completed.lock() {
            proofs.insert(
                planned.target.clone(),
                CompletedProof {
                    reference: DependencyExecutionProofV1 {
                        target: planned.target.clone(),
                        receipt_id: receipt_id.into(),
                        run_id: run.result.run_id.clone(),
                        plan_id: run.work_plan_id.clone().unwrap_or_default(),
                        identity_digest: identity.identity_digest.clone(),
                        conclusion,
                        effective_valid_until_ms: metadata.effective_valid_until_ms,
                        effective_requires_time_validity: metadata.effective_requires_time_validity,
                    },
                    ended_at_ms,
                },
            );
        }
    }
}

fn failure(code: Code) -> CollectionFailure {
    CollectionFailure::new(
        code,
        "execution could not establish complete target freshness authority",
    )
}

pub(super) fn run_target_capture(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    run_id: &str,
    work_plan_id: Option<&str>,
    planned: &PlannedTarget,
    run_control: &mut dyn RepositoryRunControl,
    freshness: Option<&freshness::ExecutionFreshness>,
) -> TargetCapture {
    let authority =
        freshness.map(|freshness| freshness.before_target(ctx, catalog, planned, run_control));
    let authority_started_at_ms = freshness.map(|_| now_ms());
    let mut capture =
        run_target_capture_inner(ctx, catalog, run_id, work_plan_id, planned, run_control);
    if let (Some(freshness), Some(authority)) = (freshness, authority) {
        capture.freshness_authority =
            Some(authority.and_then(|guard| {
                freshness.after_target(ctx, catalog, planned, guard, run_control)
            }));
        capture.authority_started_at_ms = authority_started_at_ms;
    }
    capture
}
