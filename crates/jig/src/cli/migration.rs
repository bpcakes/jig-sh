use clap::{Args, Subcommand};

use crate::{command, tool_defs};

use super::ToolOpts;

const MIGRATION_ADD_AFTER_HELP: &str = "\
Use --plan-id to associate the migration with an open structured work plan.

Examples:
  jig migration add create_users
  jig migration add add_login_tokens --plan-id plan_abc123

The `jig sqlx migration add NAME` and `jig migration-add NAME` paths remain accepted for compatibility.";

pub(super) const MIGRATION_AFTER_HELP: &str = "\
Create migrations in the repository's configured backend format.

Examples:
  jig migration add create_users
  jig migration add add_login_tokens --plan-id plan_abc123";

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
    #[command(flatten)]
    pub(crate) tool: ToolOpts,
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
