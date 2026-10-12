use super::{
    ActionRunner, ActionSpec, AuthoredRepositoryModel, BACKEND_COMPONENT, BACKEND_CONTRACT_INPUTS,
    BOUNDARY_DESCRIPTION, BTreeMap, CLIENT_DRIFT_DESCRIPTION, CommandScope,
    FRONTEND_CONTRACT_DRIFT_ACTION, FRONTEND_PUBLIC_BOUNDARY_ACTION, FieldProvenance,
    HISTORICAL_DRIFT_DESCRIPTION, OPENAPI_DOCUMENT_CHECK_ACTION, OPENAPI_DOCUMENT_COMMAND_KEY,
    PUBLIC_DOCUMENT_INPUTS, REPO_COMPONENT, RepositoryRenderModel, TargetId, contract_check_action,
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

// Backend inputs the contract checks read while drift exported the documents.
const LEGACY_BACKEND_INPUTS: &[&str] = &[
    "Cargo.toml",
    "**/Cargo.toml",
    "**/*.rs",
    "go.mod",
    "**/go.mod",
    "**/*.go",
];

// Go scaffolds once generated an artifact-only boundary check.
const LEGACY_GO_BOUNDARY_DESCRIPTION: &str =
    "Check public frontend manifests and artifacts for privileged markers.";

/// The drift check marks a scaffold with frontend contracts; the boundary
/// check exists only alongside an admin API.
pub(super) fn contracts_enabled(
    actions: &[ActionSpec],
    commands: &BTreeMap<String, String>,
) -> bool {
    declares_contract_check(
        actions,
        commands,
        FRONTEND_CONTRACT_DRIFT_ACTION,
        "contracts-drift-check",
    )
}

/// A saved boundary check keeps guarding a Rust admin API that its
/// repository may still have without an admin frontend app.
pub(super) fn public_boundary_enabled(
    actions: &[ActionSpec],
    commands: &BTreeMap<String, String>,
) -> bool {
    declares_contract_check(
        actions,
        commands,
        FRONTEND_PUBLIC_BOUNDARY_ACTION,
        "contracts-boundary-check",
    )
}

fn declares_contract_check(
    actions: &[ActionSpec],
    commands: &BTreeMap<String, String>,
    action: &str,
    mode: &str,
) -> bool {
    actions.iter().any(|candidate| {
        is_repo_target(&candidate.target, action)
            && command(&candidate.runner)
                .and_then(|key| commands.get(key))
                .is_some_and(|value| value.contains(mode))
    })
}

/// Whether the saved drift check was generated beside the client-only
/// contract script, which never reads the backend.
pub(super) fn client_only_drift(actions: &[ActionSpec]) -> bool {
    actions.iter().any(|action| {
        is_repo_target(&action.target, FRONTEND_CONTRACT_DRIFT_ACTION)
            && action.description.as_deref() == Some(CLIENT_DRIFT_DESCRIPTION)
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

/// Whether the saved model predates the document check, so that update and
/// re-adoption may give it the check: it has API contracts whose drift check
/// keeps the description that generators wrote before the check, no document
/// check, and the check's command key free. Any other model without the check
/// is one whose owner removed it, which stays removed.
pub(super) fn predates_document_check(
    actions: &[ActionSpec],
    commands: &BTreeMap<String, String>,
    generated_command: &str,
) -> bool {
    contracts_enabled(actions, commands)
        && actions.iter().any(|action| {
            is_repo_target(&action.target, FRONTEND_CONTRACT_DRIFT_ACTION)
                && action.description.as_deref() == Some(HISTORICAL_DRIFT_DESCRIPTION)
        })
        && !actions
            .iter()
            .any(|action| action.target.action.as_str() == OPENAPI_DOCUMENT_CHECK_ACTION)
        && commands
            .get(OPENAPI_DOCUMENT_COMMAND_KEY)
            .is_none_or(|value| value == generated_command)
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
    let Some(drift) = current
        .actions
        .iter()
        .find(|action| is_repo_target(&action.target, FRONTEND_CONTRACT_DRIFT_ACTION))
    else {
        return upgraded;
    };
    // The historical description marks a model that predates the document
    // check, so it stays until the model has one.
    let documents_checked = upgraded
        .actions
        .iter()
        .any(|action| action.target.action.as_str() == OPENAPI_DOCUMENT_CHECK_ACTION);
    for action in &mut upgraded.actions {
        let id = action.target.action.as_str();
        if action.target.component.as_str() != REPO_COMPONENT
            || !matches!(
                id,
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
            || !generated_input_history(id, &drift.inputs).contains(&action.inputs)
        {
            continue;
        }
        if documents_checked && action.description.as_deref() == Some(HISTORICAL_DRIFT_DESCRIPTION)
        {
            action.description.clone_from(&expected.description);
        }
        action.inputs.clone_from(&expected.inputs);
    }
    if !current
        .actions
        .iter()
        .any(|action| is_repo_target(&action.target, FRONTEND_PUBLIC_BOUNDARY_ACTION))
    {
        retire_generated_boundary(current, drift, &mut upgraded, commands);
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

/// Inputs that earlier generators wrote for a contract check: the contract
/// surface plus the backend inputs of their time, and for the boundary check
/// the public documents.
fn generated_input_history(action: &str, drift_inputs: &[String]) -> Vec<Vec<String>> {
    let surface = contract_surface(drift_inputs);
    let documents: &[&str] = if action == FRONTEND_PUBLIC_BOUNDARY_ACTION {
        PUBLIC_DOCUMENT_INPUTS
    } else {
        &[]
    };
    vec![
        with_inputs(&surface, &[LEGACY_BACKEND_INPUTS, documents]),
        with_inputs(
            &surface,
            &[LEGACY_BACKEND_INPUTS, CONTRACT_RESOLUTION_INPUTS, documents],
        ),
    ]
}

/// Go scaffolds have no admin API, so their boundary check guarded nothing.
/// Drop an unmodified generated one with its profile entries, unless another
/// target still depends on it.
fn retire_generated_boundary(
    current: &RepositoryRenderModel,
    drift: &ActionSpec,
    upgraded: &mut AuthoredRepositoryModel,
    commands: &BTreeMap<String, String>,
) {
    let Some(index) = upgraded
        .actions
        .iter()
        .position(|action| is_repo_target(&action.target, FRONTEND_PUBLIC_BOUNDARY_ACTION))
    else {
        return;
    };
    let saved = &upgraded.actions[index];
    let expected_command = command(&drift.runner)
        .and_then(|key| current.commands.get(key))
        .map(|value| value.replacen("contracts-drift-check", "contracts-boundary-check", 1));
    let generated = [BOUNDARY_DESCRIPTION, LEGACY_GO_BOUNDARY_DESCRIPTION]
        .into_iter()
        .flat_map(|description| {
            generated_input_history(FRONTEND_PUBLIC_BOUNDARY_ACTION, &drift.inputs)
                .into_iter()
                .filter_map(move |inputs| {
                    contract_check_action(FRONTEND_PUBLIC_BOUNDARY_ACTION, description, inputs).ok()
                })
        })
        .any(|candidate| {
            super::freshness::matches_generated_actions(
                std::slice::from_ref(&candidate),
                std::slice::from_ref(saved),
                commands,
            )
        });
    let depended_on = upgraded
        .actions
        .iter()
        .any(|action| action.depends_on.contains(&saved.target));
    if !generated
        || depended_on
        || command(&saved.runner).and_then(|key| commands.get(key)) != expected_command.as_ref()
    {
        return;
    }
    let target = upgraded.actions.remove(index).target;
    for profile in &mut upgraded.profiles {
        profile.targets.retain(|candidate| candidate != &target);
    }
}

/// Removes the command of a retired generated boundary check.
pub(super) fn drop_retired_contract_commands(
    actions: &[ActionSpec],
    commands: &mut BTreeMap<String, String>,
) {
    let Ok(key) =
        CommandScope::Component.command_key(REPO_COMPONENT, FRONTEND_PUBLIC_BOUNDARY_ACTION)
    else {
        return;
    };
    if !actions
        .iter()
        .any(|action| command(&action.runner) == Some(key.as_str()))
        && commands
            .get(&key)
            .is_some_and(|value| value.contains("contracts-boundary-check"))
    {
        commands.remove(&key);
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

fn is_repo_target(target: &TargetId, action: &str) -> bool {
    target.component.as_str() == REPO_COMPONENT && target.action.as_str() == action
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
