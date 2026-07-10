use anyhow::{Result, anyhow, bail};

use crate::context::RepoContext;
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
    if !tool_defs::is_no_arg_execution_tool(tool) {
        if !tool_defs::is_execution_tool(tool) {
            bail!("{label} is not an execution tool: {name}");
        }
        bail!("{label} requires an argument and cannot run as a configured gate: {name}");
    }
    Ok(())
}
