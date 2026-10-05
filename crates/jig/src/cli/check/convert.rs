use anyhow::{Result, bail};
use clap::ValueEnum;

use crate::cli::CliExactTreeProvenance;
use crate::command;

use super::{
    CheckCommand, CheckComparisonOpts, CheckMigrationImmutabilityOpts, CheckOpts, CheckTargetOpts,
};

impl TryFrom<CheckOpts> for command::CheckCommand {
    type Error = anyhow::Error;

    fn try_from(opts: CheckOpts) -> Result<Self> {
        let CheckOpts {
            mut profile,
            mut affected,
            mut explain,
            mut fail_fast,
            mut comparison,
            command,
        } = opts;

        let command = match command {
            Some(CheckCommand::Selectors(selectors)) => {
                Some(CheckCommand::Selectors(normalize_external_check_args(
                    selectors,
                    &mut profile,
                    &mut affected,
                    &mut explain,
                    &mut fail_fast,
                    &mut comparison,
                )?))
            }
            command => command,
        };
        let comparison = comparison.request()?;

        match command {
            None => Ok(Self::Repository(command::RepositoryCheckRequest {
                selectors: Vec::new(),
                profile,
                affected_base: affected,
                comparison,
                explain,
                fail_fast,
            })),
            Some(CheckCommand::Selectors(selectors)) => {
                Ok(Self::Repository(command::RepositoryCheckRequest {
                    selectors,
                    profile,
                    affected_base: affected,
                    comparison,
                    explain,
                    fail_fast,
                }))
            }
            Some(command)
                if profile.is_some()
                    || affected.is_some()
                    || comparison.is_some()
                    || explain
                    || fail_fast
                    || command.has_additional_selectors() =>
            {
                let (selector, child) = repository_selector(command)?;
                let mut selectors = Vec::with_capacity(child.selectors.len() + 1);
                selectors.push(selector.into());
                selectors.extend(child.selectors);
                Ok(Self::Repository(command::RepositoryCheckRequest {
                    selectors,
                    profile,
                    affected_base: affected,
                    comparison,
                    explain,
                    fail_fast,
                }))
            }
            // Preserve the named command DTO until runtime has loaded the
            // repository contract. `dispatch_named_check` executes the legacy
            // manifest tool only for v2-v5; v6 resolves this name as a
            // repository selector so every component action is included.
            Some(command) => Ok(direct_check_command(command)),
        }
    }
}

include!("external_check.rs");

fn direct_check_command(command: CheckCommand) -> command::CheckCommand {
    match command {
        CheckCommand::Named(named) => command::CheckCommand::Named(named.into_parts().0),
        CheckCommand::AgentMap(opts) => command::CheckCommand::AgentMap(opts.into()),
        CheckCommand::AgentGuides => command::CheckCommand::AgentGuides,
        CheckCommand::MigrationImmutability(opts) => {
            command::CheckCommand::MigrationImmutability(opts.into())
        }
        CheckCommand::SqlxUncheckedNonTest => command::CheckCommand::SqlxUncheckedNonTest,
        CheckCommand::Selectors(_) => {
            unreachable!("external selectors are handled before direct commands")
        }
    }
}

fn repository_selector(command: CheckCommand) -> Result<(&'static str, CheckTargetOpts)> {
    match command {
        CheckCommand::Named(named) => {
            let (check, opts) = named.into_parts();
            Ok((check.selector, opts))
        }
        CheckCommand::AgentMap(_)
        | CheckCommand::AgentGuides
        | CheckCommand::MigrationImmutability(_)
        | CheckCommand::SqlxUncheckedNonTest => {
            bail!(
                "profiles, affected selection, --explain, and --fail-fast apply to repository targets, not Jig-owned policy subcommands"
            )
        }
        CheckCommand::Selectors(_) => unreachable!("external selectors are handled separately"),
    }
}

impl From<CheckMigrationImmutabilityOpts> for command::MigrationImmutabilityRequest {
    fn from(opts: CheckMigrationImmutabilityOpts) -> Self {
        Self {
            changed_against: opts.changed_against,
        }
    }
}

#[cfg(test)]
#[path = "convert_tests.rs"]
mod tests;
