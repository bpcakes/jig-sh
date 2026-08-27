use std::collections::BTreeSet;

use anyhow::{Result, anyhow, bail};

use crate::context::{RepoContext, WorkCheckGate, WorkGate};
use crate::tool_defs;

#[derive(Clone, Debug)]
pub(super) enum SelectedCheck {
    Gate { gate: WorkCheckGate, force: bool },
    Tool(String),
}

impl SelectedCheck {
    pub(super) fn tool(&self) -> &str {
        match self {
            Self::Gate { gate, .. } => &gate.tool,
            Self::Tool(tool) => tool,
        }
    }
}

pub(super) fn selected_checks(
    ctx: &RepoContext,
    explicit_gates: &[String],
    explicit_tools: &[String],
) -> Result<Vec<SelectedCheck>> {
    if !explicit_gates.is_empty() && !explicit_tools.is_empty() {
        bail!("Work check accepts either gate ids or tool names, not both");
    }

    let configured = ctx
        .work_gates()
        .into_iter()
        .filter_map(|gate| match gate {
            WorkGate::Check(gate) => Some(gate),
            _ => None,
        })
        .collect::<Vec<_>>();
    if !explicit_gates.is_empty() {
        let mut selected = Vec::new();
        let mut seen = BTreeSet::new();
        for id in explicit_gates {
            if !seen.insert(id.as_str()) {
                continue;
            }
            let gate = configured
                .iter()
                .find(|gate| gate.id == *id)
                .cloned()
                .ok_or_else(|| anyhow!("Unknown configured check gate id: {id}"))?;
            selected.push(SelectedCheck::Gate { gate, force: true });
        }
        return Ok(selected);
    }

    if !explicit_tools.is_empty() {
        if ctx.contract_version() < crate::context::CURRENT_CONTRACT_VERSION {
            return Ok(explicit_tools
                .iter()
                .cloned()
                .map(SelectedCheck::Tool)
                .collect());
        }
        let mut selected = Vec::new();
        let mut seen = BTreeSet::new();
        for tool in explicit_tools {
            if !seen.insert(tool.as_str()) {
                continue;
            }
            let matching = configured
                .iter()
                .filter(|gate| gate.tool == *tool)
                .cloned()
                .collect::<Vec<_>>();
            if matching.is_empty() {
                selected.push(SelectedCheck::Tool(tool.clone()));
            } else {
                selected.extend(
                    matching
                        .into_iter()
                        .map(|gate| SelectedCheck::Gate { gate, force: true }),
                );
            }
        }
        return Ok(selected);
    }

    if ctx.contract_version() < crate::context::CURRENT_CONTRACT_VERSION {
        return Ok(ctx
            .work_check_tools()
            .into_iter()
            .map(SelectedCheck::Tool)
            .collect());
    }

    Ok(configured
        .into_iter()
        .filter(|gate| gate.required)
        .map(|gate| SelectedCheck::Gate { gate, force: false })
        .collect())
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
