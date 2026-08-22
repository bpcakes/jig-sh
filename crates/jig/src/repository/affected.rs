use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use globset::{GlobBuilder, GlobMatcher};
use jig_contract::{ActionSpec, ComponentId, ComponentSpec, SelectionReason, TargetId};

use super::RepositoryCatalog;

pub(super) type TargetSelection = BTreeMap<TargetId, BTreeSet<SelectionReason>>;

/// Validates the path policy needed by affected selection when a native
/// repository catalog is constructed. Legacy projections do not declare this
/// policy and continue to use their compatibility behavior.
pub(super) fn validate_native_path_policy(
    components: &BTreeMap<ComponentId, ComponentSpec>,
    actions: &BTreeMap<TargetId, ActionSpec>,
) -> Result<()> {
    for component in components.values() {
        validate_component_root(component)?;
    }
    for action in actions.values() {
        for input in &action.inputs {
            compile_input(&action.target, input)?;
        }
    }
    Ok(())
}

/// Narrows an already resolved selector/profile candidate set to targets whose
/// components are affected by the supplied repository-relative changed paths.
/// Action dependencies deliberately expand later in the planner.
pub(super) fn select_affected_targets(
    catalog: &RepositoryCatalog,
    candidates: TargetSelection,
    changed_paths: &[String],
) -> Result<TargetSelection> {
    let paths = normalized_changed_paths(changed_paths)?;
    let matchers = component_input_matchers(catalog)?;
    let direct = directly_affected_components(catalog, &matchers, &paths);
    let propagated = propagated_components(catalog, &direct);

    let mut selected = TargetSelection::new();
    for (target, mut reasons) in candidates {
        let mut affected = false;
        if let Some(paths) = direct.get(&target.component) {
            affected = true;
            reasons.extend(
                paths
                    .iter()
                    .cloned()
                    .map(|path| SelectionReason::DirectInput { path }),
            );
        }
        if let Some(origins) = propagated.get(&target.component) {
            affected = true;
            reasons.extend(origins.iter().cloned().map(|(component, path)| {
                SelectionReason::ComponentDependency {
                    component,
                    path: Some(path),
                }
            }));
        }
        if affected {
            selected.insert(target, reasons);
        }
    }
    Ok(selected)
}

fn validate_component_root(component: &ComponentSpec) -> Result<()> {
    let root = component.root.as_str();
    if root == "." {
        return Ok(());
    }
    validate_repository_relative_text("component root", root)?;
    if root.contains(['*', '?', '[', ']', '{', '}']) {
        bail!(
            "component '{}' has invalid root {:?}: component roots must be literal repository-relative directories",
            component.id,
            root
        );
    }
    Ok(())
}

fn compile_input(target: &TargetId, input: &str) -> Result<GlobMatcher> {
    if let Err(error) = validate_repository_relative_text("action input", input) {
        bail!("target '{target}' has invalid input pattern {input:?}: {error}");
    }
    let mut builder = GlobBuilder::new(input);
    builder.literal_separator(true).backslash_escape(false);
    match builder.build() {
        Ok(glob) => Ok(glob.compile_matcher()),
        Err(error) => {
            bail!("target '{target}' has invalid input pattern {input:?}: {error}")
        }
    }
}

fn validate_repository_relative_text(kind: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{kind} must not be empty");
    }
    if value != value.trim() {
        bail!("{kind} must not have surrounding whitespace");
    }
    if value.starts_with('/') || value.contains('\\') {
        bail!("{kind} must use repository-relative forward-slash syntax");
    }
    if value.as_bytes().get(1) == Some(&b':') {
        bail!("{kind} must not use an absolute drive path");
    }
    if value.contains('\0') {
        bail!("{kind} must not contain a NUL byte");
    }
    for segment in value.split('/') {
        if segment.is_empty() {
            bail!("{kind} must not contain an empty path segment");
        }
        if matches!(segment, "." | "..") {
            bail!("{kind} must not contain '.' or '..' path segments");
        }
    }
    Ok(())
}

fn normalized_changed_paths(changed_paths: &[String]) -> Result<BTreeSet<String>> {
    let mut normalized = BTreeSet::new();
    for path in changed_paths {
        validate_repository_relative_text("changed path", path)
            .with_context(|| format!("Git reported invalid changed path {path:?}"))?;
        normalized.insert(path.clone());
    }
    Ok(normalized)
}

