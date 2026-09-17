//! Conservative recommendations, shared by inspection and generation.
//! Recognizing a command's Git independence does not prove its input coverage.

use jig_contract::{
    ActionEffect, ActionInputsPolicy, ActionIntent, ActionRunner, ActionSourceState, ActionSpec,
    ArgvValue, FieldProvenance, TargetId,
};
use serde::Serialize;

mod patch;
mod report;
#[cfg(test)]
use report::report;
pub(crate) use report::{Request, format_report, preview};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct Policy {
    pub(crate) source_state: ActionSourceState,
    pub(crate) inputs_policy: ActionInputsPolicy,
}

impl Policy {
    pub(crate) fn of(action: &ActionSpec) -> Self {
        Self {
            source_state: action.source_state.unwrap_or_default(),
            inputs_policy: action.inputs_policy.unwrap_or_default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Reason {
    UnsupportedEpoch,
    NativeAuthority,
    NotReadOnlyCheck,
    AuthoredSourcePolicy,
    AlreadyWorktree,
    KnownFormatter,
    UnknownCommand,
    OwnerAssertion,
}

#[derive(Debug, Serialize)]
pub(crate) struct Recommendation {
    pub(crate) target: TargetId,
    pub(crate) current: Policy,
    pub(crate) proposed: Policy,
    pub(crate) reason: Reason,
    pub(crate) inputs: Vec<String>,
    pub(crate) proposed_inputs: Vec<String>,
    pub(crate) exhaustive_requires_owner_assertion: bool,
}

pub(crate) fn recommend(
    epoch: u32,
    action: &ActionSpec,
    resolved_command: Option<&str>,
) -> Recommendation {
    let current = Policy::of(action);
    let reason = if epoch < jig_contract::freshness::WORKTREE_FRESHNESS_CONTRACT_VERSION {
        Reason::UnsupportedEpoch
    } else if matches!(action.runner, ActionRunner::Native { .. }) {
        Reason::NativeAuthority
    } else if !is_read_only_check(action) {
        Reason::NotReadOnlyCheck
    } else if current.source_state == ActionSourceState::Worktree {
        Reason::AlreadyWorktree
    } else if is_authored(action, "source_state", action.source_state.is_some()) {
        Reason::AuthoredSourcePolicy
    } else if known_formatter(action, resolved_command) {
        Reason::KnownFormatter
    } else {
        Reason::UnknownCommand
    };
    let mut proposed = current.clone();
    if reason == Reason::KnownFormatter {
        proposed.source_state = ActionSourceState::Worktree;
    }
    Recommendation {
        target: action.target.clone(),
        exhaustive_requires_owner_assertion: current.inputs_policy
            != ActionInputsPolicy::Exhaustive,
        current,
        proposed,
        reason,
        inputs: action.inputs.clone(),
        proposed_inputs: action.inputs.clone(),
    }
}

pub(crate) fn is_authored(action: &ActionSpec, field: &str, present: bool) -> bool {
    present
        && !matches!(
            action.provenance.get(field),
            Some(FieldProvenance::Inferred | FieldProvenance::Inherited)
        )
}

pub(crate) fn is_read_only_check(action: &ActionSpec) -> bool {
    action.intent == ActionIntent::Check
        && action.effects.contains(&ActionEffect::ReadOnly)
        && action
            .effects
            .iter()
            .all(|effect| matches!(effect, ActionEffect::ReadOnly | ActionEffect::Process))
}

fn known_formatter(action: &ActionSpec, command: Option<&str>) -> bool {
    if !action.arguments.is_empty() {
        return false;
    }
    match &action.runner {
        ActionRunner::Command {
            working_directory,
            environment,
            ..
        }
        | ActionRunner::Shell {
            working_directory,
            environment,
            ..
        } => {
            root_directory(working_directory.as_deref())
                && environment.is_empty()
                && command == Some("cargo fmt --all -- --check")
        }
        ActionRunner::Argv {
            program,
            args,
            working_directory,
            environment,
        } => {
            root_directory(working_directory.as_deref())
                && environment.is_empty()
                && program == "cargo"
                && args
                    == &["fmt", "--all", "--", "--check"]
                        .map(|value| ArgvValue::Literal(value.into()))
        }
        ActionRunner::Native { .. } => false,
    }
}

fn root_directory(directory: Option<&str>) -> bool {
    matches!(directory, None | Some("."))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod patch_tests;
