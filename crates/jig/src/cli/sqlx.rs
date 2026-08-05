use clap::{Args, Subcommand};

use crate::{command, tool_defs};

use super::ToolOpts;

const MIGRATION_ADD_AFTER_HELP: &str = "\
Use --plan-id to associate the migration with an open structured work plan.

Examples:
  jig sqlx migration add create_users
  jig sqlx migration add add_login_tokens --plan-id plan_abc123

The legacy `jig migration-add NAME` path remains accepted for compatibility.";

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

#[derive(Args, Debug)]
#[command(after_help = MIGRATION_ADD_AFTER_HELP)]
pub(crate) struct MigrationAddOpts {
    /// Migration name, for example create_users.
    pub(crate) name: String,
    #[command(flatten)]
    pub(crate) tool: ToolOpts,
}

impl From<SqlxCommand> for command::SqlxCommand {
    fn from(command: SqlxCommand) -> Self {
        match command {
            SqlxCommand::Migration(SqlxMigrationCommand::Add(opts)) => {
                Self::MigrationAdd(opts.into())
            }
            SqlxCommand::Schema(SqlxSchemaCommand::Dump(opts)) => Self::SchemaDump(opts.into()),
        }
    }
}

impl From<MigrationAddOpts> for command::MigrationAddRequest {
    fn from(opts: MigrationAddOpts) -> Self {
        Self {
            name: opts.name,
            tool: opts.tool.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_add_conversion_preserves_tool_receipt_controls() {
        let request: command::MigrationAddRequest = MigrationAddOpts {
            name: "create_users".to_string(),
            tool: ToolOpts {
                plan_id: Some("plan_1".to_string()),
                no_receipt: false,
            },
        }
        .into();

        assert_eq!(request.name, "create_users");
        let (plan_id, record_receipt) = request.tool.into_parts();
        assert_eq!(plan_id.as_deref(), Some("plan_1"));
        assert!(record_receipt);

        let no_receipt_request: command::MigrationAddRequest = MigrationAddOpts {
            name: "drop_old_table".to_string(),
            tool: ToolOpts {
                plan_id: None,
                no_receipt: true,
            },
        }
        .into();

        let (plan_id, record_receipt) = no_receipt_request.tool.into_parts();
        assert_eq!(plan_id, None);
        assert!(!record_receipt);
    }
}
