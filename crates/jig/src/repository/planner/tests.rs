use std::collections::BTreeMap;

use jig_contract::{
    ActionArguments, ActionEffect, ActionIntent, ActionRunner, ActionSpec, ComponentId,
    ComponentSpec, ManifestTool, ProfileId, ProfileSpec, SelectionReason, SourceIdentity, TargetId,
};

use super::{
    MAX_SELECTION_REASONS, PlanRunRequest, PlanningPolicy, plan_run_with_source,
    plan_run_with_source_and_paths,
};
use crate::repository::RepositoryCatalog;

fn fixture() -> RepositoryCatalog {
    let components = [
        ComponentSpec::new(ComponentId::parse("api").unwrap(), "api"),
        ComponentSpec::new(ComponentId::parse("web").unwrap(), "web"),
    ];
    let api_target: TargetId = "api:test".parse().unwrap();
    let web_target: TargetId = "web:test".parse().unwrap();
    let mut api = ActionSpec::new(
        api_target.clone(),
        ActionIntent::Check,
        ActionRunner::command("go_test_command"),
    );
    api.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
    api.inputs.push("api/**/*.go".into());
    let mut web = ActionSpec::new(
        web_target.clone(),
        ActionIntent::Check,
        ActionRunner::command("typescript_test_command"),
    );
    web.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
    web.inputs.push("web/**/*.ts".into());
    let profile_id = ProfileId::parse("verify").unwrap();
    let profile = ProfileSpec::new(profile_id.clone(), vec![api_target, web_target]);
    RepositoryCatalog::from_native(
        6,
        "sha256:config",
        &components,
        &[api, web],
        &[profile],
        Some(&profile_id),
    )
    .unwrap()
}

fn source() -> SourceIdentity {
    SourceIdentity::new(Some("abc123".into()), "worktree")
}

#[test]
fn unqualified_action_selects_each_component_deterministically() {
    let plan = plan_run_with_source(
        &fixture(),
        PlanRunRequest {
            selectors: vec!["test".into()],
            ..PlanRunRequest::default()
        },
        source(),
    )
    .unwrap();
    assert_eq!(
        plan.targets
            .iter()
            .map(|target| target.target.to_string())
            .collect::<Vec<_>>(),
        ["api:test", "web:test"]
    );
    assert!(plan.targets.iter().all(|target| {
        target.reasons
            == [SelectionReason::Explicit {
                selector: "test".into(),
            }]
    }));
}

#[test]
fn bare_selection_uses_the_default_profile() {
    let plan = plan_run_with_source(&fixture(), PlanRunRequest::default(), source()).unwrap();
    assert_eq!(plan.profile.unwrap().as_str(), "verify");
    assert_eq!(plan.targets.len(), 2);
}

#[test]
fn exact_and_wildcard_selectors_share_one_resolver() {
    let exact = plan_run_with_source(
        &fixture(),
        PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..PlanRunRequest::default()
        },
        source(),
    )
    .unwrap();
    let wildcard = plan_run_with_source(
        &fixture(),
        PlanRunRequest {
            selectors: vec!["*:test".into()],
            ..PlanRunRequest::default()
        },
        source(),
    )
    .unwrap();
    assert_eq!(exact.targets.len(), 1);
    assert_eq!(wildcard.targets.len(), 2);
}

#[test]
fn canonical_shaped_legacy_alias_falls_back_when_it_matches_no_target() {
    let tools = [
        ManifestTool::new("api:test", "command", "Run legacy API tests.")
            .with_command("api_test_command"),
    ];
    let checks = vec!["api:test".to_owned()];
    let catalog = RepositoryCatalog::from_legacy(5, "sha256:config", &tools, &checks).unwrap();

    let plan = plan_run_with_source(
        &catalog,
        PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..PlanRunRequest::default()
        },
        source(),
    )
    .unwrap();

    assert_eq!(plan.targets.len(), 1);
    assert_eq!(
        plan.targets[0].reasons,
        [SelectionReason::LegacyAlias {
            alias: "api:test".into(),
        }]
    );
}

#[test]
fn equivalent_selector_order_has_the_same_plan_id() {
    let first = plan_run_with_source(
        &fixture(),
        PlanRunRequest {
            selectors: vec!["web:test".into(), "api:test".into()],
            ..PlanRunRequest::default()
        },
        source(),
    )
    .unwrap();
    let second = plan_run_with_source(
        &fixture(),
        PlanRunRequest {
            selectors: vec!["api:test".into(), "web:test".into()],
            ..PlanRunRequest::default()
        },
        source(),
    )
    .unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(first, second);
}

