use clap::Subcommand;

use crate::tool_defs;

use super::{MigrationAddOpts, ToolOpts};

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
    Dump(ToolOpts),
}
