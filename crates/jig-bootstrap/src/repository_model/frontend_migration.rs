use std::collections::BTreeSet;

use super::{
    ActionRunner, ActionSpec, AuthoredRepositoryModel, BACKEND_COMPONENT, BACKEND_CONTRACT_INPUTS,
    BTreeMap, FRONTEND_CONTRACT_DRIFT_ACTION, FRONTEND_PUBLIC_BOUNDARY_ACTION, FieldProvenance,
    OPENAPI_DOCUMENT_CHECK_ACTION, REPO_COMPONENT, RepositoryRenderModel, TargetId,
    retire_saved_frontend_dependencies,
};

// Lockfile and workspace inputs the contract checks gained when frontends
// stopped depending on the backend.
const CONTRACT_RESOLUTION_INPUTS: &[&str] = &[
    "Cargo.lock",
    "**/Cargo.lock",
    "go.sum",
    "**/go.sum",
    "go.work",
    "**/go.work",
    "go.work.sum",
    "**/go.work.sum",
    "vendor/modules.txt",
    "**/vendor/modules.txt",
];

// Backend inputs the drift check read while it exported the documents itself.
const LEGACY_DRIFT_BACKEND_INPUTS: &[&str] = &[
    "Cargo.toml",
    "**/Cargo.toml",
    "**/*.rs",
    "go.mod",
    "**/go.mod",
    "**/*.go",
];

pub(super) fn contracts_enabled(
    actions: &[ActionSpec],
    commands: &BTreeMap<String, String>,
) -> bool {
    [
        (FRONTEND_CONTRACT_DRIFT_ACTION, "contracts-drift-check"),
        (FRONTEND_PUBLIC_BOUNDARY_ACTION, "contracts-boundary-check"),
    ]
    .into_iter()
    .all(|(action, mode)| {
        actions.iter().any(|candidate| {
            candidate.target.component.as_str() == REPO_COMPONENT
                && candidate.target.action.as_str() == action
                && command(&candidate.runner)
                    .and_then(|key| commands.get(key))
                    .is_some_and(|value| value.contains(mode))
        })
    })
}

/// Normalize only generated legacy fields, both for custom-model preservation
/// and for deciding whether a saved scaffold can be regenerated on update.
pub(super) fn upgrade_saved_projection(
    current: &RepositoryRenderModel,
    saved: &AuthoredRepositoryModel,
    commands: &BTreeMap<String, String>,
) -> AuthoredRepositoryModel {
    let mut upgraded = saved.clone();
    retire_saved_frontend_dependencies(&mut upgraded.components, &mut upgraded.actions);
    if !current.frontend_contracts_enabled() || !saved.frontend_contracts_enabled(commands) {
        return upgraded;
    }
    // The drift check stops reading the backend only behind the generated
    // document check, which proves that the documents match the backend.
    let documents_checked = generated_document_check(current, &upgraded, commands);
    for action in &mut upgraded.actions {
        if action.target.component.as_str() != REPO_COMPONENT
            || !matches!(
                action.target.action.as_str(),
                FRONTEND_CONTRACT_DRIFT_ACTION | FRONTEND_PUBLIC_BOUNDARY_ACTION
            )
            || action.provenance.get("inputs") != Some(&FieldProvenance::Inferred)
        {
            continue;
        }
        let Some(expected) = current
            .actions
            .iter()
            .find(|item| item.target == action.target)
        else {
            continue;
        };
        let mut runner = action.runner.clone();
        let mut expected_runner = expected.runner.clone();
        super::runners::make_shell_explicit(&mut runner);
        super::runners::make_shell_explicit(&mut expected_runner);
        if runner != expected_runner
            || command(&runner).and_then(|key| commands.get(key))
                != command(&expected_runner).and_then(|key| current.commands.get(key))
        {
            continue;
        }
        let history = generated_input_history(action.target.action.as_str(), &expected.inputs);
        if !history.contains(&action.inputs) {
            continue;
        }
        action.inputs = if action.target.action.as_str() == FRONTEND_CONTRACT_DRIFT_ACTION
            && !documents_checked
        {
            with_inputs(&expected.inputs, &[BACKEND_CONTRACT_INPUTS])
        } else {
            expected.inputs.clone()
        };
    }
    upgraded
}