#[test]
fn action_dependencies_form_separate_execution_layers() {
    let component = ComponentSpec::new(ComponentId::parse("api").unwrap(), "api");
    let lint_target: TargetId = "api:lint".parse().unwrap();
    let test_target: TargetId = "api:test".parse().unwrap();
    let mut lint = ActionSpec::new(
        lint_target.clone(),
        ActionIntent::Check,
        ActionRunner::command("lint_command"),
    );
    lint.effects.push(ActionEffect::ReadOnly);
    let mut test = ActionSpec::new(
        test_target.clone(),
        ActionIntent::Check,
        ActionRunner::command("test_command"),
    );
    test.effects.push(ActionEffect::ReadOnly);
    test.depends_on.push(lint_target.clone());
    let profile_id = ProfileId::parse("verify").unwrap();
    let profile = ProfileSpec::new(profile_id.clone(), vec![test_target.clone()]);
    let catalog = RepositoryCatalog::from_native(
        6,
        "digest",
        &[component],
        &[lint, test],
        &[profile],
        Some(&profile_id),
    )
    .unwrap();

    let plan = plan_run_with_source(&catalog, PlanRunRequest::default(), source()).unwrap();
    assert_eq!(
        plan.execution_layers,
        [vec![lint_target], vec![test_target]]
    );
}

#[test]
fn affected_plan_filters_candidates_before_expanding_action_dependencies() {
    let components = [
        ComponentSpec::new(ComponentId::parse("api").unwrap(), "api"),
        ComponentSpec::new(ComponentId::parse("web").unwrap(), "web"),
    ];
    let lint_target: TargetId = "api:lint".parse().unwrap();
    let api_target: TargetId = "api:test".parse().unwrap();
    let web_target: TargetId = "web:test".parse().unwrap();
    let mut lint = ActionSpec::new(
        lint_target.clone(),
        ActionIntent::Check,
        ActionRunner::command("lint_command"),
    );
    lint.effects.push(ActionEffect::ReadOnly);
    lint.inputs.push("api/**/*.go".into());
    let mut api = ActionSpec::new(
        api_target.clone(),
        ActionIntent::Check,
        ActionRunner::command("api_test_command"),
    );
    api.effects.push(ActionEffect::ReadOnly);
    api.inputs.push("api/**/*.go".into());
    api.depends_on.push(lint_target.clone());
    let mut web = ActionSpec::new(
        web_target.clone(),
        ActionIntent::Check,
        ActionRunner::command("web_test_command"),
    );
    web.effects.push(ActionEffect::ReadOnly);
    web.inputs.push("web/**/*.ts".into());
    let profile_id = ProfileId::parse("verify").unwrap();
    let profile = ProfileSpec::new(profile_id.clone(), vec![api_target.clone(), web_target]);
    let catalog = RepositoryCatalog::from_native(
        6,
        "digest",
        &components,
        &[lint, api, web],
        &[profile],
        Some(&profile_id),
    )
    .unwrap();

    let plan = plan_run_with_source_and_paths(
        &catalog,
        PlanRunRequest {
            affected_base: Some(" main ".into()),
            ..PlanRunRequest::default()
        },
        source(),
        Some(&["api/main.go".into()]),
        &[],
        BTreeMap::new(),
        PlanningPolicy::ChecksOnly,
    )
    .unwrap();

    assert_eq!(plan.affected_base.as_deref(), Some("main"));
    assert_eq!(
        plan.targets
            .iter()
            .map(|target| target.target.to_string())
            .collect::<Vec<_>>(),
        ["api:lint", "api:test"]
    );
    assert_eq!(
        plan.execution_layers,
        [vec![lint_target], vec![api_target.clone()]]
    );
    assert_eq!(
        plan.targets[0].reasons,
        [SelectionReason::ActionDependency { target: api_target }]
    );
    assert_eq!(
        plan.targets[1].reasons,
        [
            SelectionReason::Profile {
                profile: profile_id,
            },
            SelectionReason::DirectInput {
                path: "api/main.go".into(),
            },
        ]
    );
}

#[test]
fn affected_plan_allows_a_deterministic_empty_selection() {
    let plan = plan_run_with_source_and_paths(
        &fixture(),
        PlanRunRequest {
            affected_base: Some("HEAD~1".into()),
            ..PlanRunRequest::default()
        },
        source(),
        Some(&[]),
        &[],
        BTreeMap::new(),
        PlanningPolicy::ChecksOnly,
    )
    .unwrap();

    assert!(plan.targets.is_empty());
    assert!(plan.execution_layers.is_empty());
    assert!(plan.effects.is_empty());
    assert_eq!(plan.affected_base.as_deref(), Some("HEAD~1"));
}

#[test]
fn affected_plan_bounds_reasons_with_deterministic_completeness_metadata() {
    let paths = (0..250)
        .map(|index| format!("api/generated/{index:03}.go"))
        .collect::<Vec<_>>();
    let mut reversed = paths.clone();
    reversed.reverse();
    let request = PlanRunRequest {
        affected_base: Some("main".into()),
        ..PlanRunRequest::default()
    };

    let first = plan_run_with_source_and_paths(
        &fixture(),
        request.clone(),
        source(),
        Some(&paths),
        &[],
        BTreeMap::new(),
        PlanningPolicy::ChecksOnly,
    )
    .unwrap();
    let second = plan_run_with_source_and_paths(
        &fixture(),
        request,
        source(),
        Some(&reversed),
        &[],
        BTreeMap::new(),
        PlanningPolicy::ChecksOnly,
    )
    .unwrap();

    assert_eq!(first, second);
    let target = &first.targets[0];
    assert_eq!(target.reasons.len(), MAX_SELECTION_REASONS);
    assert_eq!(target.selection_reason_count, Some(251));
    assert!(target.selection_reasons_truncated);
    assert!(
        target
            .selection_reasons_digest
            .as_deref()
            .is_some_and(|digest| digest.starts_with("sha256:"))
    );
    assert!(matches!(target.reasons[0], SelectionReason::Profile { .. }));
    assert!(serde_json::to_vec(&first).unwrap().len() < 20_000);
}

