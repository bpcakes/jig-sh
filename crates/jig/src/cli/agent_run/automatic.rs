use std::ffi::OsString;

use anyhow::{Result, bail};
use jig_agents::agent_provider::AgentProvider;
use serde_json::{Value, json};

use super::super::output::{Render, emit};

pub(super) fn launch<P: AgentProvider>(
    provider: &P,
    args: &[OsString],
    dry_run: bool,
    json_output: bool,
    output: Render,
) -> Result<()> {
    let selection = crate::signal_supervision::supervise(
        "Automatic home selection could not start signal supervision",
        "Automatic home selection signal supervision could not retire safely",
        |cancelled| jig_agents::selection::select(provider, &cancelled),
    )?;
    let Some(home) = selection.home else {
        let message = format!(
            "No eligible {} home with fresh subscription headroom; inspect `jig {} homes --usage` or pass HOME explicitly",
            P::METADATA.name,
            P::METADATA.command,
        );
        if json_output {
            emit(
                true,
                output,
                &json!({
                    "ok": false,
                    "schema_version": 1,
                    "command": format!("{} launch", P::METADATA.command),
                    "error": { "kind": "no_eligible_home", "message": message },
                    "selection": selection.report,
                    "exit_status": 1,
                }),
            )?;
            return Err(crate::exit::CliExit::reported(1, message).into());
        }
        bail!("{message}");
    };
    let mut prepared = provider.prepare(&home, args)?;
    prepared.report["selection"] = selection.report;
    if dry_run && !json_output {
        let summary = format!(
            "{}\n{}",
            output(&prepared.report),
            summary(&prepared.report)
        );
        return super::super::output::print_text(&summary);
    }
    super::finish::<P>(prepared, dry_run, json_output, output)
}

fn summary(value: &Value) -> String {
    let selection = &value["selection"];
    let selected = selection["selected_index"].as_u64().unwrap_or_default() as usize;
    let candidate = &selection["candidates"][selected];
    let remaining = candidate["assessment"]["remaining_percent"]
        .as_f64()
        .unwrap_or_default();
    format!(
        "  Auto selection: {} remaining in the limiting window{}\n  Full selection decision: rerun with --dry-run --json",
        jig_tui::format_percent(remaining),
        if selection["partial"] == true {
            " (partial inspection)"
        } else {
            ""
        },
    )
}
