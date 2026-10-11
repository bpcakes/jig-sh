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
    let documents_checked = generated_document_check(current, &upgraded.actions, commands);
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
/// and its command as saved; a custom model keeps its own targets.
pub(super) fn with_generated_document_check(
    current: &RepositoryRenderModel,
    saved: &AuthoredRepositoryModel,
    commands: &BTreeMap<String, String>,
) -> Option<(AuthoredRepositoryModel, BTreeMap<String, String>)> {
    let check = current
        .actions
        .iter()
        .find(|action| is_document_check(&action.target))?;
    if saved
        .actions
        .iter()
        .any(|action| action.target == check.target)
    {
        return None;
    }
    let key = command(&check.runner)?;
    let mut adopted = saved.clone();
    let index = adopted
        .actions
        .iter()
        .position(|action| action.target > check.target)
        .unwrap_or(adopted.actions.len());
    adopted.actions.insert(index, check.clone());
    for profile in &mut adopted.profiles {
        if let Some(generated) = current.profiles.iter().find(|item| item.id == profile.id)
            && generated
                .targets
                .iter()
                .filter(|target| **target != check.target)
                .eq(&profile.targets)
        {
            profile.targets.clone_from(&generated.targets);
        }
    }
    let mut adopted_commands = commands.clone();
    adopted_commands.insert(key.into(), current.commands.get(key)?.clone());
    Some((adopted, adopted_commands))
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

/// Whether the saved model has the generated document check: its runner and
/// command, reading at least the generated inputs.
fn generated_document_check(
    current: &RepositoryRenderModel,
    actions: &[ActionSpec],
    commands: &BTreeMap<String, String>,
) -> bool {
    let Some(expected) = current
        .actions
        .iter()
        .find(|action| is_document_check(&action.target))
    else {
        return false;
    };
    actions.iter().any(|action| {
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
    })
}

fn command(runner: &ActionRunner) -> Option<&str> {
    match runner {
        ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. } => {
            Some(command)
        }
        _ => None,
    }
}
