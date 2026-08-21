use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use anyhow::{Context, Result, bail};
use jig_contract::{
    ActionEffect, ActionId, ActionIntent, PlannedTarget, ProfileId, RunPlan, SelectionReason,
    SourceIdentity, TargetId,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{context::RepoContext, git_receipts::repo_worktree_fingerprint};

use super::RepositoryCatalog;

#[derive(Clone, Debug, Default)]
pub(crate) struct PlanRunRequest {
    pub(crate) selectors: Vec<String>,
    pub(crate) profile: Option<String>,
    pub(crate) affected_base: Option<String>,
}

pub(crate) fn plan_run(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    request: PlanRunRequest,
) -> Result<RunPlan> {
    let source = SourceIdentity::new(
        head_commit(ctx)?,
        repo_worktree_fingerprint(ctx.root()).context("Failed to identify the current worktree")?,
    );
    plan_run_with_source(catalog, request, source)
}

/// Re-resolves a plan request against current checked-in configuration and
/// source identity. An accepted plan is executable only while it remains the
/// exact deterministic result of that resolution.
pub(crate) fn validate_run_plan(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    plan: &RunPlan,
) -> Result<()> {
    if plan.schema_version != RunPlan::SCHEMA_VERSION {
        bail!(
            "run plan '{}' uses unsupported schema version {}",
            plan.id,
            plan.schema_version
        );
    }
    if plan.config_digest != catalog.config_digest() {
        bail!(
            "run plan '{}' is stale: repository configuration changed",
            plan.id
        );
    }
    let expected = plan_run(
        ctx,
        catalog,
        PlanRunRequest {
            selectors: plan.selectors.clone(),
            profile: plan.profile.as_ref().map(ToString::to_string),
            affected_base: plan.affected_base.clone(),
        },
    )?;
    if expected != *plan {
        bail!(
            "run plan '{}' is stale or was modified after planning; inspect a fresh plan before execution",
            plan.id
        );
    }
    Ok(())
}

fn head_commit(ctx: &RepoContext) -> Result<Option<String>> {
    let output = Command::new("git")
        .current_dir(ctx.root())
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .context("Failed to run git rev-parse for run planning")?;
    if !output.status.success() {
        return Ok(None);
    }
    let commit = String::from_utf8(output.stdout)
        .context("git rev-parse returned a non-UTF-8 commit id")?
        .trim()
        .to_owned();
    Ok((!commit.is_empty()).then_some(commit))
}

fn plan_run_with_source(
    catalog: &RepositoryCatalog,
    mut request: PlanRunRequest,
    source: SourceIdentity,
) -> Result<RunPlan> {
    if let Some(base) = request.affected_base.as_deref() {
        bail!(
            "affected selection against '{base}' is not available until component input propagation is configured"
        );
    }
    normalize_selectors(&mut request.selectors)?;
    if request.profile.is_some() && !request.selectors.is_empty() {
        bail!("choose either explicit selectors or --profile, not both");
    }

    let mut selected = BTreeMap::<TargetId, BTreeSet<SelectionReason>>::new();
    let selected_profile = match request.profile.as_deref() {
        Some(profile) => Some(ProfileId::parse(profile)?),
        None if request.selectors.is_empty() => catalog.default_check_profile().cloned(),
        None => None,
    };

    if let Some(profile_id) = selected_profile.as_ref() {
        let profile = catalog
            .profile(profile_id)
            .ok_or_else(|| anyhow::anyhow!("unknown check profile '{profile_id}'"))?;
        for target in &profile.targets {
            selected
                .entry(target.clone())
                .or_default()
                .insert(SelectionReason::Profile {
                    profile: profile_id.clone(),
                });
        }
    } else if request.selectors.is_empty() {
        bail!("this workspace has no default check profile");
    }

    for selector in &request.selectors {
        select_explicit(catalog, selector, &mut selected)?;
    }
    expand_action_dependencies(catalog, &mut selected)?;
    validate_check_actions(catalog, selected.keys())?;

    let execution_layers = execution_layers(catalog, selected.keys())?;
    let mut effects = BTreeSet::new();
    let mut targets = Vec::with_capacity(selected.len());
    for (target, reasons) in selected {
        let action = catalog
            .action(&target)
            .expect("selected targets must exist in the repository catalog");
        effects.extend(action.effects.iter().copied());
        let mut planned = PlannedTarget::new(
            target,
            action.intent,
            action.runner.clone(),
            action_input_digest(action, &source.worktree_fingerprint),
        );
        planned.effects.clone_from(&action.effects);
        planned.inputs.clone_from(&action.inputs);
        planned.depends_on.clone_from(&action.depends_on);
        planned.timeout_seconds = action.timeout_seconds;
        planned.result_parser = action.result_parser;
        planned.reasons = reasons.into_iter().collect();
        targets.push(planned);
    }

    let mut plan = RunPlan::new(
        "",
        catalog.config_digest(),
        source,
        targets,
        execution_layers,
    );
    plan.selectors = request.selectors;
    plan.profile = selected_profile;
    plan.effects = effects.into_iter().collect();
    plan.id = plan_digest(&plan)?;
    Ok(plan)
}

fn normalize_selectors(selectors: &mut Vec<String>) -> Result<()> {
    for selector in selectors.iter_mut() {
        *selector = selector.trim().to_owned();
        if selector.is_empty() {
            bail!("check selectors must not be empty");
        }
    }
    selectors.sort();
    selectors.dedup();
    Ok(())
}

fn select_explicit(
    catalog: &RepositoryCatalog,
    selector: &str,
    selected: &mut BTreeMap<TargetId, BTreeSet<SelectionReason>>,
) -> Result<()> {
    if let Some(target) = catalog.target_for_alias(selector) {
        selected
            .entry(target.clone())
            .or_default()
            .insert(SelectionReason::LegacyAlias {
                alias: selector.to_owned(),
            });
        return Ok(());
    }

    let matches = if let Some((component, action)) = selector.split_once(':') {
        if action.contains(':') {
            bail!("invalid target selector '{selector}': expected component:action");
        }
        validate_selector_part("component", component)?;
        validate_selector_part("action", action)?;
        catalog
            .actions()
            .filter(|spec| {
                selector_part_matches(component, spec.target.component.as_str())
                    && selector_part_matches(action, spec.target.action.as_str())
            })
            .map(|spec| spec.target.clone())
            .collect::<Vec<_>>()
    } else {
        validate_selector_part("action", selector)?;
        catalog
            .actions()
            .filter(|spec| selector_part_matches(selector, spec.target.action.as_str()))
            .map(|spec| spec.target.clone())
            .collect::<Vec<_>>()
    };
    if matches.is_empty() {
        bail!("check selector '{selector}' matched no targets");
    }
    for target in matches {
        selected
            .entry(target)
            .or_default()
            .insert(SelectionReason::Explicit {
                selector: selector.to_owned(),
            });
    }
    Ok(())
}

fn validate_selector_part(kind: &str, part: &str) -> Result<()> {
    if part == "*" {
        return Ok(());
    }
    match kind {
        "component" => {
            crate::repository::ComponentId::parse(part)?;
        }
        "action" => {
            ActionId::parse(part)?;
        }
        _ => unreachable!("selector kinds are closed"),
    }
    Ok(())
}

fn selector_part_matches(selector: &str, value: &str) -> bool {
    selector == "*" || selector == value
}

fn expand_action_dependencies(
    catalog: &RepositoryCatalog,
    selected: &mut BTreeMap<TargetId, BTreeSet<SelectionReason>>,
) -> Result<()> {
    let mut pending = selected.keys().cloned().collect::<Vec<_>>();
    while let Some(target) = pending.pop() {
        let action = catalog
            .action(&target)
            .ok_or_else(|| anyhow::anyhow!("selected target '{target}' is not defined"))?;
        for dependency in &action.depends_on {
            let was_new = !selected.contains_key(dependency);
            selected.entry(dependency.clone()).or_default().insert(
                SelectionReason::ActionDependency {
                    target: target.clone(),
                },
            );
            if was_new {
                pending.push(dependency.clone());
            }
        }
    }
    Ok(())
}

fn validate_check_actions<'a>(
    catalog: &RepositoryCatalog,
    targets: impl Iterator<Item = &'a TargetId>,
) -> Result<()> {
    for target in targets {
        let action = catalog
            .action(target)
            .expect("selected targets must exist in the repository catalog");
        if action.intent != ActionIntent::Check
            || !action.effects.contains(&ActionEffect::ReadOnly)
            || action.effects.contains(&ActionEffect::Worktree)
            || action.effects.contains(&ActionEffect::External)
        {
            bail!(
                "target '{target}' is not a read-only check; use the action-specific command or `jig run` for {:?} actions with {:?} effects",
                action.intent,
                action.effects
            );
        }
    }
    Ok(())
}

