use super::*;
use crate::bootstrap::adopt_infer::ComponentCandidates;
use crate::bootstrap::adopt_infer::components::Ecosystem;

/// Reconfigure generated harness capabilities while retaining authored component
/// identities and project-owned actions. The caller only uses this after an
/// explicit footprint or capability change during readoption.
pub(in crate::bootstrap) fn refresh(
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
    let mut managed = BTreeSet::new();
    for action in &prior.actions {
        if generated_action(action, prior) {
            managed.insert(action.target.clone());
            if let ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. } =
                &action.runner
            {
                commands.remove(command);
            }
        }
    }
    model
        .actions
        .retain(|action| !managed.contains(&action.target));
    let mut retained_owners = BTreeMap::new();
    for action in generated.actions {
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
        if let ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. } =
            &action.runner
            && let Some(value) = generated_commands.get(command)
        {
            commands
                .entry(command.clone())
                .or_insert_with(|| value.clone());
        }
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

fn generated_action(action: &ActionSpec, model: &AuthoredRepositoryModel) -> bool {
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
        ActionRunner::Argv { .. } => false,
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
