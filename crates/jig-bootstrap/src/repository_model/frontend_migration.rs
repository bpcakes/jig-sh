use jig_repository::shell::optional_cargo_command;

use super::{
    ActionRunner, ActionSpec, AuthoredRepositoryModel, BACKEND_COMPONENT, BACKEND_CONTRACT_INPUTS,
    BOUNDARY_DESCRIPTION, BTreeMap, CommandScope, FRONTEND_CONTRACT_DRIFT_ACTION,
    FRONTEND_PUBLIC_BOUNDARY_ACTION, FieldProvenance, OPENAPI_INPUT, PUBLIC_DOCUMENT_INPUTS,
    REPO_COMPONENT, RepositoryRenderModel, TargetId, contract_check_action,
    retire_saved_frontend_dependencies,
};
use crate::answers::{
    DEFAULT_GO_TEST_COMMAND, DEFAULT_GO_TEST_LOCKED_COMMAND, DEFAULT_RUST_TEST_COMMAND,
    DEFAULT_RUST_TEST_LOCKED_COMMAND,
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
    upgrade_backend_contract_tests(current, &mut upgraded.actions);
    let tests_check_documents =
        backend_contract_tests_check_documents(current, &upgraded.actions, commands);
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
        if runner != expected_runner
            || command(&runner).and_then(|key| commands.get(key))
                != command(&expected_runner).and_then(|key| current.commands.get(key))
        {
            continue;
        }
        // Dropping backend inputs from the drift check is safe only while the
        // saved backend tests prove the documents match the backend on every
        // backend and document change.
        if id == FRONTEND_CONTRACT_DRIFT_ACTION
            && !tests_check_documents
            && action
                .inputs
                .iter()
                .any(|input| !expected.inputs.contains(input))
        {
            continue;
        }
        if generated_input_history(id, &drift.inputs).contains(&action.inputs) {
            action.inputs.clone_from(&expected.inputs);
        }
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

/// Inputs that generators wrote for a contract check: the contract surface
/// plus the backend inputs of their time, and for the boundary check the
/// public documents. A drift check may already read only the surface.
fn generated_input_history(action: &str, drift_inputs: &[String]) -> Vec<Vec<String>> {
    let surface = drift_inputs
        .iter()
        .filter(|input| !BACKEND_CONTRACT_INPUTS.contains(&input.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let with = |extra: &[&[&str]]| {
        let mut inputs = surface
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
    };
    if action == FRONTEND_PUBLIC_BOUNDARY_ACTION {
        return vec![
            with(&[LEGACY_BACKEND_INPUTS, PUBLIC_DOCUMENT_INPUTS]),
            with(&[
                LEGACY_BACKEND_INPUTS,
                CONTRACT_RESOLUTION_INPUTS,
                PUBLIC_DOCUMENT_INPUTS,
            ]),
        ];
    }
    vec![
        with(&[]),
        with(&[LEGACY_BACKEND_INPUTS]),
        with(&[LEGACY_BACKEND_INPUTS, CONTRACT_RESOLUTION_INPUTS]),
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

fn is_repo_target(target: &TargetId, action: &str) -> bool {
    target.component.as_str() == REPO_COMPONENT && target.action.as_str() == action
}

fn is_backend_contract_test(target: &TargetId) -> bool {
    target.component.as_str() == BACKEND_COMPONENT
        && matches!(target.action.as_str(), "test" | "test-locked")
}

/// Add the document input to backend tests that still read the adapter's
/// generated inputs.
fn upgrade_backend_contract_tests(current: &RepositoryRenderModel, actions: &mut [ActionSpec]) {
    for action in actions
        .iter_mut()
        .filter(|action| is_backend_contract_test(&action.target))
    {
        let Some(expected) = current
            .actions
            .iter()
            .find(|item| item.target == action.target)
        else {
            continue;
        };
        let legacy = expected
            .inputs
            .iter()
            .filter(|input| input.as_str() != OPENAPI_INPUT)
            .cloned()
            .collect::<Vec<_>>();
        if action.provenance.get("inputs") == Some(&FieldProvenance::Inherited)
            && action.inputs == legacy
            && legacy.len() < expected.inputs.len()
        {
            action.inputs.clone_from(&expected.inputs);
            action
                .provenance
                .insert("inputs".into(), FieldProvenance::Inferred);
        }
    }
}

/// Whether every backend test runs the generator's default command, which
/// includes the committed-document tests, so that the backend tests own
/// document freshness. Generation and the saved-model upgrade share this rule.
pub(super) fn backend_tests_run_generated_commands<'a>(
    actions: impl IntoIterator<Item = &'a ActionSpec>,
    commands: &BTreeMap<String, String>,
    go: bool,
) -> bool {
    let mut tests = actions
        .into_iter()
        .filter(|action| is_backend_contract_test(&action.target))
        .peekable();
    tests.peek().is_some()
        && tests.all(|action| {
            command(&action.runner).and_then(|key| commands.get(key))
                == generated_backend_test_command(go, action.target.action.as_str()).as_ref()
        })
}

/// Whether the saved backend tests prove the documents fresh: each runs its
/// generated runner and command and reads every generated input, which
/// covers the backend and the documents.
fn backend_contract_tests_check_documents(
    current: &RepositoryRenderModel,
    actions: &[ActionSpec],
    commands: &BTreeMap<String, String>,
) -> bool {
    let go = current.components.iter().any(|component| {
        component.id.as_str() == BACKEND_COMPONENT
            && component.adapters.iter().any(|adapter| adapter == "go")
    });
    backend_tests_run_generated_commands(actions, commands, go)
        && actions
            .iter()
            .filter(|action| is_backend_contract_test(&action.target))
            .all(|action| {
                let Some(expected) = current
                    .actions
                    .iter()
                    .find(|item| item.target == action.target)
                else {
                    return false;
                };
                let mut runner = action.runner.clone();
                let mut expected_runner = expected.runner.clone();
                super::runners::make_shell_explicit(&mut runner);
                super::runners::make_shell_explicit(&mut expected_runner);
                runner == expected_runner
                    && expected
                        .inputs
                        .iter()
                        .all(|input| action.inputs.contains(input))
            })
}

/// The command the generator writes for a backend test.
fn generated_backend_test_command(go: bool, action: &str) -> Option<String> {
    match (go, action) {
        (false, "test") => Some(optional_cargo_command(DEFAULT_RUST_TEST_COMMAND, "test")),
        (false, "test-locked") => Some(optional_cargo_command(
            DEFAULT_RUST_TEST_LOCKED_COMMAND,
            "test-locked",
        )),
        (true, "test") => Some(DEFAULT_GO_TEST_COMMAND.into()),
        (true, "test-locked") => Some(DEFAULT_GO_TEST_LOCKED_COMMAND.into()),
        _ => None,
    }
}

fn command(runner: &ActionRunner) -> Option<&str> {
    match runner {
        ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. } => {
            Some(command)
        }
        _ => None,
    }
}
