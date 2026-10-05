use clap::{Args, Subcommand};

use super::output;
use super::runtime_dispatch::RuntimeDispatch;
use crate::command::RuntimeCommand;
use crate::{command, tool_defs};

const MIGRATION_ADD_AFTER_HELP: &str = "\
Examples:
  jig migration add create_users

The `jig sqlx migration add NAME` and `jig migration-add NAME` paths remain accepted for compatibility.";

pub(super) const MIGRATION_AFTER_HELP: &str = "\
Create migrations in the repository's configured backend format.

Examples:
  jig migration add create_users";

#[derive(Debug, Subcommand)]
pub(crate) enum MigrationCommand {
    /// Add a migration stub in the configured backend format.
    #[command(name = tool_defs::cli_command::MIGRATION_ADD_NESTED)]
    Add(MigrationAddOpts),
}

#[derive(Args, Debug)]
#[command(after_help = MIGRATION_ADD_AFTER_HELP)]
pub(crate) struct MigrationAddOpts {
    /// Migration name, for example create_users.
    pub(crate) name: String,
}

impl From<MigrationAddOpts> for command::MigrationAddRequest {
    fn from(opts: MigrationAddOpts) -> Self {
        Self { name: opts.name }
    }
}

impl MigrationCommand {
    pub(super) fn into_dispatch(self) -> RuntimeDispatch {
        match self {
            Self::Add(opts) => opts.into_dispatch(),
        }
    }
}

impl MigrationAddOpts {
    /// Shared by `migration add`, `sqlx migration add`, and the retained
    /// `migration-add` spelling.
    pub(super) fn into_dispatch(self) -> RuntimeDispatch {
        RuntimeDispatch::new(
            RuntimeCommand::MigrationAdd(self.into()),
            output::format_migration_add_summary,
        )
    }
}
