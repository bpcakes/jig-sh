use anyhow::Result;
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::tool_defs::{args, tool};

use super::tool_execution;

pub(super) fn add(
    ctx: &RepoContext,
    request: crate::command::MigrationAddRequest,
) -> Result<Value> {
    let (plan_id, record_receipt) = request.tool.into_parts();
    tool_execution::execute_manifest_tool(
        ctx,
        tool::MIGRATION_ADD,
        json!({ args::NAME: request.name }),
        plan_id,
        record_receipt,
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
