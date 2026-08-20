//! Backend-neutral migration command DTOs.

use super::ToolRequest;

#[derive(Debug)]
pub(crate) struct MigrationAddRequest {
    pub(crate) name: String,
    pub(crate) tool: ToolRequest,
}