/// Earlier generators had no document check. To decide whether a saved
/// scaffold can be regenerated, count the generated one, its profile entries,
/// and its command as saved. Only a model from before the check qualifies: its
/// drift check still reads the backend in a generated shape, and the check's
/// command key is free. A model whose owner removed the check, or reused its
/// command key, keeps its own targets.
pub(super) fn with_generated_document_check(
    current: &RepositoryRenderModel,
    saved: &AuthoredRepositoryModel,
    commands: &BTreeMap<String, String>,
) -> Option<(AuthoredRepositoryModel, BTreeMap<String, String>)> {
    let check = current
        .actions
        .iter()
        .find(|action| is_document_check(&action.target))?;
    let key = command(&check.runner)?;
    let generated_command = current.commands.get(key)?;
    let drift = current.actions.iter().find(|action| {
        action.target.component.as_str() == REPO_COMPONENT
            && action.target.action.as_str() == FRONTEND_CONTRACT_DRIFT_ACTION
    })?;
    let history = generated_input_history(FRONTEND_CONTRACT_DRIFT_ACTION, &drift.inputs);
    if saved
        .actions
        .iter()
        .any(|action| action.target == check.target)
        || commands
            .get(key)
            .is_some_and(|value| value != generated_command)
        || !saved
            .actions
            .iter()
            .any(|action| action.target == drift.target && history.contains(&action.inputs))
    {
        return None;
    }
    let mut adopted = saved.clone();
    insert_sorted(&mut adopted.actions, check.clone(), |action| &action.target);
    for profile in &mut adopted.profiles {
        if current
            .profiles
            .iter()
            .any(|item| item.id == profile.id && item.targets.contains(&check.target))
        {
            insert_sorted(&mut profile.targets, check.target.clone(), |target| target);
        }
    }
    let mut adopted_commands = commands.clone();
    adopted_commands.insert(key.into(), generated_command.clone());
    Some((adopted, adopted_commands))
}

/// Generated actions and profile targets are sorted by target.
fn insert_sorted<T>(items: &mut Vec<T>, item: T, target: impl Fn(&T) -> &TargetId) {
    let index = items
        .iter()
        .position(|candidate| target(candidate) > target(&item))
        .unwrap_or(items.len());
    items.insert(index, item);
}

/// Inputs that earlier generators wrote for a contract check.
fn generated_input_history(action: &str, expected: &[String]) -> Vec<Vec<String>> {
    match action {
        FRONTEND_CONTRACT_DRIFT_ACTION => vec![
            with_inputs(expected, &[LEGACY_DRIFT_BACKEND_INPUTS]),
            with_inputs(expected, &[BACKEND_CONTRACT_INPUTS]),
        ],
        _ => vec![
            expected
                .iter()
                .filter(|input| !CONTRACT_RESOLUTION_INPUTS.contains(&input.as_str()))
                .cloned()
                .collect(),
        ],
    }
}

fn with_inputs(inputs: &[String], extra: &[&[&str]]) -> Vec<String> {
    let mut inputs = inputs
        .iter()
        .cloned()
        .chain(
            extra
                .iter()
                .flat_map(|set| set.iter().map(|input| (*input).to_owned())),
        )
        .collect::<Vec<_>>();
    inputs.sort();
    inputs.dedup();
    inputs
}

fn is_document_check(target: &TargetId) -> bool {
    target.component.as_str() == BACKEND_COMPONENT
        && target.action.as_str() == OPENAPI_DOCUMENT_CHECK_ACTION
}

/// Whether the saved model has the generated document check (a read-only
/// check with its runner and command, reading at least the generated inputs)
/// and every profile that runs the drift check, directly or through
/// dependencies, also runs it.
fn generated_document_check(
    current: &RepositoryRenderModel,
    saved: &AuthoredRepositoryModel,
    commands: &BTreeMap<String, String>,
) -> bool {
    let Some(expected) = current
        .actions
        .iter()
        .find(|action| is_document_check(&action.target))
    else {
        return false;
    };
    let Some(drift) = current.actions.iter().find(|action| {
        action.target.component.as_str() == REPO_COMPONENT
            && action.target.action.as_str() == FRONTEND_CONTRACT_DRIFT_ACTION
    }) else {
        return false;
    };
    let generated = saved.actions.iter().any(|action| {
        let mut runner = action.runner.clone();
        let mut expected_runner = expected.runner.clone();
        super::runners::make_shell_explicit(&mut runner);
        super::runners::make_shell_explicit(&mut expected_runner);
        action.target == expected.target
            && runner == expected_runner
            && command(&runner).and_then(|key| commands.get(key))
                == command(&expected_runner).and_then(|key| current.commands.get(key))
            && expected
                .inputs
                .iter()
                .all(|input| action.inputs.contains(input))
    });
    generated
        && jig_repository::validate_read_only_check_closure(
            &saved.actions,
            std::iter::once(&expected.target),
        )
        .is_ok()
        && saved
            .profiles
            .iter()
            .filter(|profile| reaches(&saved.actions, &profile.targets, &drift.target))
            .all(|profile| reaches(&saved.actions, &profile.targets, &expected.target))
}

/// Whether `target` is among `targets` or their dependencies.
fn reaches(actions: &[ActionSpec], targets: &[TargetId], target: &TargetId) -> bool {
    let mut pending = targets.to_vec();
    let mut seen = BTreeSet::new();
    while let Some(candidate) = pending.pop() {
        if candidate == *target {
            return true;
        }
        if seen.insert(candidate.clone())
            && let Some(action) = actions.iter().find(|action| action.target == candidate)
        {
            pending.extend(action.depends_on.iter().cloned());
        }
    }
    false
}

fn command(runner: &ActionRunner) -> Option<&str> {
    match runner {
        ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. } => {
            Some(command)
        }
        _ => None,
    }
}
