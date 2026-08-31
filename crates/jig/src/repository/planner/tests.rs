use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use jig_contract::{
    ActionArguments, ActionEffect, ActionIntent, ActionRunner, ActionSpec, ComponentId,
    ComponentSpec, ManifestTool, PlannedTarget, ProfileId, ProfileSpec, RunPlan, SelectionReason,
    SourceIdentity, TargetId,
};
use serde_json::json;
use tempfile::{TempDir, tempdir};

use super::{
    MAX_SELECTION_REASONS, PlanRunRequest, PlanningPolicy, plan_run_with_source,
    plan_run_with_source_and_paths,
};
use crate::context::RepoContext;
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

fn git(root: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn v7_file_budget_repository() -> (TempDir, RepoContext, RepositoryCatalog) {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::create_dir_all(temp.path().join(".jig")).unwrap();
    fs::create_dir_all(temp.path().join("docs")).unwrap();
    fs::write(temp.path().join("docs/example.md"), "# Example\n").unwrap();
    fs::write(temp.path().join("source.rs"), "fn example() {}\n").unwrap();
    fs::write(
        temp.path().join(".jig/file-budget.toml"),
        "version=1\n[[rules]]\nid=\"source\"\ninclude=[\"**\"]\nmax_lines=100\n",
    )
    .unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "ExampleProject"
default_branch = "main"

[commands]
docs_command = "printf 'docs ok\n'"

[repository]
default_check_profile = "verify"

[[repository.components]]
id = "repo"
root = "."

[[repository.actions]]
target = { component = "repo", action = "file-budget" }
intent = "check"
effects = ["read_only"]
runner = { kind = "native", operation = "jig.file_budget" }
inputs = ["**"]

[[repository.actions]]
target = { component = "repo", action = "docs" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "docs_command" }
inputs = ["docs/**"]

[[repository.profiles]]
id = "verify"
targets = [
  { component = "repo", action = "file-budget" },
  { component = "repo", action = "docs" },
]
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 7,
            "tool_namespace": "jig",
            "required_commands": ["docs_command"],
            "tools": [],
            "components": [{"id": "repo", "root": "."}],
            "actions": [
                {
                    "target": {"component": "repo", "action": "file-budget"},
                    "intent": "check",
                    "effects": ["read_only"],
                    "runner": {"kind": "native", "operation": "jig.file_budget"},
                    "inputs": ["**"]
                },
                {
                    "target": {"component": "repo", "action": "docs"},
                    "intent": "check",
                    "effects": ["read_only", "process"],
                    "runner": {"kind": "command", "command": "docs_command"},
                    "inputs": ["docs/**"]
                }
            ],
            "profiles": [{
                "id": "verify",
                "targets": [
                    {"component": "repo", "action": "file-budget"},
                    {"component": "repo", "action": "docs"}
                ]
            }],
            "default_check_profile": "verify"
        }))
        .unwrap(),
    )
    .unwrap();
    git(temp.path(), &["init", "-q", "-b", "main"]);
    git(temp.path(), &["config", "user.name", "Jig Test"]);
    git(
        temp.path(),
        &["config", "user.email", "jig@example.invalid"],
    );
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-q", "-m", "fixture"]);
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
    let catalog = RepositoryCatalog::from_context(&ctx).unwrap();
    (temp, ctx, catalog)
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

fn prepared_file_budget_target(plan: &RunPlan) -> &PlannedTarget {
    plan.targets
        .iter()
        .find(|target| target.target.to_string() == "repo:file-budget")
        .expect("fixture plan contains file-budget target")
}

fn reidentify(plan: &mut RunPlan) {
    plan.id = super::plan_digest(plan).unwrap();
}

