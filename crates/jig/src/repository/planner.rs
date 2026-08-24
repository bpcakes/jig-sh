use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use jig_contract::{
    ActionArguments, ActionEffect, ActionIntent, ActionRunner, PlannedTarget, ProfileId, RunPlan,
    SelectionReason, SourceIdentity, TargetId,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    context::RepoContext,
    git_receipts::{
        repo_changed_paths_since, repo_observed_ignored_dotenv_paths, repository_source_snapshot,
    },
};

use super::{
    RepositoryCatalog,
    affected::{
        MAX_SELECTION_REASONS, TargetSelection, TargetSelectionReasons,
        add_selection_reason_digest, select_affected_targets, selection_reason_item_digest,
    },
};

const SELECTION_REASONS_DIGEST_DOMAIN: &[u8] = b"jig-selection-reasons-v2\0";

#[derive(Clone, Debug, Default)]
pub(crate) struct PlanRunRequest {
    pub(crate) selectors: Vec<String>,
    pub(crate) profile: Option<String>,
    pub(crate) affected_base: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlanningPolicy {
    ChecksOnly,
    DeclaredActions,
}

pub(crate) fn plan_run(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    request: PlanRunRequest,
) -> Result<RunPlan> {
    plan_run_with_policy(
        ctx,
        catalog,
        request,
        BTreeMap::new(),
        PlanningPolicy::ChecksOnly,
    )
}

pub(crate) fn plan_action_run(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    request: PlanRunRequest,
    arguments: BTreeMap<TargetId, ActionArguments>,
) -> Result<RunPlan> {
    plan_run_with_policy(
        ctx,
        catalog,
        request,
        arguments,
        PlanningPolicy::DeclaredActions,
    )
}

fn plan_run_with_policy(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    mut request: PlanRunRequest,
    arguments: BTreeMap<TargetId, ActionArguments>,
    policy: PlanningPolicy,
) -> Result<RunPlan> {
    normalize_affected_base(&mut request.affected_base)?;
    if request.affected_base.is_some() && catalog.contract_version() < 6 {
        bail!("affected selection requires repository contract version 6 or later");
    }
    let source = current_source_identity(ctx)?;
    let (changed_paths, observed_input_paths) = match request.affected_base.as_deref() {
        Some(base) => (
            Some(repo_changed_paths_since(ctx.root(), base)?),
            repo_observed_ignored_dotenv_paths(ctx.root())?,
        ),
        None => (None, Vec::new()),
    };
    if changed_paths.is_some() && current_source_identity(ctx)? != source {
        bail!("repository source changed while resolving affected paths; plan again");
    }
    plan_run_with_source_and_paths(
        catalog,
        request,
        source,
        changed_paths.as_deref(),
        &observed_input_paths,
        arguments,
        policy,
    )
}

fn current_source_identity(ctx: &RepoContext) -> Result<SourceIdentity> {
    let source = repository_source_snapshot(ctx.root())
        .context("Failed to identify the current worktree")?;
    Ok(SourceIdentity::new(
        source.head_commit,
        source.worktree_fingerprint,
    ))
}

pub(crate) fn validate_run_plan_source(ctx: &RepoContext, plan: &RunPlan) -> Result<()> {
    if current_source_identity(ctx)? != plan.source {
        bail!(
            "run plan '{}' is stale: repository source changed after planning",
            plan.id
        );
    }
    Ok(())
}

/// Confirms that the execution authority currently on disk still matches the
/// digest under which a plan was derived. The source fingerprint deliberately
/// excludes `.agent/**`, so this check is the independent guard for manifest
/// changes as well as config changes that race a long-lived `RepoContext`.
pub(crate) fn validate_current_repository_authority(
    ctx: &RepoContext,
    expected_digest: &str,
) -> Result<()> {
    let current = RepoContext::load_from_root(ctx.root().to_path_buf())
        .context("Failed to reload current repository execution authority")?;
    if current.contract_digest() != expected_digest {
        bail!(
            "repository execution authority changed (expected {expected_digest}, current {}); plan again",
            current.contract_digest()
        );
    }
    Ok(())
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
    // The submitted targets choose only which deterministic resolution path
    // to replay. They cannot grant themselves execution authority: the full
    // re-resolved plan, including targets, effects, runners, and source
    // identity, must still compare equal below.
    let policy = if plan.targets.iter().all(|target| {
        target.intent == ActionIntent::Check
            && target.effects.contains(&ActionEffect::ReadOnly)
            && !target.effects.contains(&ActionEffect::Worktree)
            && !target.effects.contains(&ActionEffect::External)
    }) {
        PlanningPolicy::ChecksOnly
    } else {
        PlanningPolicy::DeclaredActions
    };
    let arguments = plan
        .targets
        .iter()
        .filter(|target| !target.arguments.is_empty())
        .map(|target| (target.target.clone(), target.arguments.clone()))
        .collect();
    let expected = plan_run_with_policy(
        ctx,
        catalog,
        PlanRunRequest {
            selectors: plan.selectors.clone(),
            profile: plan.profile.as_ref().map(ToString::to_string),
            affected_base: plan.affected_base.clone(),
        },
        arguments,
        policy,
    )?;
    if expected != *plan {
        bail!(
            "run plan '{}' is stale or was modified after planning; inspect a fresh plan before execution",
            plan.id
        );
    }
    // Check after the source-dependent re-derivation so a manifest-only
    // change that races that work cannot escape through the `.agent/**`
    // exclusion in the source fingerprint.
    validate_current_repository_authority(ctx, catalog.config_digest())?;
    Ok(())
}

#[cfg(test)]
fn plan_run_with_source(
    catalog: &RepositoryCatalog,
    request: PlanRunRequest,
    source: SourceIdentity,
) -> Result<RunPlan> {
    plan_run_with_source_and_paths(
        catalog,
        request,
        source,
        None,
        &[],
        BTreeMap::new(),
        PlanningPolicy::ChecksOnly,
    )
}

fn plan_run_with_source_and_paths(
    catalog: &RepositoryCatalog,
    mut request: PlanRunRequest,
    source: SourceIdentity,
    changed_paths: Option<&[String]>,
    observed_input_paths: &[String],
    arguments: BTreeMap<TargetId, ActionArguments>,
    policy: PlanningPolicy,
) -> Result<RunPlan> {
    normalize_affected_base(&mut request.affected_base)?;
    normalize_selectors(&mut request.selectors)?;
    if request.profile.is_some() && !request.selectors.is_empty() {
        bail!("choose either explicit selectors or --profile, not both");
    }
    match (&request.affected_base, changed_paths) {
        (Some(base), None) => {
            bail!("affected selection against '{base}' requires repository Git context")
        }
        (Some(_), Some(_)) if catalog.contract_version() < 6 => {
            bail!("affected selection requires repository contract version 6 or later")
        }
        (None, Some(_)) => bail!("affected paths require an explicit Git base"),
        _ => {}
    }

    let mut selected = TargetSelection::new();
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
    if let Some(changed_paths) = changed_paths {
        selected = select_affected_targets(catalog, selected, changed_paths, observed_input_paths)?;
    }
    expand_action_dependencies(catalog, &mut selected)?;
    match policy {
        PlanningPolicy::ChecksOnly => validate_check_actions(catalog, selected.keys())?,
        PlanningPolicy::DeclaredActions => {
            validate_declared_action_selection(catalog, &request, selected.keys())?;
        }
    }
    let arguments = bind_action_arguments(catalog, selected.keys(), arguments)?;

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
            conservative_action_input_digest(action, &source.worktree_fingerprint),
        );
        planned.arguments = arguments.get(&planned.target).cloned().unwrap_or_default();
        planned.effects.clone_from(&action.effects);
        planned.inputs.clone_from(&action.inputs);
        planned.depends_on.clone_from(&action.depends_on);
        planned.timeout_seconds = action.timeout_seconds;
        planned.result_parser = action.result_parser;
        let bounded_reasons = bounded_selection_reasons(reasons)?;
        planned.reasons = bounded_reasons.preview;
        planned.selection_reason_count = bounded_reasons.total;
        planned.selection_reasons_truncated = bounded_reasons.truncated;
        planned.selection_reasons_digest = bounded_reasons.digest;
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
    plan.affected_base = request.affected_base;
    plan.effects = effects.into_iter().collect();
    plan.id = plan_digest(&plan)?;
    Ok(plan)
}

