use super::{
    ActionRunner, ActionSpec, AuthoredRepositoryModel, BACKEND_COMPONENT, BTreeMap,
    FRONTEND_CONTRACT_DRIFT_ACTION, FRONTEND_PUBLIC_BOUNDARY_ACTION, FieldProvenance,
    OPENAPI_INPUT, REPO_COMPONENT, RepositoryRenderModel, TargetId,
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
    upgrade_backend_contract_tests(current, &mut upgraded.actions);
    // Narrowing the drift check is safe only while the backend tests, which
    // prove the documents match the backend, run on document edits.
    let tests_read_documents = backend_contract_tests_read_documents(&upgraded.actions);
    for action in &mut upgraded.actions {
        if action.target.component.as_str() != REPO_COMPONENT
            || !matches!(
                action.target.action.as_str(),
                FRONTEND_CONTRACT_DRIFT_ACTION | FRONTEND_PUBLIC_BOUNDARY_ACTION
            )
            || action.provenance.get("inputs") != Some(&FieldProvenance::Inferred)
            || (action.target.action.as_str() == FRONTEND_CONTRACT_DRIFT_ACTION
                && !tests_read_documents)
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
        if generated_input_history(action.target.action.as_str(), &expected.inputs)
            .contains(&action.inputs)
        {
            action.inputs.clone_from(&expected.inputs);
        }
    }
    upgraded
}

/// Inputs that earlier generators wrote for a contract check.
fn generated_input_history(action: &str, expected: &[String]) -> Vec<Vec<String>> {
    let with = |extra: &[&[&str]]| {
        let mut inputs = expected
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
    match action {
        FRONTEND_CONTRACT_DRIFT_ACTION => vec![
            with(&[LEGACY_DRIFT_BACKEND_INPUTS]),
            with(&[LEGACY_DRIFT_BACKEND_INPUTS, CONTRACT_RESOLUTION_INPUTS]),
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

fn backend_contract_tests_read_documents(actions: &[ActionSpec]) -> bool {
    let mut tests = actions
        .iter()
        .filter(|action| is_backend_contract_test(&action.target))
        .peekable();
    tests.peek().is_some()
        && tests.all(|action| action.inputs.iter().any(|input| input == OPENAPI_INPUT))
}

fn command(runner: &ActionRunner) -> Option<&str> {
    match runner {
        ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. } => {
            Some(command)
        }
        _ => None,
    }
}