#[test]
fn v7_planning_is_lazy_and_persists_bounded_native_authority() {
    let (_temp, ctx, catalog) = v7_file_budget_repository();
    let plan = super::plan_run(&ctx, &catalog, PlanRunRequest::default()).unwrap();
    let prepared = prepared_file_budget_target(&plan)
        .prepared_native_input
        .as_ref()
        .unwrap();

    assert_eq!(prepared.schema_version, 1);
    assert_eq!(prepared.policy_source.path, ".jig/file-budget.toml");
    assert!(matches!(
        prepared.policy,
        jig_contract::PolicyPreparationV1::Ready { .. }
    ));
    assert!(matches!(
        prepared.comparison,
        jig_contract::ComparisonPreparationV1::Ready {
            comparison: jig_contract::ResolvedComparisonV1::MergeBase { .. }
        }
    ));

    fs::remove_file(ctx.root().join(".jig/file-budget.toml")).unwrap();
    let docs_only = super::plan_run(
        &ctx,
        &RepositoryCatalog::from_context(&ctx).unwrap(),
        PlanRunRequest {
            selectors: vec!["repo:docs".into()],
            ..PlanRunRequest::default()
        },
    )
    .unwrap();
    assert_eq!(docs_only.targets.len(), 1);
    assert!(docs_only.targets[0].prepared_native_input.is_none());
}

#[test]
fn untrusted_prepared_native_authority_is_replayed_exactly() {
    let (_temp, ctx, catalog) = v7_file_budget_repository();
    let plan = super::plan_run(&ctx, &catalog, PlanRunRequest::default()).unwrap();

    let mut altered_tree = plan.clone();
    let prepared = altered_tree
        .targets
        .iter_mut()
        .find_map(|target| target.prepared_native_input.as_mut())
        .unwrap();
    let jig_contract::ComparisonPreparationV1::Ready { comparison } = &mut prepared.comparison
    else {
        panic!("expected ready comparison");
    };
    let jig_contract::ResolvedComparisonV1::MergeBase { merge_base_oid, .. } = comparison else {
        panic!("expected merge-base comparison");
    };
    *merge_base_oid = "0".repeat(40);
    reidentify(&mut altered_tree);
    let error = super::validate_run_plan(&ctx, &catalog, &altered_tree)
        .unwrap_err()
        .to_string();
    assert!(error.contains("stale"), "{error}");

    let mut altered_policy = plan.clone();
    let prepared = altered_policy
        .targets
        .iter_mut()
        .find_map(|target| target.prepared_native_input.as_mut())
        .unwrap();
    let jig_contract::PolicyPreparationV1::Ready {
        policy_semantic_digest,
        ..
    } = &mut prepared.policy
    else {
        panic!("expected ready policy");
    };
    *policy_semantic_digest = "sha256:forged".into();
    reidentify(&mut altered_policy);
    let error = super::validate_run_plan(&ctx, &catalog, &altered_policy)
        .unwrap_err()
        .to_string();
    assert!(error.contains("stale"), "{error}");

    let mut altered_configuration = plan;
    let prepared = altered_configuration
        .targets
        .iter_mut()
        .find_map(|target| target.prepared_native_input.as_mut())
        .unwrap();
    prepared.configuration.max_candidates += 1;
    reidentify(&mut altered_configuration);
    let error = super::validate_run_plan(&ctx, &catalog, &altered_configuration)
        .unwrap_err()
        .to_string();
    assert!(error.contains("stale"), "{error}");
}

#[test]
fn policy_bytes_are_part_of_submitted_plan_freshness() {
    let (_temp, ctx, catalog) = v7_file_budget_repository();
    let plan = super::plan_run(&ctx, &catalog, PlanRunRequest::default()).unwrap();
    fs::write(
        ctx.root().join(".jig/file-budget.toml"),
        "version=1\n[[rules]]\nid=\"source\"\ninclude=[\"**\"]\nmax_lines=101\n",
    )
    .unwrap();

    let error =
        super::validate_run_plan(&ctx, &RepositoryCatalog::from_context(&ctx).unwrap(), &plan)
            .unwrap_err()
            .to_string();
    assert!(
        error.contains("stale") || error.contains("changed"),
        "{error}"
    );
}
