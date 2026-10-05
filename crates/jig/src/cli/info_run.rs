use std::io::Write;

use anyhow::{Result, bail};

use super::output::{HumanOutput, emit, print_json};
use super::run::finish_after_json_output;
use super::structured_error::require_json_ok;
use super::{InfoCommand, InfoOpts};
use crate::context::RepoContext;
use crate::repository::InspectRequest;
use crate::{doctor, info};

mod freshness;

pub(super) fn run_info_command(opts: InfoOpts, json_output: bool) -> Result<()> {
    opts.validate_projection()?;
    if let Some(InfoCommand::Freshness(freshness)) = opts.subject.as_ref() {
        if opts.commands {
            bail!("--commands cannot be combined with an info subject");
        }
        return freshness::run(freshness, json_output);
    }
    if matches!(opts.subject.as_ref(), Some(InfoCommand::GoVersion)) {
        if opts.commands {
            bail!("--commands cannot be combined with an info subject");
        }
        return run_go_version(json_output);
    }
    let request = opts.subject.map(|subject| match subject {
        InfoCommand::GoVersion | InfoCommand::Freshness(_) => {
            unreachable!("handled above")
        }
        InfoCommand::Workspace => InspectRequest::Workspace,
        InfoCommand::Components => InspectRequest::Components,
        InfoCommand::Component { id } => InspectRequest::Component(id),
        InfoCommand::Targets => InspectRequest::Targets,
        InfoCommand::Target { id } => InspectRequest::Target(id),
        InfoCommand::Profiles => InspectRequest::Profiles,
        InfoCommand::Profile { id } => InspectRequest::Profile(id),
    });
    let output = info::run(opts.commands, json_output, request, opts.projection)?;
    emit(json_output, HumanOutput::Info, &output)?;
    finish_after_json_output(require_json_ok(true, &output), json_output)
}

fn run_go_version(json_output: bool) -> Result<()> {
    let ctx = RepoContext::load()?;
    let selector = doctor::go_version_selector(&ctx)?;
    if json_output {
        print_json(&serde_json::json!({
            "ok": true,
            "command": "info go-version",
            "version": selector,
        }))?;
    } else {
        writeln!(std::io::stdout().lock(), "{selector}")?;
    }
    Ok(())
}
