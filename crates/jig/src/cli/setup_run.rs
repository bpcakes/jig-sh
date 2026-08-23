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
    let progress = crate::progress::CliProgress::for_human_output("setup", json_output);
    progress.header("prepare repository and agent tooling");
    #[cfg(all(unix, not(test)))]
    let signal_session = doctor::DoctorSignalSession::start().map_err(|_| {
        anyhow::anyhow!("Setup was not started because signal supervision is unavailable")
    })?;
    #[cfg(all(unix, not(test)))]
    let cancellation = signal_session.cancellation();
    #[cfg(all(unix, not(test)))]
    let mut observer =
        crate::progress::CliExecutionObserver::with_cancellation(json_output, move || {
            cancellation.cancelled()
        });
    #[cfg(any(not(unix), test))]
    let mut observer = crate::progress::CliExecutionObserver::for_human_output(json_output);
    let outcome = run_setup_with_progress(
        || {
            #[cfg(all(unix, not(test)))]
            return doctor::run_with_cancellation(&|| cancellation.cancelled());
            #[cfg(any(not(unix), test))]
            doctor::run()
        },
        |command| runtime::dispatch_with_observer(&ctx, command, &mut observer),
        |current, total, label| progress.step(label, format!("phase {current}/{total}")),
    );
    let outcome = observer.finish_with(outcome);
    #[cfg(all(unix, not(test)))]
    let outcome = crate::codex::finish_signal_supervised(
        outcome,
        signal_session.finish(),
        "Setup signal supervision could not retire safely",
    );
    let output = outcome?;
    progress.done("setup complete");
    emit(json_output, HumanOutput::Setup, &output)?;
    require_json_ok(true, &output)
}

#[cfg(test)]
fn run_setup_with(
    run_doctor: impl FnMut() -> Result<Value>,
    dispatch: impl FnMut(RuntimeCommand) -> Result<Value>,
) -> Result<Value> {
    run_setup_with_progress(run_doctor, dispatch, |_, _, _| {})
}

fn run_setup_with_progress(
    mut run_doctor: impl FnMut() -> Result<Value>,
    mut dispatch: impl FnMut(RuntimeCommand) -> Result<Value>,
    mut progress: impl FnMut(usize, usize, &str),
) -> Result<Value> {
    const SETUP_PHASE_COUNT: usize = 7;
    let mut current_phase = 0;
    let mut next_phase = |label| {
        current_phase += 1;
        progress(current_phase, SETUP_PHASE_COUNT, label);
    };

    next_phase("doctor before");
    let doctor_before = run_doctor()?;
    next_phase("bootstrap");
    let bootstrap = dispatch(RuntimeCommand::Bootstrap(ToolRequest::default()))?;

    next_phase("agent readiness");
    let agent_before = dispatch(RuntimeCommand::Agent(AgentCommand::Doctor))?;
    let mut registrations = Vec::new();
    let mut registration_sources = Vec::new();
    if agent_before["codex"]["required"].as_bool().unwrap_or(false) {
        if agent_before["codex"]["available"].as_bool() != Some(true) {
            let next_step = agent_before["next_steps"]
                .as_array()
                .and_then(|steps| steps.first())
                .and_then(Value::as_str)
                .unwrap_or("Install or update Codex, then rerun scripts/jig setup.");
            bail!("Agent tooling setup requires Codex marketplace support. {next_step}");
        }
        registration_sources = unregistered_marketplace_sources(&agent_before)?;
    }
    next_phase("marketplace registration");
    for source in registration_sources {
        registrations.push(dispatch(RuntimeCommand::Agent(AgentCommand::Bootstrap(
            AgentBootstrapRequest {
                marketplace: Some(source),
            },
        )))?);
    }
    next_phase("agent verification");
    let agent_after = dispatch(RuntimeCommand::Agent(AgentCommand::Doctor))?;
    next_phase("contract verification");
    let contract = dispatch(RuntimeCommand::Check(CheckCommand::Contract(
        ToolRequest::default(),
    )))?;
    next_phase("doctor after");
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

    #[test]
    fn setup_progress_keeps_registration_items_in_one_monotonic_phase() {
        let agent_doctor_calls = std::cell::Cell::new(0);
        let mut registered = Vec::new();
        let mut phases = Vec::new();

        let output = run_setup_with_progress(
            || Ok(json!({ "ok": true })),
            |command| {
                Ok(match command {
                    RuntimeCommand::Bootstrap(_) => json!({ "ok": true }),
                    RuntimeCommand::Agent(AgentCommand::Doctor) => {
                        let call = agent_doctor_calls.get();
                        agent_doctor_calls.set(call + 1);
                        if call == 0 {
                            json!({
                                "ok": false,
                                "codex": { "required": true, "available": true },
                                "marketplaces": [
                                    {
                                        "source": "example-org/example-skills-a",
                                        "registered": false,
                                    },
                                    {
                                        "source": "example-org/example-skills-b",
                                        "registered": false,
                                    },
                                ],
                            })
                        } else {
                            json!({ "ok": true })
                        }
                    }
                    RuntimeCommand::Agent(AgentCommand::Bootstrap(request)) => {
                        registered.push(request.marketplace.unwrap());
                        json!({ "ok": true })
                    }
                    RuntimeCommand::Check(CheckCommand::Contract(_)) => json!({ "ok": true }),
                    _ => panic!("unexpected setup command"),
                })
            },
            |current, total, label| phases.push((current, total, label.to_string())),
        )
        .unwrap();

        assert_eq!(output["ok"], true);
        assert_eq!(
            registered,
            [
                "example-org/example-skills-a",
                "example-org/example-skills-b"
            ]
        );
        assert_eq!(
            phases
                .iter()
                .map(|(current, total, _)| (*current, *total))
                .collect::<Vec<_>>(),
            (1..=7).map(|current| (current, 7)).collect::<Vec<_>>()
        );
        assert_eq!(phases[3], (4, 7, "marketplace registration".to_string()));
    }
}
