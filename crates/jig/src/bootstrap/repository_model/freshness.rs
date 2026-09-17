use super::*;

impl RepositoryRenderModel {
    pub(in crate::bootstrap) fn prepare_freshness_epoch(&mut self, epoch: u32) -> Result<()> {
        for action in &mut self.actions {
            retract_formatter_inference(action, epoch, &self.commands);
            prepare_action_inputs_policy(action, epoch)?;
        }
        Ok(())
    }
}

/// Compare generated projections across explicit-shell and freshness cutovers.
/// An authored policy or provenance remains distinct from a generated default.
pub(super) fn matches_generated_actions(
    expected: &[ActionSpec],
    authored: &[ActionSpec],
    commands: &BTreeMap<String, String>,
) -> bool {
    expected.len() == authored.len()
        && expected.iter().zip(authored).all(|(expected, authored)| {
            let mut expected = expected.clone();
            let mut authored = authored.clone();
            // Missing fields take conservative generated defaults; authored
            // policies and their provenance must remain distinguishable.
            for action in [&mut expected, &mut authored] {
                retract_formatter_inference(
                    action,
                    crate::context::CURRENT_CONTRACT_VERSION,
                    commands,
                );
                super::runners::make_shell_explicit(&mut action.runner);
                if prepare_action_inputs_policy(action, crate::context::CURRENT_CONTRACT_VERSION)
                    .is_err()
                {
                    return false;
                }
            }
            expected == authored
        })
}

/// An unreleased generator inferred Git independence from Cargo command spelling.
/// Retract that inference on update, including when unrelated edits keep the
/// model authored. Explicit owner policies and changed implementations stay intact.
fn retract_formatter_inference(
    action: &mut ActionSpec,
    epoch: u32,
    commands: &BTreeMap<String, String>,
) {
    use crate::repository::freshness::adoption::{cargo_formatter, is_read_only_check};
    use jig_contract::ActionSourceState;

    if epoch < jig_contract::freshness::WORKTREE_FRESHNESS_CONTRACT_VERSION
        || action.source_state != Some(ActionSourceState::Worktree)
        || action.provenance.get("source_state") != Some(&FieldProvenance::Inferred)
        || !is_read_only_check(action)
    {
        return;
    }
    let command = match &action.runner {
        ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. } => {
            commands.get(command).map(String::as_str)
        }
        _ => None,
    };
    if cargo_formatter(action, command) {
        action.source_state = Some(ActionSourceState::Git);
    }
}

/// Default generated and inherited actions conservatively. An explicit authored
/// assertion keeps its value and provenance through adoption and recopy.
pub(in crate::bootstrap) fn prepare_action_inputs_policy(
    action: &mut ActionSpec,
    epoch: u32,
) -> Result<()> {
    if epoch >= jig_contract::freshness::TARGET_FRESHNESS_CONTRACT_VERSION {
        let provenance = if action.inputs_policy.is_some() {
            FieldProvenance::Declared
        } else {
            FieldProvenance::Inferred
        };
        action.inputs_policy = Some(action.inputs_policy.unwrap_or_default());
        action
            .provenance
            .entry("inputs_policy".into())
            .or_insert(provenance);
    }
    if epoch >= jig_contract::freshness::WORKTREE_FRESHNESS_CONTRACT_VERSION {
        let provenance = if action.source_state.is_some() {
            FieldProvenance::Declared
        } else {
            FieldProvenance::Inferred
        };
        action.source_state = Some(action.source_state.unwrap_or_default());
        action
            .provenance
            .entry("source_state".into())
            .or_insert(provenance);
    }
    crate::repository::freshness::validate_inputs_policy(epoch, action)
}
