use std::collections::BTreeMap;

use jig_contract::{
    ActionEffect, ActionIntent, ActionRunner, ActionSpec, ComponentId, ComponentSpec, ProfileId,
    ProfileSpec, SelectionReason, TargetId,
};

use super::{TargetSelection, select_affected_targets};
use crate::repository::RepositoryCatalog;

fn affected_fixture() -> RepositoryCatalog {
    let mut shared = ComponentSpec::new(ComponentId::parse("shared").unwrap(), "packages/shared");
    shared.propagate_affected_to_dependents = true;
    let mut api = ComponentSpec::new(ComponentId::parse("api").unwrap(), "api");
    api.depends_on.push(shared.id.clone());
    let mut web = ComponentSpec::new(ComponentId::parse("web").unwrap(), "web");
    web.depends_on.push(shared.id.clone());

    let shared_target: TargetId = "shared:test".parse().unwrap();
    let api_target: TargetId = "api:test".parse().unwrap();
    let web_target: TargetId = "web:test".parse().unwrap();
    let mut shared_action = check_action(shared_target.clone(), "shared_test_command");
    shared_action.inputs.push("packages/shared/**".into());
    let mut api_action = check_action(api_target.clone(), "api_test_command");
    api_action.inputs = vec!["api/**/*.go".into(), "go.work".into(), ".env".into()];
    let mut web_action = check_action(web_target.clone(), "web_test_command");
    web_action.inputs.push("web/**/*.ts".into());

    let profile_id = ProfileId::parse("verify").unwrap();
    let profile = ProfileSpec::new(
        profile_id.clone(),
        vec![shared_target, api_target, web_target],
    );
    RepositoryCatalog::from_native(
        6,
        "sha256:config",
        &[shared, api, web],
        &[shared_action, api_action, web_action],
        &[profile],
        Some(&profile_id),
    )
    .unwrap()
}

fn generated_root_fixture() -> RepositoryCatalog {
    let repo = ComponentSpec::new(ComponentId::parse("repo").unwrap(), ".");
    let mut api = ComponentSpec::new(ComponentId::parse("api").unwrap(), ".");
    api.propagate_affected_to_dependents = true;
    let mut web = ComponentSpec::new(ComponentId::parse("web").unwrap(), "apps/web");
    web.depends_on.push(api.id.clone());

    let repo_target: TargetId = "repo:contract".parse().unwrap();
    let api_target: TargetId = "api:test".parse().unwrap();
    let web_target: TargetId = "web:test".parse().unwrap();
    let mut repo_action = check_action(repo_target.clone(), "contract_command");
    repo_action.inputs.push(".jig.toml".into());
    let mut api_action = check_action(api_target.clone(), "api_test_command");
    api_action.inputs = vec![
        "crates/**/*.rs".into(),
        "**/Cargo.toml".into(),
        "**/go.mod".into(),
    ];
    let mut web_action = check_action(web_target.clone(), "web_test_command");
    web_action.inputs.push("apps/web/**/*.ts".into());

    let profile_id = ProfileId::parse("verify").unwrap();
    let profile = ProfileSpec::new(
        profile_id.clone(),
        vec![repo_target, api_target, web_target],
    );
    RepositoryCatalog::from_native(
        6,
        "sha256:config",
        &[repo, api, web],
        &[repo_action, api_action, web_action],
        &[profile],
        Some(&profile_id),
    )
    .unwrap()
}

fn check_action(target: TargetId, command: &str) -> ActionSpec {
    let mut action = ActionSpec::new(target, ActionIntent::Check, ActionRunner::command(command));
    action.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
    action
}

fn candidates(catalog: &RepositoryCatalog) -> TargetSelection {
    let profile = ProfileId::parse("verify").unwrap();
    catalog
        .actions()
        .map(|action| {
            let mut reasons = super::TargetSelectionReasons::default();
            reasons.insert(SelectionReason::Profile {
                profile: profile.clone(),
            });
            (action.target.clone(), reasons)
        })
        .collect::<BTreeMap<_, _>>()
}

