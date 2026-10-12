use super::{
    ActionRunner, ActionSpec, AuthoredRepositoryModel, BACKEND_COMPONENT, BACKEND_CONTRACT_INPUTS,
    BTreeMap, CLIENT_DRIFT_DESCRIPTION, FRONTEND_CONTRACT_DRIFT_ACTION,
    FRONTEND_PUBLIC_BOUNDARY_ACTION, FieldProvenance, HISTORICAL_DRIFT_DESCRIPTION,
    OPENAPI_DOCUMENT_CHECK_ACTION, OPENAPI_DOCUMENT_COMMAND_KEY, REPO_COMPONENT,
    RepositoryRenderModel, TargetId, retire_saved_frontend_dependencies,
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

/// Whether the saved drift check was generated beside the client-only
/// contract script, which never reads the backend.
pub(super) fn client_only_drift(actions: &[ActionSpec]) -> bool {
    actions.iter().any(|action| {
        is_drift(&action.target) && action.description.as_deref() == Some(CLIENT_DRIFT_DESCRIPTION)
    })
}

/// Whether the saved model runs a document check through the generated
/// check's command key, whose command generation then keeps, as it keeps the
/// other backend commands. A key that no document check runs belongs to the
/// owner, so generation never adopts it.
pub(super) fn has_document_check(actions: &[ActionSpec]) -> bool {
    actions.iter().any(|action| {
        action.target.action.as_str() == OPENAPI_DOCUMENT_CHECK_ACTION
            && command(&action.runner) == Some(OPENAPI_DOCUMENT_COMMAND_KEY)
    })
}

/// Whether the saved model has not gained the document check: it has API
/// contracts whose drift check keeps the description that generators wrote
/// before the check, and no document check. Any other model without the
/// check is one whose owner removed it, which stays removed.
pub(super) fn lacks_document_check(
    actions: &[ActionSpec],
    commands: &BTreeMap<String, String>,
) -> bool {
    contracts_enabled(actions, commands)
        && actions.iter().any(|action| {
            is_drift(&action.target)
                && action.description.as_deref() == Some(HISTORICAL_DRIFT_DESCRIPTION)
        })
        && !actions
            .iter()
            .any(|action| action.target.action.as_str() == OPENAPI_DOCUMENT_CHECK_ACTION)
}

/// Whether the saved model predates the document check, so that update and
/// re-adoption may give it the check: it lacks the check, and the check's
/// command key is free.
pub(super) fn predates_document_check(
    actions: &[ActionSpec],
    commands: &BTreeMap<String, String>,
    generated_command: &str,
) -> bool {
    lacks_document_check(actions, commands)
        && commands
            .get(OPENAPI_DOCUMENT_COMMAND_KEY)
            .is_none_or(|value| value == generated_command)
}

/// Whether a saved contract check reads other inputs than generation gives it
/// now. A capability refresh, which cannot change those inputs, keeps such a
/// check, as its owner may have changed them; loading the model then updates
/// one that earlier generators wrote, as an update does.
pub(super) fn customized_contract_inputs(saved: &ActionSpec, generated: &ActionSpec) -> bool {
    saved.target.component.as_str() == REPO_COMPONENT
        && matches!(
            saved.target.action.as_str(),
            FRONTEND_CONTRACT_DRIFT_ACTION | FRONTEND_PUBLIC_BOUNDARY_ACTION
        )
        && saved.inputs != generated.inputs
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
    // The historical description marks a model that predates the document
    // check, so it stays until the model has one.
    let documents_checked = upgraded
        .actions
        .iter()
        .any(|action| action.target.action.as_str() == OPENAPI_DOCUMENT_CHECK_ACTION);
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
        // A client-only drift check never had the earlier backend inputs, so
        // its owner added any it now has.
        if runner != expected_runner
            || command(&runner).and_then(|key| commands.get(key))
                != command(&expected_runner).and_then(|key| current.commands.get(key))
            || action.description.as_deref() == Some(CLIENT_DRIFT_DESCRIPTION)
            || !generated_input_history(action.target.action.as_str(), &expected.inputs)
                .contains(&action.inputs)
        {
            continue;
        }
        if documents_checked && action.description.as_deref() == Some(HISTORICAL_DRIFT_DESCRIPTION)
        {
            action.description.clone_from(&expected.description);
        }
        action.inputs.clone_from(&expected.inputs);
    }
    upgraded
}

/// Earlier generators had no document check. To decide whether a saved
/// scaffold can be regenerated, count the generated one, its profile entries,
/// and its command as saved. Only a model that predates the check qualifies;
/// a model whose owner removed the check, or reused its command key, keeps
/// its own targets.
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
    if !predates_document_check(&saved.actions, commands, generated_command) {
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

/// Inputs that earlier generators wrote for a contract check. Every drift
/// shape is the contract surface plus the backend inputs of its time.
fn generated_input_history(action: &str, expected: &[String]) -> Vec<Vec<String>> {
    let surface = contract_surface(expected);
    match action {
        FRONTEND_CONTRACT_DRIFT_ACTION => vec![
            with_inputs(&surface, &[LEGACY_DRIFT_BACKEND_INPUTS]),
            with_inputs(&surface, &[BACKEND_CONTRACT_INPUTS]),
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

/// The contract inputs of a drift check, without the backend.
fn contract_surface(inputs: &[String]) -> Vec<String> {
    inputs
        .iter()
        .filter(|input| !BACKEND_CONTRACT_INPUTS.contains(&input.as_str()))
        .cloned()
        .collect()
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

fn is_drift(target: &TargetId) -> bool {
    target.component.as_str() == REPO_COMPONENT
        && target.action.as_str() == FRONTEND_CONTRACT_DRIFT_ACTION
}

fn is_document_check(target: &TargetId) -> bool {
    target.component.as_str() == BACKEND_COMPONENT
        && target.action.as_str() == OPENAPI_DOCUMENT_CHECK_ACTION
}

fn command(runner: &ActionRunner) -> Option<&str> {
    match runner {
        ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. } => {
            Some(command)
        }
        _ => None,
    }
}
