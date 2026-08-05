use anyhow::Result;
use serde_json::{Value, json};

use crate::command::SqlxCommand;
use crate::context::RepoContext;
use crate::tool_defs::{args, tool};

use super::tool_execution;

pub(super) fn dispatch(ctx: &RepoContext, command: SqlxCommand) -> Result<Value> {
    match command {
        SqlxCommand::MigrationAdd(request) => migration_add(ctx, request),
        SqlxCommand::SchemaDump(request) => tool_execution::execute_manifest_tool_request(
            ctx,
            tool::SCHEMA_DUMP,
            json!({}),
            request,
        ),
    }
}

fn migration_add(ctx: &RepoContext, request: crate::command::MigrationAddRequest) -> Result<Value> {
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
