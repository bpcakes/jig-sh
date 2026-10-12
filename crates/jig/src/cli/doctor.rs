//! `jig doctor`: the read-only readiness report.

use anyhow::Result;

use super::output::emit;
use super::run::finish_after_json_output;
use super::structured_error::require_json_ok;

pub(super) mod render;

pub(super) const DOCTOR_AFTER_HELP: &str = "\
Runs the read-only readiness checks that are otherwise split across bootstrap,
agent doctor, check contract, proxy status, and vault status.

Human-readable output is the default. Pass --json for structured automation output.

Examples:
  jig doctor
  jig doctor --json";

pub(super) fn run_doctor_command(json_output: bool) -> Result<()> {
    let output = crate::doctor::run()?;
    emit(json_output, render::format_doctor_summary, &output)?;
    finish_after_json_output(require_json_ok(true, &output), json_output)
}