fn selected(catalog: &RepositoryCatalog, changed_paths: &[&str]) -> TargetSelection {
    select_affected_targets(
        catalog,
        candidates(catalog),
        &changed_paths
            .iter()
            .map(|path| (*path).into())
            .collect::<Vec<_>>(),
        &[],
    )
    .unwrap()
}

fn selected_with_observed_inputs(
    catalog: &RepositoryCatalog,
    observed_input_paths: &[&str],
) -> TargetSelection {
    select_affected_targets(
        catalog,
        candidates(catalog),
        &[],
        &observed_input_paths
            .iter()
            .map(|path| (*path).into())
            .collect::<Vec<_>>(),
    )
    .unwrap()
}

fn target_names(selection: &TargetSelection) -> Vec<String> {
    selection.keys().map(ToString::to_string).collect()
}

#[test]
fn affected_direct_api_and_web_inputs_remain_component_scoped() {
    let catalog = affected_fixture();

    assert_eq!(
        target_names(&selected(&catalog, &["api/main.go"])),
        ["api:test"]
    );
    assert_eq!(
        target_names(&selected(&catalog, &["web/src/main.ts"])),
        ["web:test"]
    );
}

#[test]
fn affected_shared_input_propagates_only_by_component_policy() {
    let catalog = affected_fixture();
    let selection = selected(&catalog, &["packages/shared/value.txt"]);

    assert_eq!(
        target_names(&selection),
        ["api:test", "shared:test", "web:test"]
    );
    assert!(
        selection[&"shared:test".parse().unwrap()].contains(&SelectionReason::DirectInput {
            path: "packages/shared/value.txt".into(),
        })
    );
    for target in ["api:test", "web:test"] {
        assert!(selection[&target.parse().unwrap()].contains(
            &SelectionReason::ComponentDependency {
                component: ComponentId::parse("shared").unwrap(),
                path: Some("packages/shared/value.txt".into()),
            }
        ));
    }
}

#[test]
fn affected_repository_global_input_is_explicit() {
    let catalog = affected_fixture();
    let selection = selected(&catalog, &["go.work"]);

    assert_eq!(target_names(&selection), ["api:test"]);
    assert!(
        selection[&"api:test".parse().unwrap()].contains(&SelectionReason::DirectInput {
            path: "go.work".into(),
        })
    );
}

#[test]
fn presence_only_dotenv_paths_select_only_explicit_consumers() {
    let catalog = affected_fixture();
    let selection = selected_with_observed_inputs(&catalog, &[".env", ".env.local"]);

    assert_eq!(target_names(&selection), ["api:test"]);
    assert!(
        selection[&"api:test".parse().unwrap()].contains(&SelectionReason::DirectInput {
            path: ".env".into(),
        })
    );
    assert!(selection.values().all(|reasons| {
        !reasons.contains(&SelectionReason::UnclaimedInput {
            path: ".env.local".into(),
        })
    }));
}

#[test]
fn affected_root_fallback_uses_the_most_specific_component() {
    let catalog = affected_fixture();

    assert_eq!(
        target_names(&selected(&catalog, &["api/generated.data"])),
        ["api:test"]
    );
}

#[test]
fn affected_unclaimed_paths_fail_closed_to_the_candidate_set() {
    let catalog = affected_fixture();
    let selection = selected(&catalog, &["notes/todo.txt"]);

    assert_eq!(
        target_names(&selection),
        ["api:test", "shared:test", "web:test"]
    );
    for reasons in selection.values() {
        assert!(reasons.contains(&SelectionReason::UnclaimedInput {
            path: "notes/todo.txt".into(),
        }));
    }
}

#[test]
fn affected_ignore_skips_only_reviewed_non_authority_paths() {
    let mut catalog = affected_fixture();
    catalog.affected_ignore = vec!["README.md".into(), "docs/**".into()];

    assert!(selected(&catalog, &["README.md", "docs/guide.md"]).is_empty());
    assert_eq!(
        target_names(&selected(&catalog, &["docs/guide.md", "api/main.go"])),
        ["api:test"]
    );
}

