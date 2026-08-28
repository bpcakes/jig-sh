//! SQLx-specific command DTOs.

use super::ToolRequest;

#[derive(Debug)]
pub(crate) enum SqlxCommand {
    SchemaDump(ToolRequest),
}