fn normalize_affected_base(base: &mut Option<String>) -> Result<()> {
    let Some(value) = base else {
        return Ok(());
    };
    *value = value.trim().to_owned();
    if value.is_empty() {
        bail!("--affected requires a non-empty Git base");
    }
    Ok(())
}

fn normalize_selectors(selectors: &mut Vec<String>) -> Result<()> {
    for selector in selectors.iter_mut() {
        *selector = selector.trim().to_owned();
        if selector.is_empty() {
            bail!("repository selectors must not be empty");
        }
    }
    selectors.sort();
    selectors.dedup();
    Ok(())
}

fn validate_declared_action_selection<'a>(
    catalog: &RepositoryCatalog,
    request: &PlanRunRequest,
    targets: impl Iterator<Item = &'a TargetId>,
) -> Result<()> {
    let has_effectful_action = targets.into_iter().any(|target| {
        let action = catalog
            .action(target)
            .expect("selected targets must exist in the repository catalog");
        action.intent != ActionIntent::Check
            || action.effects.contains(&ActionEffect::Worktree)
            || action.effects.contains(&ActionEffect::External)
    });
    if has_effectful_action && request.selectors.is_empty() {
        bail!("effectful repository actions require explicit selectors");
    }
    Ok(())
}

fn bind_action_arguments<'a>(
    catalog: &RepositoryCatalog,
    targets: impl Iterator<Item = &'a TargetId>,
    mut arguments: BTreeMap<TargetId, ActionArguments>,
) -> Result<BTreeMap<TargetId, ActionArguments>> {
    let targets = targets.cloned().collect::<BTreeSet<_>>();
    if let Some(target) = arguments.keys().find(|target| !targets.contains(*target)) {
        bail!("arguments were provided for unselected target '{target}'");
    }
    for target in &targets {
        let action = catalog
            .action(target)
            .expect("selected targets must exist in the repository catalog");
        let requires_name = matches!(
            &action.runner,
            ActionRunner::Native { operation }
                if jig_features::native_tool_requires_name(operation)
        );
        let supplied = arguments.get(target).and_then(|args| args.name.as_deref());
        if requires_name {
            let name = supplied.ok_or_else(|| {
                anyhow::anyhow!("target '{target}' requires string argument 'name'")
            })?;
            if name.trim().is_empty() || name.starts_with('-') {
                bail!("target '{target}' has invalid argument 'name'");
            }
        } else if supplied.is_some() {
            bail!("target '{target}' does not accept argument 'name'");
        }
    }
    arguments.retain(|_, value| !value.is_empty());
    Ok(arguments)
}

