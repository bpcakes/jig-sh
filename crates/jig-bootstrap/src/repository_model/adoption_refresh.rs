use super::*;
use crate::adopt_infer::ComponentCandidates;
use crate::adopt_infer::components::Ecosystem;

/// Reconfigure generated harness capabilities while retaining authored component
/// identities and project-owned actions. The caller only uses this after an
/// explicit footprint or capability change during readoption.
pub fn refresh(
    answers: &RenderAnswers,
    candidates: &ComponentCandidates,
    prior: &AuthoredRepositoryModel,
    prior_commands: &BTreeMap<String, String>,
) -> Result<(AuthoredRepositoryModel, BTreeMap<String, String>)> {
    if !prior
        .actions
        .iter()
        .any(|action| generated_action(action, prior))
    {
        return Ok((prior.clone(), prior_commands.clone()));
    }
    let mut selection = candidates.clone();
    // Opaque project-owned components stay in `prior`; they are not adapter input.
    selection
        .candidates
        .retain(|candidate| candidate.ecosystem != Ecosystem::Authored);
    for candidate in &mut selection.candidates {
        if let Some(app) = answers
            .frontend_apps()
            .iter()
            .find(|app| app.dir == candidate.root)
        {
            candidate.frontend_name = Some(app.name.clone());
        }
    }
    let (generated, generated_commands) =
        RepositoryRenderModel::from_adoption(answers, &selection)?;
    let mut model = prior.clone();
    let mut commands = prior_commands.clone();
    commands.retain(|_, value| !value.trim().is_empty());
    let managed = managed_targets(&generated, &generated_commands, prior, prior_commands);
    drop_managed_commands(prior, &managed, &mut commands);
    let declared = declared_dependencies(prior, &managed);
    model
        .actions
        .retain(|action| !managed.contains(&action.target));
    let mut retained_owners = BTreeMap::new();
    for mut action in generated.actions {
        if model
            .actions
            .iter()
            .any(|prior| prior.target == action.target)
        {
            continue;
        }
        let owners = model
            .actions
            .iter()
            .filter(|prior| {
                prior
                    .legacy_aliases
                    .iter()
                    .any(|alias| action.legacy_aliases.contains(alias))
            })
            .map(|prior| prior.target.clone())
            .collect::<Vec<_>>();
        if !owners.is_empty() {
            retained_owners.insert(action.target, owners);
            continue;
        }
        // As on update, only a model that predates the document check gains
        // it; a refresh never restores a check its owner removed, nor runs it
        // through a command the owner keeps under its key.
        if action.target.action.as_str() == OPENAPI_DOCUMENT_CHECK_ACTION
            && !prior
                .actions
                .iter()
                .any(|prior| prior.target.action.as_str() == OPENAPI_DOCUMENT_CHECK_ACTION)
            && !prior.predates_document_check(prior_commands, answers.backend_language().is_go())
        {
            continue;
        }
        // Nor does it restore a boundary check that the model lacks: its
        // owner removed it, even where an admin app remains.
        if action.target.component.as_str() == REPO_COMPONENT
            && action.target.action.as_str() == FRONTEND_PUBLIC_BOUNDARY_ACTION
            && !prior
                .actions
                .iter()
                .any(|prior| prior.target == action.target)
        {
            continue;
        }
        if let ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. } =
            &action.runner
            && let Some(value) = generated_commands.get(command)
        {
            commands
                .entry(command.clone())
                .or_insert_with(|| value.clone());
        }
        carry_declared_dependencies(&mut action, &declared);
        model.actions.push(action);
    }
    let remap_targets = |targets: &mut Vec<TargetId>| {
        *targets = targets
            .iter()
            .flat_map(|target| {
                retained_owners
                    .get(target)
                    .cloned()
                    .unwrap_or_else(|| vec![target.clone()])
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
    };
    for action in &mut model.actions {
        remap_targets(&mut action.depends_on);
    }
    // Component identity, roots, tags and dependency edges remain source-owned.
    // Capability adapters follow explicit SQLx choices on generated backend owners.
    for component in &mut model.components {
        if let Some(current) = generated
            .components
            .iter()
            .find(|current| current.id == component.id)
        {
            component.adapters.retain(|adapter| adapter != "sqlx");
            if current.adapters.iter().any(|adapter| adapter == "sqlx") {
                component.adapters.push("sqlx".into());
            }
        }
    }
    let live_targets = model
        .actions
        .iter()
        .map(|action| action.target.clone())
        .collect::<BTreeSet<_>>();
    for profile in &mut model.profiles {
        remap_targets(&mut profile.targets);
        profile
            .targets
            .retain(|target| live_targets.contains(target));
        if profile.id == model.default_check_profile
            && let Some(current) = generated
                .profiles
                .iter()
                .find(|current| current.id == generated.default_check_profile)
        {
            let mut targets = current.targets.clone();
            remap_targets(&mut targets);
            for target in targets {
                if live_targets.contains(&target) && !profile.targets.contains(&target) {
                    profile.targets.push(target);
                }
            }
        }
    }
    model.actions.sort_by(|a, b| a.target.cmp(&b.target));
    Ok((model, commands))
}

/// The prior actions that the refresh replaces with the generated ones. As on
/// update, a boundary check that generation no longer writes leaves only if
/// it is unmodified and no other target depends on it.
fn managed_targets(
    generated: &AuthoredRepositoryModel,
    generated_commands: &BTreeMap<String, String>,
    prior: &AuthoredRepositoryModel,
    prior_commands: &BTreeMap<String, String>,
) -> BTreeSet<TargetId> {
    let keeps_boundary = super::frontend_migration::keeps_saved_boundary(
        generated,
        generated_commands,
        prior,
        prior_commands,
    );
    prior
        .actions
        .iter()
        .filter(|action| {
            generated_action(action, prior)
                && !(keeps_boundary
                    && action.target.component.as_str() == REPO_COMPONENT
                    && action.target.action.as_str() == FRONTEND_PUBLIC_BOUNDARY_ACTION)
        })
        .map(|action| action.target.clone())
        .collect()
}

/// Removes the commands of the managed actions. A command that a retained
/// action still runs stays, as on update.
fn drop_managed_commands(
    prior: &AuthoredRepositoryModel,
    managed: &BTreeSet<TargetId>,
    commands: &mut BTreeMap<String, String>,
) {
    let runner_command = |action: &ActionSpec| match &action.runner {
        ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. } => {
            Some(command.clone())
        }
        _ => None,
    };
    let (managed_actions, retained_actions): (Vec<_>, Vec<_>) = prior
        .actions
        .iter()
        .partition(|action| managed.contains(&action.target));
    let retained = retained_actions
        .into_iter()
        .filter_map(runner_command)
        .collect::<BTreeSet<_>>();
    for command in managed_actions.into_iter().filter_map(runner_command) {
        if !retained.contains(&command) {
            commands.remove(&command);
        }
    }
}

