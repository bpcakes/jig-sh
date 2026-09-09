use super::*;

impl AuthoredRepositoryModel {
    pub(in crate::bootstrap) fn command_references_resolve(
        &self,
        commands: &BTreeMap<String, String>,
    ) -> bool {
        self.actions.iter().all(|action| match &action.runner {
            ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. } => commands
                .get(command)
                .is_some_and(|value| !value.trim().is_empty()),
            ActionRunner::Native { .. } | ActionRunner::Argv { .. } => true,
        })
    }
}

impl RepositoryRenderModel {
    /// Make shell authority explicit on cutover, preserving authored argv/shell.
    pub(in crate::bootstrap) fn prepare_runner_epoch(
        &mut self,
        contract_version: u32,
    ) -> Result<()> {
        for action in &mut self.actions {
            if contract_version >= crate::repository::ACTION_EXECUTION_CONTRACT_VERSION {
                make_shell_explicit(&mut action.runner);
            }
            crate::repository::runners::validate(contract_version, action)?;
        }
        Ok(())
    }
}

/// Treat only the legacy implicit shell as the same authored implementation.
/// Argv remains a distinct choice even when it names a shell executable.
pub(super) fn make_shell_explicit(runner: &mut ActionRunner) {
    if let ActionRunner::Command {
        command,
        working_directory,
        environment,
    } = runner
    {
        *runner = ActionRunner::Shell {
            command: command.clone(),
            working_directory: working_directory.clone(),
            environment: environment.clone(),
        };
    }
}
