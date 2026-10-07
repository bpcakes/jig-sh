use anyhow::Result;
use jig_context::RepoContext;
use serde_json::{Value, json};

use crate::execution::ExecutionControl;
use crate::tool_defs::{args, tool};

use super::tool_execution;

pub(super) fn add(
    ctx: &RepoContext,
    request: crate::command::MigrationAddRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    tool_execution::execute_manifest_tool_with_observer(
        ctx,
        tool::MIGRATION_ADD,
        json!({ args::NAME: request.name }),
        observer,
    )
    .map(|value| {
        let name = value["args"][args::NAME].clone();
        json!({
            "ok": true,
            "tool": tool::MIGRATION_ADD,
            args::NAME: name,
            "result": value["result"],
        })
    })
}