/// The dependencies that an owner declared on the generated actions that a
/// refresh replaces.
fn declared_dependencies(
    prior: &AuthoredRepositoryModel,
    managed: &BTreeSet<TargetId>,
) -> BTreeMap<TargetId, Vec<TargetId>> {
    prior
        .actions
        .iter()
        .filter(|action| {
            managed.contains(&action.target)
                && action.provenance.get("depends_on") == Some(&FieldProvenance::Declared)
        })
        .map(|action| (action.target.clone(), action.depends_on.clone()))
        .collect()
}

/// Gives a regenerated action the dependencies that its owner declared on
/// the action it replaces.
fn carry_declared_dependencies(
    action: &mut ActionSpec,
    declared: &BTreeMap<TargetId, Vec<TargetId>>,
) {
    let Some(dependencies) = declared.get(&action.target) else {
        return;
    };
    action.depends_on.extend(dependencies.iter().cloned());
    action.depends_on.sort();
    action.depends_on.dedup();
    action
        .provenance
        .insert("depends_on".into(), FieldProvenance::Declared);
}

fn generated_action(action: &ActionSpec, model: &AuthoredRepositoryModel) -> bool {
    // Resource declarations describe this exact implementation, including a
    // wrapper's inner Cargo invocation. Preserve its owner and command rather
    // than discarding or transplanting the opt-in onto regenerated code.
    if !action.resources.is_empty() {
        return false;
    }
    // A policy assertion belongs to this implementation and its input set.
    // Retain the whole action, including its runner, when refreshing capabilities.
    // Transplanting the assertion onto a replacement runner would be unsound.
    if jig_repository::freshness::adoption::is_authored(
        action,
        "source_state",
        action.source_state.is_some(),
    ) || jig_repository::freshness::adoption::is_authored(
        action,
        "inputs_policy",
        action.inputs_policy.is_some(),
    ) || action.inputs_policy == Some(jig_contract::ActionInputsPolicy::Exhaustive)
    {
        return false;
    }
    // Generated target provenance and canonical command keys identify owned actions;
    // arbitrary commands and custom target IDs must survive footprint changes.
    if !matches!(
        action.provenance.get("target"),
        Some(FieldProvenance::Inherited | FieldProvenance::Inferred)
    ) && !(action.provenance.get("target") == Some(&FieldProvenance::Declared)
        && action.provenance.get("runner") == Some(&FieldProvenance::Inferred)
        && model.components.iter().any(|owner| {
            owner.id == action.target.component
                && owner.adapters.iter().any(|adapter| adapter == "typescript")
        }))
    {
        return false;
    }
    match &action.runner {
        ActionRunner::Argv { .. } | ActionRunner::RustNextestV1 { .. } => false,
        ActionRunner::Native { operation, .. } => matches!(
            operation.as_str(),
            tool::CONTRACT_CHECK | tool::FILE_BUDGET | tool::SCHEMA_CHECK | tool::MIGRATION_ADD
        ),
        ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. } => {
            let actual_component = action.target.component.as_str();
            let component = if command.starts_with("api_")
                && model
                    .components
                    .iter()
                    .any(|owner| owner.id == action.target.component && owner.root == ".")
            {
                "api"
            } else {
                actual_component
            };
            let scope = if command.starts_with(&format!("{component}_compat_")) {
                CommandScope::Compatibility
            } else {
                CommandScope::Component
            };
            scope
                .command_key(component, action.target.action.as_str())
                .is_ok_and(|expected| expected == *command)
                && model.components.iter().any(|owner| {
                    owner.id == action.target.component
                        && owner.adapters.iter().any(|adapter| {
                            ["jig", "rust", "go", "sqlx", "typescript"].contains(&adapter.as_str())
                        })
                })
        }
    }
}
