use anyhow::Result;
use jig_context::RepoContext;
use serde_json::{Value, json};

use crate::command::SqlxCommand;
use crate::execution::ExecutionControl;
use crate::tool_defs::tool;

use super::tool_execution;

pub(super) fn dispatch_with_observer(
    ctx: &RepoContext,
    command: SqlxCommand,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    match command {
        SqlxCommand::SchemaDump => tool_execution::execute_manifest_tool_with_observer(
            ctx,
            tool::SCHEMA_DUMP,
            json!({}),
            observer,
        ),
    }
}
