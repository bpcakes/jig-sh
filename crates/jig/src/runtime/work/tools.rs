use anyhow::{Result, anyhow, bail};

use crate::context::RepoContext;
use crate::repository::RepositoryCatalog;
use crate::tool_defs;

pub(super) fn selected_tools(ctx: &RepoContext, explicit_tools: &[String]) -> Result<Vec<String>> {
    let tools = if explicit_tools.is_empty() {
        ctx.work_check_tools()
    } else {
        explicit_tools.to_vec()
    };

    if tools.is_empty() {
        bail!("No work check gates configured. Add work.gates to .jig.toml or pass --tool.");
    }

    Ok(tools)
}

pub(super) fn validate_check_tool(ctx: &RepoContext, name: &str, label: &str) -> Result<()> {
    let tool = ctx.tool_spec(name).ok_or_else(|| {
        anyhow!(
            "{}",
            super::super::tool_execution::undeclared_tool_message(ctx, name)
        )
    })?;
    if !tool_defs::is_execution_tool(tool) {
        bail!("{label} is not an execution tool: {name}");
    }
    let requires_name = if ctx.contract_version() >= 6 {
        let catalog = RepositoryCatalog::from_context(ctx)?;
        let native_operation =
            catalog
                .action_for_alias(name)
                .and_then(|action| match &action.runner {
                    jig_contract::ActionRunner::Native { operation } => Some(operation.as_str()),
                    jig_contract::ActionRunner::Command { .. } => None,
                });
        tool_defs::execution_tool_requires_name_for_native_operation(tool, native_operation)
    } else {
        tool_defs::execution_tool_requires_name(tool)
    };
    if requires_name {
        bail!("{label} requires an argument and cannot run as a configured gate: {name}");
    }
    Ok(())
}