fn component_input_matchers(
    catalog: &RepositoryCatalog,
) -> Result<BTreeMap<ComponentId, Vec<GlobMatcher>>> {
    let mut matchers = catalog
        .components()
        .map(|component| (component.id.clone(), Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for action in catalog.actions() {
        let component_matchers = matchers
            .get_mut(&action.target.component)
            .expect("catalog actions must reference defined components");
        for input in &action.inputs {
            component_matchers.push(compile_input(&action.target, input)?);
        }
    }
    Ok(matchers)
}

fn directly_affected_components(
    catalog: &RepositoryCatalog,
    matchers: &BTreeMap<ComponentId, Vec<GlobMatcher>>,
    paths: &BTreeSet<String>,
) -> BTreeMap<ComponentId, BTreeSet<String>> {
    let mut direct = BTreeMap::<ComponentId, BTreeSet<String>>::new();
    for path in paths {
        let matched_components = matchers
            .iter()
            .filter(|(_, patterns)| patterns.iter().any(|pattern| pattern.is_match(path)))
            .map(|(component, _)| component.clone())
            .collect::<BTreeSet<_>>();
        let matched_components = if matched_components.is_empty() {
            most_specific_component_roots(catalog, matchers, path)
        } else {
            matched_components
        };
        for component in matched_components {
            direct.entry(component).or_default().insert(path.clone());
        }
    }
    direct
}

fn most_specific_component_roots(
    catalog: &RepositoryCatalog,
    matchers: &BTreeMap<ComponentId, Vec<GlobMatcher>>,
    path: &str,
) -> BTreeSet<ComponentId> {
    let mut matches = Vec::new();
    let mut maximum_depth = None;
    for component in catalog.components() {
        if !root_contains(&component.root, path) {
            continue;
        }
        if component.root == "."
            && matchers
                .get(&component.id)
                .is_some_and(|patterns| !patterns.is_empty())
        {
            continue;
        }
        let depth = root_depth(&component.root);
        maximum_depth = Some(maximum_depth.map_or(depth, |current: usize| current.max(depth)));
        matches.push((depth, component.id.clone()));
    }
    matches
        .into_iter()
        .filter(|(depth, _)| Some(*depth) == maximum_depth)
        .map(|(_, component)| component)
        .collect()
}

fn root_contains(root: &str, path: &str) -> bool {
    root == "."
        || path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn root_depth(root: &str) -> usize {
    if root == "." {
        0
    } else {
        root.split('/').count()
    }
}

type PropagationOrigin = (ComponentId, String);

fn propagated_components(
    catalog: &RepositoryCatalog,
    direct: &BTreeMap<ComponentId, BTreeSet<String>>,
) -> BTreeMap<ComponentId, BTreeSet<PropagationOrigin>> {
    let mut dependents = BTreeMap::<ComponentId, BTreeSet<ComponentId>>::new();
    for component in catalog.components() {
        for dependency in &component.depends_on {
            dependents
                .entry(dependency.clone())
                .or_default()
                .insert(component.id.clone());
        }
    }

    let mut propagated = BTreeMap::<ComponentId, BTreeSet<PropagationOrigin>>::new();
    for (origin, paths) in direct {
        for path in paths {
            let mut seen = BTreeSet::from([origin.clone()]);
            let mut pending = vec![origin.clone()];
            while let Some(component_id) = pending.pop() {
                let component = catalog
                    .component(&component_id)
                    .expect("affected components must exist in the catalog");
                if !component.propagate_affected_to_dependents {
                    continue;
                }
                for dependent in dependents.get(&component_id).into_iter().flatten() {
                    if dependent != origin {
                        propagated
                            .entry(dependent.clone())
                            .or_default()
                            .insert((origin.clone(), path.clone()));
                    }
                    if seen.insert(dependent.clone()) {
                        pending.push(dependent.clone());
                    }
                }
            }
        }
    }
    propagated
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use jig_contract::{
        ActionEffect, ActionIntent, ActionRunner, ActionSpec, ComponentId, ComponentSpec,
        ProfileId, ProfileSpec, SelectionReason, TargetId,
    };

    use super::{TargetSelection, select_affected_targets};
    use crate::repository::RepositoryCatalog;

    fn affected_fixture() -> RepositoryCatalog {
        let mut shared =
            ComponentSpec::new(ComponentId::parse("shared").unwrap(), "packages/shared");
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
        api_action.inputs = vec!["api/**/*.go".into(), "go.work".into()];
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
        api_action.inputs.push("crates/**/*.rs".into());
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
        let mut action =
            ActionSpec::new(target, ActionIntent::Check, ActionRunner::command(command));
        action.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
        action
    }

    fn candidates(catalog: &RepositoryCatalog) -> TargetSelection {
        let profile = ProfileId::parse("verify").unwrap();
        catalog
            .actions()
            .map(|action| {
                (
                    action.target.clone(),
                    BTreeSet::from([SelectionReason::Profile {
                        profile: profile.clone(),
                    }]),
                )
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
        assert!(selection[&"shared:test".parse().unwrap()].contains(
            &SelectionReason::DirectInput {
                path: "packages/shared/value.txt".into(),
            }
        ));
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
    fn affected_root_fallback_uses_the_most_specific_component() {
        let catalog = affected_fixture();

        assert_eq!(
            target_names(&selected(&catalog, &["api/generated.data"])),
            ["api:test"]
        );
    }

    #[test]
    fn affected_unrelated_paths_produce_an_empty_selection() {
        let catalog = affected_fixture();

        assert!(selected(&catalog, &["notes/todo.txt"]).is_empty());
    }

    #[test]
    fn affected_unmatched_path_does_not_select_explicit_root_components() {
        let catalog = generated_root_fixture();

        assert!(selected(&catalog, &["docs/guide.md"]).is_empty());
        assert_eq!(
            target_names(&selected(&catalog, &["crates/api/src/lib.rs"])),
            ["api:test", "web:test"]
        );
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
            reasons.iter().cloned().collect::<Vec<_>>(),
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
    }
}
