use anyhow::Result;
use serde_json::{Value, json};

use crate::command::SqlxCommand;
use crate::context::RepoContext;
use crate::execution::ExecutionControl;
use crate::tool_defs::tool;

use super::tool_execution;

pub(super) fn dispatch_with_observer(
    ctx: &RepoContext,
    command: SqlxCommand,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    match command {
        SqlxCommand::SchemaDump(request) => {
            tool_execution::execute_manifest_tool_request_with_observer(
                ctx,
                tool::SCHEMA_DUMP,
                json!({}),
                request,
                observer,
            )
        }
    }
}
