//! Epoch-7 and epoch-8 action input declarations and their adoption preview.
//!
//! These declarations are part of the repository contract and drive affected
//! selection; Jig no longer records per-target freshness identities from them.

use anyhow::{Result, ensure};
use jig_contract::freshness::{
    TARGET_FRESHNESS_CONTRACT_VERSION, WORKTREE_FRESHNESS_CONTRACT_VERSION,
};
use jig_contract::{ActionInputsPolicy, ActionSourceState, ActionSpec};

pub(crate) mod adoption;

pub(crate) fn validate_inputs_policy(epoch: u32, action: &ActionSpec) -> Result<()> {
    ensure!(
        epoch >= WORKTREE_FRESHNESS_CONTRACT_VERSION || action.source_state.is_none(),
        "target '{}' source_state requires contract version {WORKTREE_FRESHNESS_CONTRACT_VERSION} or later",
        action.target
    );
    ensure!(
        action.source_state != Some(ActionSourceState::Worktree)
            || !matches!(action.runner, jig_contract::ActionRunner::Native { .. }),
        "native target '{}' must retain Git and comparison authority; source_state = worktree is for commands that consume working files",
        action.target
    );
    ensure!(
        epoch >= TARGET_FRESHNESS_CONTRACT_VERSION || action.inputs_policy.is_none(),
        "target '{}' inputs_policy requires contract version {TARGET_FRESHNESS_CONTRACT_VERSION} or later, including an explicit whole_repository value",
        action.target
    );
    ensure!(
        action.inputs_policy != Some(ActionInputsPolicy::Exhaustive) || !action.inputs.is_empty(),
        "target '{}' exhaustive inputs_policy requires non-empty inputs",
        action.target
    );
    ensure!(
        action.inputs_policy != Some(ActionInputsPolicy::Exhaustive)
            || !action
                .inputs
                .iter()
                .any(|input| input.split('/').any(|part| part == ".git")),
        "target '{}' exhaustive inputs cannot declare excluded Git metadata",
        action.target
    );
    Ok(())
}

#[cfg(test)]
mod tests;
