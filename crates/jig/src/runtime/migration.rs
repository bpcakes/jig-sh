use anyhow::Result;
use serde_json::{Value, json};

use crate::context::RepoContext;
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
        request.tool.record_receipt(),
        observer,
    )
    .map(|value| {
        let name = value["args"][args::NAME].clone();
        json!({
            "ok": true,
            "tool": tool::MIGRATION_ADD,
            args::NAME: name,
            "result": value["result"],
            "receipt_id": value["receipt_id"],
        })
    })
}
