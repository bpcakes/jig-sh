use super::*;

impl RepositoryRenderModel {
    pub(in crate::bootstrap) fn prepare_freshness_epoch(&mut self, epoch: u32) -> Result<()> {
        for action in &mut self.actions {
            infer_missing_source_policy(action, epoch, &self.commands);
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
            // Missing fields can take current generated defaults. Saved values,
            // including inferred Git policies, must remain distinguishable.
            for action in [&mut expected, &mut authored] {
                infer_missing_source_policy(
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

fn infer_missing_source_policy(
    action: &mut ActionSpec,
    epoch: u32,
    commands: &BTreeMap<String, String>,
) {
    if action.source_state.is_some() {
        return;
    }
    let command = match &action.runner {
        ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. } => {
            commands.get(command).map(String::as_str)
        }
        _ => None,
    };
    let recommendation = crate::repository::freshness::adoption::recommend(epoch, action, command);
    if recommendation.reason == crate::repository::freshness::adoption::Reason::KnownFormatter {
        action.source_state = Some(recommendation.proposed.source_state);
        action
            .provenance
            .insert("source_state".into(), FieldProvenance::Inferred);
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
