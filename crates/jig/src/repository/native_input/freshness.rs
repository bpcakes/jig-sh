use super::*;
use crate::repository::freshness::{CollectionBudget, CollectionFailure, CollectionResult};
use crate::state::PlanBaseline;
use jig_contract::freshness::FreshnessReasonCode;

/// A gate uses its durable exact work-plan baseline and the normal native
/// policy/comparison preparation. It never borrows a receipt's explicit request.
pub(crate) fn prepare_gate_file_budget_input(
    ctx: &RepoContext,
    configuration: NativeFileBudgetConfigV1,
    plan_id: &str,
    baseline: Option<&PlanBaseline>,
    budget: &mut CollectionBudget<'_>,
) -> CollectionResult<PreparedNativeInputV1> {
    budget.ensure_active()?;
    let baseline = baseline
        .filter(|baseline| baseline.error.is_none())
        .ok_or_else(unavailable)?;
    let oid = baseline
        .commit_oid
        .as_ref()
        .or(baseline.empty_tree_oid.as_ref())
        .ok_or_else(unavailable)?;
    let request = ComparisonRequestV1::ExactTree {
        requested_oid: oid.clone(),
        provenance: jig_contract::ExactTreeProvenanceV1::WorkPlan,
    };
    prepare_observed_input(ctx, request, configuration, Some(plan_id.into()), budget)
}

pub(crate) fn revalidate_freshness_native_input(
    ctx: &RepoContext,
    prepared: &PreparedNativeInputV1,
    budget: &mut CollectionBudget<'_>,
) -> CollectionResult<()> {
    let mut current = prepare_observed_input(
        ctx,
        prepared.request.clone(),
        prepared.configuration.clone(),
        prepared.work_plan_id.clone(),
        budget,
    )?;
    retain_push_before_fetch_provenance(prepared, &mut current);
    if &current != prepared {
        return Err(CollectionFailure::new(
            FreshnessReasonCode::SourceRaced,
            "prepared native policy or comparison authority changed around execution",
        ));
    }
    Ok(())
}

fn retain_push_before_fetch_provenance(
    prepared: &PreparedNativeInputV1,
    current: &mut PreparedNativeInputV1,
) {
    if !matches!(
        prepared.request,
        ComparisonRequestV1::ExactTree {
            provenance: jig_contract::ExactTreeProvenanceV1::PushBefore,
            ..
        }
    ) {
        return;
    }
    let fallback = |comparison: &ComparisonPreparationV1| match comparison {
        ComparisonPreparationV1::Ready {
            comparison:
                ResolvedComparisonV1::StrictInventory {
                    reason: StrictInventoryReasonV1::MissingComparisonFallback,
                    fallback_from: Some(fallback),
                },
        } => Some(fallback.clone()),
        _ => None,
    };
    let (Some(original), Some(observed)) = (
        fallback(&prepared.comparison),
        fallback(&current.comparison),
    ) else {
        return;
    };
    let expected_digest = digest_json(
        b"jig-file-budget-comparison-failure-v1\0",
        &(
            &prepared.request,
            &original.failure.code,
            &original.failure.message,
            &original.attempted_object_ids,
        ),
    );
    if original.original_request == prepared.request
        && original.original_request == observed.original_request
        && original.attempted_object_ids == observed.attempted_object_ids
        && original.failure.code == observed.failure.code
        && original.failure.failure_digest == expected_digest
        && original.failure_digest == expected_digest
    {
        // The one fetch attempt is a historical preparation fact. Inspection
        // never repeats network work. Preserve its intact diagnostic authority
        // only while the exact object remains unavailable locally and the same
        // explicit fallback still applies. A now-resolvable object fails above.
        current.comparison.clone_from(&prepared.comparison);
    }
}

fn prepare_observed_input(
    ctx: &RepoContext,
    request: ComparisonRequestV1,
    configuration: NativeFileBudgetConfigV1,
    work_plan_id: Option<String>,
    budget: &mut CollectionBudget<'_>,
) -> CollectionResult<PreparedNativeInputV1> {
    budget.ensure_active()?;
    let view = current_view(&request);
    let now = time::OffsetDateTime::now_utc().date();
    let current_date = PolicyDateV1::new(now.year() as u16, now.month() as u8, now.day())
        .map_err(|_| unavailable())?;
    let bytes = if view == CurrentViewV1::Index {
        let observed = std::cell::Cell::new(0_u64);
        let cancelled = || {
            budget.stopped()
                || observed
                    .get()
                    .saturating_add(budget.stats.content_bytes_read)
                    > budget.limits.bytes
        };
        let bytes = crate::git_receipts::read_index_blob_v1_observed(
            ctx.root(),
            POLICY_PATH_V1,
            MAX_POLICY_BYTES_V1 + 1,
            &cancelled,
            &observed,
        );
        budget.bytes(observed.get())?;
        bytes
            .map_err(|_| "native index policy could not be observed".to_owned())
            .and_then(super::policy::validate_index_policy_bytes)
    } else {
        Ok(crate::repository::freshness::read_native_authority_bytes(
            ctx.root(),
            POLICY_PATH_V1,
            MAX_POLICY_BYTES_V1,
            budget,
        )?)
    };
    budget.entries(1)?;
    let policy = prepare_policy_from_bytes(bytes, current_date);
    let observed_bytes = std::cell::Cell::new(0_u64);
    let cancelled = || {
        budget.stopped()
            || observed_bytes
                .get()
                .saturating_add(budget.stats.content_bytes_read)
                > budget.limits.bytes
    };
    let resolved = crate::git_receipts::resolve_comparison_v1_observed(
        ctx.root(),
        request.clone(),
        &cancelled,
        &observed_bytes,
    );
    budget.bytes(observed_bytes.get())?;
    budget.ensure_active()?;
    let comparison =
        comparison_preparation(ctx, &request, configuration.missing_comparison, resolved);
    Ok(PreparedNativeInputV1 {
        schema_version: PreparedNativeInputV1::SCHEMA_VERSION,
        view,
        request,
        configuration,
        policy_source: PolicySourceV1 {
            path: POLICY_PATH_V1.into(),
        },
        work_plan_id,
        policy,
        comparison,
    })
}

fn unavailable() -> CollectionFailure {
    CollectionFailure::new(
        FreshnessReasonCode::CollectionFailed,
        "the gate's native work-plan authority could not be prepared",
    )
}
