use anyhow::{Context, Result, bail};

use super::answers::RenderAnswers;
use super::repository_model::is_rust_file_loc_action;
use crate::context::{RepoContext, WorkGate};
use crate::tool_defs::{kind, tool};

pub(super) const fn jig_launcher(minimal_footprint: bool) -> &'static str {
    if minimal_footprint {
        "jig"
    } else {
        "scripts/jig"
    }
}

pub(super) fn generated_gates(ctx: &RepoContext, answers: &RenderAnswers) -> Result<Vec<String>> {
    // Derive this preview from the staged closure policy. Repo utilities that are
    // not configured work gates must not be presented as plan requirements.
    let launcher = jig_launcher(answers.is_minimal_footprint());
    if ctx.contract_version() >= 6 {
        let mut gates = Vec::new();
        if answers.bootstrap_command_configured() {
            gates.push(format!("{launcher} bootstrap"));
        }
        gates.push(format!("{launcher} check contract"));
        if answers.go_backend_enabled() {
            gates.extend([
                format!("{launcher} check fmt"),
                format!("{launcher} check lint"),
                format!("{launcher} check test"),
            ]);
        } else {
            gates.extend([
                format!("{launcher} check fmt"),
                format!("{launcher} check clippy"),
                format!("{launcher} check test"),
            ]);
            if ctx.action_specs().iter().any(is_rust_file_loc_action) {
                gates.push(format!("{launcher} run repo:rust-file-loc"));
            }
        }
        if answers.sqlx_enabled() {
            gates.push(format!("{launcher} check sqlx"));
        }
        if answers.schema_dump_enabled() {
            gates.push(format!("{launcher} check schema"));
        }
        if answers.frontend_harness_enabled() {
            gates.extend([
                format!("{launcher} check typescript-lint"),
                format!("{launcher} check typescript-typecheck"),
                format!("{launcher} check typescript-build"),
                format!("{launcher} check typescript-coverage"),
            ]);
        }
        return Ok(gates);
    }
    ctx.work_gates()
        .into_iter()
        .filter_map(|gate| match gate {
            WorkGate::Check(gate) => Some(gate),
            WorkGate::Evidence(_) | WorkGate::CodexReview(_) | WorkGate::Unsupported(_) => None,
        })
        .map(|gate| gate_command(ctx, launcher, &gate.tool))
        .collect()
}

fn gate_command(ctx: &RepoContext, launcher: &str, tool_name: &str) -> Result<String> {
    let check = match tool_name {
        tool::CONTRACT_CHECK => Some("contract"),
        tool::FMT_CHECK => Some("fmt"),
        tool::CLIPPY => Some("clippy"),
        tool::TEST => Some("test"),
        tool::SQLX_CHECK => Some("sqlx"),
        tool::SCHEMA_CHECK => Some("schema"),
        _ => None,
    };
    if let Some(check) = check {
        return Ok(format!("{launcher} check {check}"));
    }

    let tool = ctx
        .tool_spec(tool_name)
        .with_context(|| format!("Generated work gate references undeclared tool {tool_name}"))?;
    if tool.kind != kind::COMMAND {
        bail!("Generated work gate tool {tool_name} has no preview command");
    }
    let command_key = tool.command.as_deref().with_context(|| {
        format!("Generated command-backed work gate tool {tool_name} has no command key")
    })?;
    Ok(ctx
        .command_for_key(command_key)
        .with_context(|| format!("Failed to resolve generated work gate tool {tool_name}"))?
        .to_owned())
}