fn select_explicit(
    catalog: &RepositoryCatalog,
    selector: &str,
    selected: &mut TargetSelection,
) -> Result<()> {
    let legacy_alias = catalog.target_for_alias(selector);
    let canonical = match super::RepositorySelector::parse(selector) {
        Ok(canonical) => canonical,
        Err(_) if legacy_alias.is_some() => {
            select_legacy_alias(selected, selector, legacy_alias.expect("alias was checked"));
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let matches = catalog
        .actions()
        .filter(|spec| canonical.matches(&spec.target))
        .map(|spec| spec.target.clone())
        .collect::<Vec<_>>();
    if matches.is_empty() {
        if let Some(target) = legacy_alias {
            select_legacy_alias(selected, selector, target);
            return Ok(());
        }
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

fn select_legacy_alias(selected: &mut TargetSelection, alias: &str, target: &TargetId) {
    selected
        .entry(target.clone())
        .or_default()
        .insert(SelectionReason::LegacyAlias {
            alias: alias.to_owned(),
        });
}

fn expand_action_dependencies(
    catalog: &RepositoryCatalog,
    selected: &mut TargetSelection,
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

pub(super) fn validate_check_actions<'a>(
    catalog: &RepositoryCatalog,
    targets: impl Iterator<Item = &'a TargetId>,
) -> Result<()> {
    let mut pending = targets.cloned().collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    while let Some(target) = pending.pop() {
        if !seen.insert(target.clone()) {
            continue;
        }
        let action = catalog
            .action(&target)
            .expect("selected targets must exist in the repository catalog");
        if action.intent != ActionIntent::Check
            || !action.effects.contains(&ActionEffect::ReadOnly)
            || action.effects.contains(&ActionEffect::Worktree)
            || action.effects.contains(&ActionEffect::External)
        {
            bail!(
                "target '{target}' is not a read-only check; use the action-specific command or plan and execute it through the MCP repository tools for {:?} actions with {:?} effects",
                action.intent,
                action.effects
            );
        }
        pending.extend(action.depends_on.iter().cloned());
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
    Ok(conservative_action_input_digest(
        action,
        worktree_fingerprint,
    ))
}

/// Binds a target receipt to both its declared inputs and the repository-wide
/// source projection. This is intentionally conservative freshness authority,
/// not a per-target artifact-cache key: any observed source change invalidates
/// the receipt even when it falls outside the target's selection patterns.
fn conservative_action_input_digest(
    action: &jig_contract::ActionSpec,
    worktree_fingerprint: &str,
) -> String {
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

struct BoundedSelectionReasons {
    preview: Vec<SelectionReason>,
    total: Option<usize>,
    truncated: bool,
    digest: Option<String>,
}

fn bounded_selection_reasons(reasons: TargetSelectionReasons) -> Result<BoundedSelectionReasons> {
    let mut total = reasons.explicit.len();
    let mut sum_digest = [0u8; 32];
    for reason in &reasons.explicit {
        add_selection_reason_digest(&mut sum_digest, selection_reason_item_digest(reason)?);
    }
    let mut preview = reasons.explicit.into_iter().collect::<BTreeSet<_>>();
    for batch in reasons.batches {
        total = total.saturating_add(batch.count);
        add_selection_reason_digest(&mut sum_digest, batch.sum_digest);
        preview.extend(batch.preview.iter().cloned());
    }
    let mut preview = preview.into_iter().collect::<Vec<_>>();
    if total <= MAX_SELECTION_REASONS {
        if preview.len() != total {
            bail!(
                "could not construct deterministic selection evidence because reason batches overlap (expected {total} unique reasons, observed {}); retry planning and report this as a Jig defect if it recurs",
                preview.len()
            );
        }
        return Ok(BoundedSelectionReasons {
            preview,
            total: None,
            truncated: false,
            digest: None,
        });
    }

    let mut hasher = Sha256::new();
    hasher.update(SELECTION_REASONS_DIGEST_DOMAIN);
    hasher.update((total as u64).to_be_bytes());
    hasher.update(sum_digest);
    let digest = format!("sha256:{:x}", hasher.finalize());

    // Preserve intent and dependency explanations before path-expanded detail.
    // Natural ordering remains the tie-breaker within each semantic class.
    preview.sort_by(|left, right| {
        selection_reason_priority(left)
            .cmp(&selection_reason_priority(right))
            .then_with(|| left.cmp(right))
    });
    preview.truncate(MAX_SELECTION_REASONS);
    Ok(BoundedSelectionReasons {
        preview,
        total: Some(total),
        truncated: true,
        digest: Some(digest),
    })
}

const fn selection_reason_priority(reason: &SelectionReason) -> u8 {
    match reason {
        SelectionReason::Explicit { .. }
        | SelectionReason::Profile { .. }
        | SelectionReason::ActionDependency { .. }
        | SelectionReason::LegacyAlias { .. } => 0,
        SelectionReason::DirectInput { .. }
        | SelectionReason::UnclaimedInput { .. }
        | SelectionReason::ComponentDependency { .. } => 1,
    }
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
mod tests;