#[test]
fn check_rejects_effectful_actions() {
    let component = ComponentSpec::new(ComponentId::parse("api").unwrap(), "api");
    let target: TargetId = "api:generate".parse().unwrap();
    let mut action = ActionSpec::new(
        target.clone(),
        ActionIntent::Generate,
        ActionRunner::command("generate_command"),
    );
    action.effects.push(ActionEffect::Worktree);
    let profile_id = ProfileId::parse("verify").unwrap();
    let profile = ProfileSpec::new(profile_id.clone(), vec![target]);
    let catalog = RepositoryCatalog::from_native(
        6,
        "digest",
        &[component],
        &[action],
        &[profile],
        Some(&profile_id),
    )
    .unwrap();

    let error = plan_run_with_source(&catalog, PlanRunRequest::default(), source())
        .unwrap_err()
        .to_string();
    assert!(error.contains("not a read-only check"));
}

#[test]
fn action_plans_bind_required_native_arguments_into_the_digest() {
    let component = ComponentSpec {
        adapters: vec!["rust".into(), "sqlx".into()],
        ..ComponentSpec::new(ComponentId::parse("api").unwrap(), "api")
    };
    let target: TargetId = "api:migration-add".parse().unwrap();
    let mut action = ActionSpec::new(
        target.clone(),
        ActionIntent::Generate,
        ActionRunner::native(jig_contract::tool::MIGRATION_ADD),
    );
    action.effects = vec![ActionEffect::Worktree, ActionEffect::Process];
    let profile_id = ProfileId::parse("verify").unwrap();
    let profile = ProfileSpec::new(profile_id.clone(), vec![target.clone()]);
    let catalog = RepositoryCatalog::from_native(
        6,
        "digest",
        &[component],
        &[action],
        &[profile],
        Some(&profile_id),
    )
    .unwrap();
    let request = PlanRunRequest {
        selectors: vec![target.to_string()],
        ..PlanRunRequest::default()
    };

    let missing = plan_run_with_source_and_paths(
        &catalog,
        request.clone(),
        source(),
        None,
        &[],
        BTreeMap::new(),
        PlanningPolicy::DeclaredActions,
    )
    .unwrap_err()
    .to_string();
    assert!(missing.contains("requires string argument 'name'"));

    let arguments = BTreeMap::from([(
        target,
        ActionArguments {
            name: Some("create_examples".into()),
        },
    )]);
    let plan = plan_run_with_source_and_paths(
        &catalog,
        request,
        source(),
        None,
        &[],
        arguments,
        PlanningPolicy::DeclaredActions,
    )
    .unwrap();

    assert_eq!(
        plan.targets[0].arguments.name.as_deref(),
        Some("create_examples")
    );
    assert!(plan.id.starts_with("run-plan_sha256:"));
}

#[test]
fn action_dependency_cycles_are_reported_with_the_targets() {
    let component = ComponentSpec::new(ComponentId::parse("api").unwrap(), "api");
    let lint_target: TargetId = "api:lint".parse().unwrap();
    let test_target: TargetId = "api:test".parse().unwrap();
    let mut lint = ActionSpec::new(
        lint_target.clone(),
        ActionIntent::Check,
        ActionRunner::command("lint_command"),
    );
    lint.effects.push(ActionEffect::ReadOnly);
    lint.depends_on.push(test_target.clone());
    let mut test = ActionSpec::new(
        test_target.clone(),
        ActionIntent::Check,
        ActionRunner::command("test_command"),
    );
    test.effects.push(ActionEffect::ReadOnly);
    test.depends_on.push(lint_target.clone());
    let profile_id = ProfileId::parse("verify").unwrap();
    let profile = ProfileSpec::new(profile_id.clone(), vec![test_target.clone()]);
    let mut catalog_lint = lint.clone();
    catalog_lint.depends_on.clear();
    let mut catalog_test = test.clone();
    catalog_test.depends_on.clear();
    let mut catalog = RepositoryCatalog::from_native(
        6,
        "digest",
        &[component],
        &[catalog_lint, catalog_test],
        &[profile],
        Some(&profile_id),
    )
    .unwrap();
    catalog
        .actions
        .get_mut(&lint_target)
        .unwrap()
        .depends_on
        .clone_from(&lint.depends_on);
    catalog
        .actions
        .get_mut(&test_target)
        .unwrap()
        .depends_on
        .clone_from(&test.depends_on);

    let error = plan_run_with_source(&catalog, PlanRunRequest::default(), source())
        .unwrap_err()
        .to_string();
    assert!(error.contains("action dependency cycle among: api:lint, api:test"));
}
