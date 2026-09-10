use std::ffi::OsString;
use std::path::Path;

use anyhow::{Context, Result, bail};
use jig_codex_tui::{ConfigurationHome, Home, HomeUpdate, InspectionSource};

use super::output::{HumanOutput, emit};
use crate::agent_provider::{AgentProvider, Discovery, HomeInspection, PreparedLaunch};

pub(super) fn homes<P: AgentProvider>(
    provider: &P,
    usage: bool,
    json: bool,
    output: HumanOutput,
) -> Result<()> {
    if usage && !P::METADATA.usage {
        bail!(
            "{} does not support subscription usage inspection",
            P::METADATA.name
        );
    }
    let progress = if !json && (usage || P::METADATA.inspect_on_list) {
        crate::progress::CliProgress::new(P::METADATA.homes_command)
    } else {
        crate::progress::CliProgress::disabled(P::METADATA.homes_command)
    };
    progress.header(format!("inspect local {} homes", P::METADATA.name));
    let report = super::home_picker::supervise(|cancelled| {
        provider.homes_report(usage, &cancelled, &mut |done, total| {
            progress.step("inspect homes", format!("{done}/{total}"));
        })
    });
    let report = progress.log_blocked_on_err(report)?;
    progress.done(format!(
        "inspected {} {} homes",
        report["homes"].as_array().map_or(0, Vec::len),
        P::METADATA.name
    ));
    emit(json, output, &report)
}

pub(super) fn launch<P: AgentProvider>(
    provider: &P,
    home: Option<&Path>,
    args: &[OsString],
    dry_run: bool,
    json: bool,
    output: HumanOutput,
) -> Result<()> {
    validate_launch::<P>(dry_run, json)?;
    let home = match home {
        Some(path) => provider.resolve(path)?,
        None if json => bail!(
            "Pass a {} HOME when using --json; use `jig {} homes --json` to list homes",
            P::METADATA.name,
            P::METADATA.command
        ),
        None => {
            let Some(home) = select(provider)? else {
                return Ok(());
            };
            home
        }
    };
    finish::<P>(provider.prepare(&home, args)?, dry_run, json, output)
}

pub(super) fn validate_launch<P: AgentProvider>(dry_run: bool, json: bool) -> Result<()> {
    if json && !dry_run {
        bail!(
            "--json can be used with `jig {} launch` only when --dry-run is present",
            P::METADATA.command
        );
    }
    Ok(())
}

pub(super) fn finish<P: AgentProvider>(
    mut prepared: PreparedLaunch,
    dry_run: bool,
    json: bool,
    output: HumanOutput,
) -> Result<()> {
    if dry_run {
        return emit(json, output, &prepared.report);
    }
    crate::agent_launch::launch(&mut prepared.command, P::METADATA.name, || {
        prepared.error_context
    })
}

fn select<P: AgentProvider>(provider: &P) -> Result<Option<P::Home>> {
    let command = format!("jig {} launch", P::METADATA.command);
    jig_tui::require_terminal(
        &command,
        &format!(
            "pass HOME explicitly (see `jig {} homes`)",
            P::METADATA.command
        ),
    )?;
    select_with(provider, |entries, source, warnings| {
        super::home_picker::supervise(|cancelled| {
            jig_codex_tui::select_provider_with_cancellation(
                &format!("{} Home Picker", P::METADATA.name),
                &command,
                entries,
                warnings,
                source,
                P::METADATA.subscription_bucket,
                cancelled,
            )
        })
    })
}

fn select_with<P: AgentProvider>(
    provider: &P,
    pick: impl FnOnce(
        Vec<ConfigurationHome>,
        Option<PickerInspection>,
        Vec<String>,
    ) -> Result<Option<usize>>,
) -> Result<Option<P::Home>> {
    let Discovery {
        choices,
        warnings,
        inspection,
    } = provider.discover()?;
    let (selections, entries): (Vec<_>, Vec<_>) = choices
        .into_iter()
        .map(|choice| {
            (
                choice.selection,
                ConfigurationHome {
                    home: Home {
                        path: choice.path,
                        name: choice.name,
                        current: choice.current,
                    },
                    details: choice.details,
                },
            )
        })
        .unzip();
    let selected = pick(
        entries,
        inspection.map(|source| PickerInspection { source }),
        warnings,
    )?;
    selected
        .map(|index| {
            let home = selections
                .get(index)
                .context("Home picker returned an unknown configuration")?;
            provider.revalidate(home)
        })
        .transpose()
}

struct PickerInspection {
    source: Box<dyn HomeInspection>,
}

impl InspectionSource for PickerInspection {
    fn inspect(
        &self,
        emit: &mut dyn FnMut(HomeUpdate) -> Result<(), String>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<(), String> {
        self.source.inspect(
            &mut |index, details| emit(HomeUpdate { index, details }),
            cancelled,
        )
    }
}

#[cfg(test)]
mod tests;
