use super::*;

impl RepositoryRenderModel {
    pub(in crate::bootstrap) fn prepare_freshness_epoch(&mut self, epoch: u32) -> Result<()> {
        for action in &mut self.actions {
            prepare_action_inputs_policy(action, epoch)?;
        }
        Ok(())
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
    crate::repository::freshness::validate_inputs_policy(epoch, action)
}