#[test]
fn explicit_action_inputs_take_precedence_over_affected_ignore() {
    let mut catalog = affected_fixture();
    catalog.affected_ignore = vec!["go.work".into()];

    assert_eq!(
        target_names(&selected(&catalog, &["go.work"])),
        ["api:test"]
    );
}

#[test]
fn ignored_documentation_artifacts_still_select_explicit_consumers() {
    let mut catalog = affected_fixture();
    let target: TargetId = "api:test".parse().unwrap();
    catalog
        .actions
        .get_mut(&target)
        .unwrap()
        .inputs
        .push("docs/public/**".into());
    catalog.affected_ignore = vec!["docs/**".into()];

    assert_eq!(
        target_names(&selected(&catalog, &["docs/public/bundle.js"])),
        ["api:test"]
    );
}

#[test]
fn affected_unclaimed_reason_batch_is_bounded_and_shared_across_targets() {
    let catalog = affected_fixture();
    let paths = (0..1_000)
        .map(|index| format!("unclaimed/{index:04}.txt"))
        .collect::<Vec<_>>();
    let selection = select_affected_targets(&catalog, candidates(&catalog), &paths, &[]).unwrap();
    let mut reasons = selection.values();
    let first = reasons.next().unwrap().batches.first().unwrap();

    assert_eq!(first.count, 1_000);
    assert_eq!(first.preview.len(), super::MAX_SELECTION_REASONS);
    assert!(
        reasons
            .all(|candidate| { std::sync::Arc::ptr_eq(first, candidate.batches.first().unwrap()) })
    );
}

#[test]
fn affected_unmatched_path_selects_all_candidates_instead_of_failing_open() {
    let catalog = generated_root_fixture();

    let selection = selected(&catalog, &["docs/guide.md"]);
    assert_eq!(
        target_names(&selection),
        ["api:test", "repo:contract", "web:test"]
    );
    for reasons in selection.values() {
        assert!(reasons.contains(&SelectionReason::UnclaimedInput {
            path: "docs/guide.md".into(),
        }));
    }
    assert_eq!(
        target_names(&selected(&catalog, &["crates/api/src/lib.rs"])),
        ["api:test", "web:test"]
    );
}

#[test]
fn affected_git_paths_preserve_legal_posix_backslashes_and_whitespace() {
    let catalog = generated_root_fixture();

    for path in [r#"docs/a\b.txt"#, " docs/guide.md "] {
        let selection = selected(&catalog, &[path]);
        assert_eq!(
            target_names(&selection),
            ["api:test", "repo:contract", "web:test"]
        );
        for reasons in selection.values() {
            assert!(reasons.contains(&SelectionReason::UnclaimedInput { path: path.into() }));
        }
    }
}

#[test]
fn affected_nested_build_manifests_select_the_root_backend_and_dependents() {
    let catalog = generated_root_fixture();

    for path in ["crates/worker/Cargo.toml", "services/worker/go.mod"] {
        assert_eq!(
            target_names(&selected(&catalog, &[path])),
            ["api:test", "web:test"]
        );
    }
}

#[test]
fn affected_repository_authority_input_selects_every_candidate() {
    let catalog = generated_root_fixture();
    let path = ".jig.toml";
    let selection = selected(&catalog, &[path]);

    assert_eq!(
        target_names(&selection),
        ["api:test", "repo:contract", "web:test"]
    );
    for reasons in selection.values() {
        assert!(reasons.contains(&SelectionReason::DirectInput { path: path.into() }));
    }
}

