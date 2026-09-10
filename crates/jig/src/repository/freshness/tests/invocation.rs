use super::*;
use jig_contract::{
    ComparisonPreparationV1, PolicyPreparationV1, PreparedNativeInputV1, ResolvedComparisonV1,
};

fn collect(
    fixture: &Fixture,
    invocations: &[PlannedTarget],
) -> CollectionResult<TargetIdentityCollection> {
    let ctx = fixture.context();
    let catalog = fixture.catalog(&ctx);
    let mut budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_secs(30)),
        &|| false,
    );
    collect_target_identities(
        &ctx,
        &catalog,
        invocations,
        "unused-whole-repository-token",
        &mut budget,
    )
}

#[test]
fn native_policy_comparison_and_configuration_are_pinned_separately_from_source() {
    let mut fixture = Fixture::new();
    fixture.actions[0].runner = ActionRunner::native(jig_contract::tool::FILE_BUDGET);
    fixture.actions[0].inputs = vec!["**".into()];
    fixture.write_authority();
    let mut invocations = fixture.invocations();
    let target = invocations[0].target.clone();
    assert!(collect(&fixture, &invocations).unwrap().targets[&target].is_err());
    invocations[0].prepared_native_input = Some(PreparedNativeInputV1 {
        schema_version: 1,
        view: jig_contract::CurrentViewV1::Worktree,
        request: jig_contract::ComparisonRequestV1::ExactTree {
            requested_oid: "1".repeat(40),
            provenance: jig_contract::ExactTreeProvenanceV1::WorkPlan,
        },
        configuration: jig_contract::NativeFileBudgetConfigV1::default(),
        policy_source: jig_contract::PolicySourceV1 {
            path: ".jig/file-budget.toml".into(),
        },
        work_plan_id: Some("plan_example".into()),
        policy: PolicyPreparationV1::Ready {
            policy_raw_digest: "sha256:example-raw".into(),
            policy_semantic_digest: "sha256:example-policy".into(),
        },
        comparison: ComparisonPreparationV1::Ready {
            comparison: ResolvedComparisonV1::ExactTree {
                requested_oid: "1".repeat(40),
                peeled_commit_oid: None,
                tree_oid: "2".repeat(40),
                provenance: jig_contract::ExactTreeProvenanceV1::WorkPlan,
            },
        },
    });
    let identity = |invocations: &[PlannedTarget]| {
        collect(&fixture, invocations)
            .unwrap()
            .targets
            .remove(&target)
            .unwrap()
            .unwrap()
    };
    let baseline = identity(&invocations);
    for changed in 0..4 {
        let mut modified = invocations.clone();
        let native = modified[0].prepared_native_input.as_mut().unwrap();
        match changed {
            0 => {
                let PolicyPreparationV1::Ready {
                    policy_semantic_digest,
                    ..
                } = &mut native.policy
                else {
                    unreachable!()
                };
                *policy_semantic_digest = "sha256:changed-policy".into();
            }
            1 => {
                let ComparisonPreparationV1::Ready {
                    comparison: ResolvedComparisonV1::ExactTree { tree_oid, .. },
                } = &mut native.comparison
                else {
                    unreachable!()
                };
                *tree_oid = "3".repeat(40);
            }
            2 => native.work_plan_id = Some("plan_another_example".into()),
            _ => native.view = jig_contract::CurrentViewV1::Index,
        }
        let changed = identity(&modified);
        assert_ne!(baseline.runner_digest, changed.runner_digest);
        assert_ne!(baseline.identity_digest, changed.identity_digest);
        assert_eq!(baseline.source_digest, changed.source_digest);
    }
    invocations[0]
        .prepared_native_input
        .as_mut()
        .unwrap()
        .schema_version = 99;
    assert!(collect(&fixture, &invocations).unwrap().targets[&target].is_err());
}

#[test]
fn missing_or_ambiguous_bound_dependency_invocations_are_not_guessed() {
    let fixture = Fixture::new();
    let mut invocations = fixture.invocations();
    let duplicate = invocations[0].clone();
    invocations.push(duplicate);
    assert_eq!(
        collect(&fixture, &invocations).err().unwrap().reason.code,
        FreshnessReasonCode::UnsupportedAuthority
    );
    invocations.truncate(1);
    assert!(
        collect(&fixture, &invocations)
            .unwrap()
            .targets
            .values()
            .all(Result::is_err)
    );
}

#[test]
fn effective_cwd_and_explicit_environment_are_runner_authority() {
    let mut fixture = Fixture::new();
    let before = fixture.identity("web:test");
    let ActionRunner::Argv {
        working_directory, ..
    } = &mut fixture.actions[0].runner
    else {
        unreachable!()
    };
    *working_directory = Some(".".into());
    fixture.write_authority();
    let normalized = fixture.identity("web:test");
    assert_eq!(before.runner_digest, normalized.runner_digest);
    let ActionRunner::Argv { environment, .. } = &mut fixture.actions[0].runner else {
        unreachable!()
    };
    environment.insert("EXAMPLE_MODE".into(), "strict".into());
    fixture.write_authority();
    assert_ne!(
        normalized.runner_digest,
        fixture.identity("web:test").runner_digest
    );
}
