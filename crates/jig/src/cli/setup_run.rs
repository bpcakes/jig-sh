use anyhow::{Result, bail};
use serde_json::{Value, json};

use super::output::{HumanOutput, emit};
use super::structured_error::require_json_ok;
use crate::command::{
    AgentBootstrapRequest, AgentCommand, CheckCommand, RuntimeCommand, ToolRequest,
};
use crate::{context::RepoContext, doctor, runtime};

pub(super) fn run_setup_command(json_output: bool) -> Result<()> {
    let ctx = RepoContext::load()?;
    let output = run_setup_with(doctor::run, |command| runtime::dispatch(&ctx, command))?;
    emit(json_output, HumanOutput::Setup, &output)?;
    require_json_ok(true, &output)
}

fn run_setup_with(
    mut run_doctor: impl FnMut() -> Result<Value>,
    mut dispatch: impl FnMut(RuntimeCommand) -> Result<Value>,
) -> Result<Value> {
    let doctor_before = run_doctor()?;
    let bootstrap = dispatch(RuntimeCommand::Bootstrap(ToolRequest::default()))?;

    let agent_before = dispatch(RuntimeCommand::Agent(AgentCommand::Doctor))?;
    let mut registrations = Vec::new();
    if agent_before["codex"]["required"].as_bool().unwrap_or(false) {
        if agent_before["codex"]["available"].as_bool() != Some(true) {
            let next_step = agent_before["next_steps"]
                .as_array()
                .and_then(|steps| steps.first())
                .and_then(Value::as_str)
                .unwrap_or("Install or update Codex, then rerun scripts/jig setup.");
            bail!("Agent tooling setup requires Codex marketplace support. {next_step}");
        }
        for source in unregistered_marketplace_sources(&agent_before)? {
            registrations.push(dispatch(RuntimeCommand::Agent(AgentCommand::Bootstrap(
                AgentBootstrapRequest {
                    marketplace: Some(source),
                },
            )))?);
        }
    }
    let agent_after = dispatch(RuntimeCommand::Agent(AgentCommand::Doctor))?;
    let contract = dispatch(RuntimeCommand::Check(CheckCommand::Contract(
        ToolRequest::default(),
    )))?;
    let doctor_after = run_doctor()?;
    let ok = bootstrap["ok"].as_bool().unwrap_or(false)
        && agent_after["ok"].as_bool().unwrap_or(false)
        && contract["ok"].as_bool().unwrap_or(false)
        && doctor_after["ok"].as_bool().unwrap_or(false);

    Ok(json!({
        "ok": ok,
        "command": "setup",
        "doctor_before": doctor_before,
        "bootstrap": bootstrap,
        "agent": {
            "before": agent_before,
            "registrations": registrations,
            "after": agent_after,
        },
        "contract": contract,
        "doctor_after": doctor_after,
    }))
}

fn unregistered_marketplace_sources(agent_doctor: &Value) -> Result<Vec<String>> {
    agent_doctor["marketplaces"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter(|marketplace| !marketplace["registered"].as_bool().unwrap_or(false))
        .map(|marketplace| {
            marketplace["source"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Agent doctor reported an unregistered marketplace without a source"
                    )
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[test]
    fn setup_runs_doctor_before_bootstrap_and_verifies_again_at_the_end() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let doctor_calls = Rc::clone(&calls);
        let dispatch_calls = Rc::clone(&calls);
        let agent_ready = Rc::new(RefCell::new(false));
        let dispatch_agent_ready = Rc::clone(&agent_ready);

        let output = run_setup_with(
            || {
                doctor_calls.borrow_mut().push("doctor");
                Ok(json!({ "ok": true }))
            },
            |command| {
                let mut calls = dispatch_calls.borrow_mut();
                Ok(match command {
                    RuntimeCommand::Bootstrap(_) => {
                        calls.push("bootstrap");
                        json!({ "ok": true })
                    }
                    RuntimeCommand::Agent(AgentCommand::Doctor) => {
                        calls.push("agent doctor");
                        let ready = *dispatch_agent_ready.borrow();
                        json!({
                            "ok": ready,
                            "codex": { "required": true, "available": true },
                            "marketplaces": [{
                                "source": "owner/skills",
                                "registered": ready,
                            }],
                            "next_steps": [],
                        })
                    }
                    RuntimeCommand::Agent(AgentCommand::Bootstrap(request)) => {
                        calls.push("agent bootstrap");
                        assert_eq!(request.marketplace.as_deref(), Some("owner/skills"));
                        *dispatch_agent_ready.borrow_mut() = true;
                        json!({ "ok": true })
                    }
                    RuntimeCommand::Check(CheckCommand::Contract(_)) => {
                        calls.push("check contract");
                        json!({ "ok": true })
                    }
                    _ => panic!("unexpected setup command"),
                })
            },
        )
        .unwrap();

        assert_eq!(
            calls.borrow().as_slice(),
            [
                "doctor",
                "bootstrap",
                "agent doctor",
                "agent bootstrap",
                "agent doctor",
                "check contract",
                "doctor",
            ]
        );
        assert_eq!(output["ok"], true);
    }

    #[test]
    fn marketplace_source_is_required_for_automatic_registration() {
        let error = unregistered_marketplace_sources(&json!({
            "marketplaces": [{ "registered": false }]
        }))
        .unwrap_err()
        .to_string();

        assert!(error.contains("without a source"));
    }
}