#[test]
fn affected_reasons_and_targets_are_stable_and_sorted() {
    let catalog = affected_fixture();
    let first = selected(
        &catalog,
        &["packages/shared/z.txt", "packages/shared/a.txt"],
    );
    let second = selected(
        &catalog,
        &[
            "packages/shared/a.txt",
            "packages/shared/z.txt",
            "packages/shared/a.txt",
        ],
    );

    assert_eq!(first, second);
    let reasons = &first[&"api:test".parse().unwrap()];
    assert_eq!(
        reasons.preview().into_iter().collect::<Vec<_>>(),
        [
            SelectionReason::Profile {
                profile: ProfileId::parse("verify").unwrap(),
            },
            SelectionReason::ComponentDependency {
                component: ComponentId::parse("shared").unwrap(),
                path: Some("packages/shared/a.txt".into()),
            },
            SelectionReason::ComponentDependency {
                component: ComponentId::parse("shared").unwrap(),
                path: Some("packages/shared/z.txt".into()),
            },
        ]
    );
}

#[test]
fn affected_path_policy_rejects_escaping_roots_and_inputs() {
    let component = ComponentSpec::new(ComponentId::parse("api").unwrap(), "/api");
    let target: TargetId = "api:test".parse().unwrap();
    let action = check_action(target.clone(), "api_test_command");
    let profile_id = ProfileId::parse("verify").unwrap();
    let profile = ProfileSpec::new(profile_id.clone(), vec![target]);
    let error = RepositoryCatalog::from_native(
        6,
        "digest",
        &[component],
        &[action],
        &[profile],
        Some(&profile_id),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("repository-relative"));

    let component = ComponentSpec::new(ComponentId::parse("api").unwrap(), "api");
    let target: TargetId = "api:test".parse().unwrap();
    let mut action = check_action(target.clone(), "api_test_command");
    action.inputs.push("../secrets/**".into());
    let profile = ProfileSpec::new(profile_id.clone(), vec![target]);
    let error = RepositoryCatalog::from_native(
        6,
        "digest",
        &[component],
        &[action],
        &[profile],
        Some(&profile_id),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("must not contain '.' or '..'"));

    let component = ComponentSpec::new(ComponentId::parse("api").unwrap(), "api");
    let target: TargetId = "api:test".parse().unwrap();
    let action = check_action(target.clone(), "api_test_command");
    let profile = ProfileSpec::new(profile_id.clone(), vec![target]);
    let error = RepositoryCatalog::from_native_with_ignore(
        6,
        "digest",
        &[component],
        &[action],
        &[profile],
        Some(&profile_id),
        &["**".into()],
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("must not match repository execution authority"));

    let component = ComponentSpec::new(ComponentId::parse("api").unwrap(), "services/\napi");
    let target: TargetId = "api:test".parse().unwrap();
    let action = check_action(target.clone(), "api_test_command");
    let profile = ProfileSpec::new(profile_id.clone(), vec![target]);
    let error = RepositoryCatalog::from_native(
        6,
        "digest",
        &[component],
        &[action],
        &[profile],
        Some(&profile_id),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("must not contain control characters"));
}

#[test]
fn affected_path_policy_rejects_sources_inside_the_unobserved_agent_tree() {
    let target: TargetId = "api:test".parse().unwrap();
    let profile_id = ProfileId::parse("verify").unwrap();

    let component = ComponentSpec::new(ComponentId::parse("api").unwrap(), ".agent/api");
    let action = check_action(target.clone(), "api_test_command");
    let profile = ProfileSpec::new(profile_id.clone(), vec![target.clone()]);
    let error = RepositoryCatalog::from_native(
        6,
        "digest",
        &[component],
        &[action],
        &[profile],
        Some(&profile_id),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("excluded from source identity"), "{error}");

    let component = ComponentSpec::new(ComponentId::parse("api").unwrap(), "api");
    let mut action = check_action(target.clone(), "api_test_command");
    action.inputs.push(".agent/generated/**".into());
    let profile = ProfileSpec::new(profile_id.clone(), vec![target]);
    let error = RepositoryCatalog::from_native(
        6,
        "digest",
        &[component],
        &[action],
        &[profile],
        Some(&profile_id),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("excluded from source identity"), "{error}");
}
