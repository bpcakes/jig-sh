use clap::Subcommand;

use super::MigrationAddOpts;
use super::runtime_dispatch::RuntimeDispatch;
use crate::command::{self, RuntimeCommand};
use crate::tool_defs;

pub(super) const SQLX_AFTER_HELP: &str = "\
SQLx checks remain grouped with the other project checks under `jig check`.

Examples:
  jig sqlx migration add create_users
  jig sqlx schema dump
  jig check sqlx
  jig check schema";

#[derive(Debug, Subcommand)]
pub(crate) enum SqlxCommand {
    /// Create and manage forward-only SQLx migrations.
    #[command(name = tool_defs::cli_command::SQLX_MIGRATION, subcommand)]
    Migration(SqlxMigrationCommand),
    /// Generate and manage schema documentation.
    #[command(name = tool_defs::cli_command::SQLX_SCHEMA, subcommand)]
    Schema(SqlxSchemaCommand),
}

#[derive(Debug, Subcommand)]
pub(crate) enum SqlxMigrationCommand {
    /// Add a forward-only SQLx migration file.
    #[command(name = tool_defs::cli_command::SQLX_MIGRATION_ADD)]
    Add(MigrationAddOpts),
}

#[derive(Debug, Subcommand)]
pub(crate) enum SqlxSchemaCommand {
    /// Regenerate schema documentation when schema dumps are enabled.
    #[command(name = tool_defs::cli_command::SQLX_SCHEMA_DUMP)]
    Dump,
}

impl SqlxCommand {
    pub(super) fn into_dispatch(self) -> RuntimeDispatch {
        match self {
            Self::Migration(SqlxMigrationCommand::Add(opts)) => opts.into_dispatch(),
            Self::Schema(SqlxSchemaCommand::Dump) => schema_dump_dispatch(),
        }
    }
}

/// Shared by `sqlx schema dump` and the retained `schema-dump` spelling.
pub(super) fn schema_dump_dispatch() -> RuntimeDispatch {
    RuntimeDispatch::tool(RuntimeCommand::Sqlx(command::SqlxCommand::SchemaDump))
}