fn execution_layers<'a>(
    catalog: &RepositoryCatalog,
    targets: impl Iterator<Item = &'a TargetId>,
) -> Result<Vec<Vec<TargetId>>> {
    let targets = targets.cloned().collect::<BTreeSet<_>>();
    let mut remaining = targets.clone();
    let mut completed = BTreeSet::new();
    let mut layers = Vec::new();
    while !remaining.is_empty() {
        let layer = remaining
            .iter()
            .filter(|target| {
                catalog
                    .action(target)
                    .expect("selected targets must exist")
                    .depends_on
                    .iter()
                    .filter(|dependency| targets.contains(*dependency))
                    .all(|dependency| completed.contains(dependency))
            })
            .cloned()
            .collect::<Vec<_>>();
        if layer.is_empty() {
            bail!(
                "action dependency cycle among: {}",
                remaining
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        for target in &layer {
            remaining.remove(target);
            completed.insert(target.clone());
        }
        layers.push(layer);
    }
    Ok(layers)
}

pub(crate) fn target_input_digest(
    catalog: &RepositoryCatalog,
    target: &TargetId,
    worktree_fingerprint: &str,
) -> Result<String> {
    let action = catalog
        .action(target)
        .ok_or_else(|| anyhow::anyhow!("target '{target}' is not defined"))?;
    Ok(action_input_digest(action, worktree_fingerprint))
}

fn action_input_digest(action: &jig_contract::ActionSpec, worktree_fingerprint: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"jig-target-input-v1\0");
    hasher.update(action.target.to_string().as_bytes());
    hasher.update([0]);
    hasher.update(worktree_fingerprint.as_bytes());
    for input in &action.inputs {
        hasher.update([0]);
        hasher.update(input.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}

#[derive(Serialize)]
struct PlanDigestInput<'a> {
    schema_version: u32,
    config_digest: &'a str,
    source: &'a SourceIdentity,
    selectors: &'a [String],
    profile: &'a Option<ProfileId>,
    affected_base: &'a Option<String>,
    targets: &'a [PlannedTarget],
    execution_layers: &'a [Vec<TargetId>],
    effects: &'a [ActionEffect],
}

fn plan_digest(plan: &RunPlan) -> Result<String> {
    let input = PlanDigestInput {
        schema_version: plan.schema_version,
        config_digest: &plan.config_digest,
        source: &plan.source,
        selectors: &plan.selectors,
        profile: &plan.profile,
        affected_base: &plan.affected_base,
        targets: &plan.targets,
        execution_layers: &plan.execution_layers,
        effects: &plan.effects,
    };
    let bytes = serde_json::to_vec(&input).context("Failed to canonicalize the run plan")?;
    Ok(format!("run-plan_sha256:{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use jig_contract::{
        ActionEffect, ActionIntent, ActionRunner, ActionSpec, ComponentId, ComponentSpec,
        ProfileId, ProfileSpec, SelectionReason, SourceIdentity, TargetId,
    };

    use super::{PlanRunRequest, plan_run_with_source};
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
        test.depends_on.push(lint_target);
        let profile_id = ProfileId::parse("verify").unwrap();
        let profile = ProfileSpec::new(profile_id.clone(), vec![test_target]);
        let catalog = RepositoryCatalog::from_native(
            6,
            "digest",
            &[component],
            &[lint, test],
            &[profile],
            Some(&profile_id),
        )
        .unwrap();

        let error = plan_run_with_source(&catalog, PlanRunRequest::default(), source())
            .unwrap_err()
            .to_string();
        assert!(error.contains("action dependency cycle among: api:lint, api:test"));
    }
}
