//! `jig dev` and `jig proxy` in binaries built without the `dev-proxy`
//! feature: both report the missing feature without consulting a repository.

use anyhow::Result;

use super::finish_after_json_output;
use crate::cli::output::{HumanOutput, emit};
use crate::cli::structured_error::require_foreground_status;
use crate::cli::{DevOpts, ProxyCommand};
use crate::dev_proxy::commands::{dev_without_context, proxy_without_context};

pub(super) fn run_dev_command(opts: DevOpts, json_output: bool) -> Result<()> {
    let human_output = opts.human_output();
    let output = dev_without_context(opts.into())?;
    emit(json_output, human_output, &output)?;
    finish_after_json_output(require_foreground_status(&output), json_output)
}

pub(super) fn run_proxy_command(command: ProxyCommand, json_output: bool) -> Result<()> {
    let output = proxy_without_context(command.into())?;
    emit(json_output, HumanOutput::Proxy, &output)?;
    finish_after_json_output(require_foreground_status(&output), json_output)
}
