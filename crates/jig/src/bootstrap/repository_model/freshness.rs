use super::*;

impl RepositoryRenderModel {
    pub(in crate::bootstrap) fn prepare_freshness_epoch(&mut self, epoch: u32) -> Result<()> {
        for action in &mut self.actions {
            prepare_action_inputs_policy(action, epoch)?;
        }
        Ok(())
    }
}

/// Compare generated projections across explicit-shell and freshness cutovers.
/// An authored policy or provenance remains distinct from a generated default.
pub(super) fn matches_generated_actions(expected: &[ActionSpec], authored: &[ActionSpec]) -> bool {
    expected.len() == authored.len()
        && expected.iter().zip(authored).all(|(expected, authored)| {
            let mut expected = expected.clone();
            let mut authored = authored.clone();
            for action in [&mut expected, &mut authored] {
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
