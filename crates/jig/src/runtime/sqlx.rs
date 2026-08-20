use anyhow::Result;
use serde_json::{Value, json};

use crate::command::SqlxCommand;
use crate::context::RepoContext;
use crate::tool_defs::tool;

use super::tool_execution;

pub(super) fn dispatch(ctx: &RepoContext, command: SqlxCommand) -> Result<Value> {
    match command {
        SqlxCommand::SchemaDump(request) => tool_execution::execute_manifest_tool_request(
            ctx,
            tool::SCHEMA_DUMP,
            json!({}),
            request,
        ),
    }
}
