use anyhow::{Result, bail};
use jig_codex_tui::{ConfigurationHome, Home, HomeUpdate, InspectionSource};

use super::claude::ClaudeCommand;
use super::output::{HumanOutput, emit};
use crate::claude;

pub(super) fn run_claude_command(command: ClaudeCommand, json_output: bool) -> Result<()> {
    match command {
        ClaudeCommand::Homes(opts) => emit(
            json_output,
            HumanOutput::ClaudeHomes,
            &if opts.usage {
                super::home_picker::supervise(|cancelled| {
                    claude::usage::report(claude::discover_homes()?, &cancelled)
                })?
            } else {
                claude::discover_homes()?.report()
            },
        ),
        ClaudeCommand::Launch(opts) => {
            if json_output && !opts.dry_run {
                bail!("--json can be used with `jig claude launch` only when --dry-run is present");
            }
            let home = match opts.home {
                Some(home) => claude::resolve_home(&home)?,
                None if json_output => bail!(
                    "Pass a Claude HOME when using --json; use `jig claude homes --json` to list homes"
                ),
                None => {
                    let Some(home) = select_home()? else {
                        return Ok(());
                    };
                    home
                }
            };
            if opts.dry_run {
                return emit(
                    json_output,
                    HumanOutput::ClaudeLaunch,
                    &claude::dry_run_report(&home, &opts.claude_args),
                );
            }
            claude::launch(&home, &opts.claude_args)
        }
    }
}

fn select_home() -> Result<Option<claude::Home>> {
    jig_tui::require_terminal(
        "jig claude launch",
        "pass HOME explicitly (see `jig claude homes`)",
    )?;
    let homes = claude::discover_homes()?;
    let selections = homes.selections();
    let entries = selections
        .iter()
        .map(|home| ConfigurationHome {
            home: Home {
                path: home.path.clone(),
                name: format!(
                    "{}{}",
                    crate::home_paths::home_name(&home.path),
                    if home.default_config {
                        " [default config]"
                    } else {
                        ""
                    },
                ),
                current: homes.is_current(home),
            },
            details: vec![
                (
                    "Configuration".into(),
                    if home.default_config {
                        "Native default".into()
                    } else {
                        "Explicit override".into()
                    },
                ),
                (
                    "CLAUDE_CONFIG_DIR".into(),
                    if home.default_config {
                        "unset".into()
                    } else {
                        home.path.to_string_lossy().into_owned()
                    },
                ),
            ],
        })
        .collect();
    let selected = super::home_picker::supervise(|cancelled| {
        jig_codex_tui::select_inspected_configuration_with_cancellation(
            "Claude Home Picker",
            "jig claude launch",
            entries,
            PickerInspection(claude::usage::Inspection::new(
                selections.clone(),
                homes.warnings,
                true,
            )),
            cancelled,
        )
    })?;
    selected
        .map(|index| {
            let home = &selections[index];
            if home.default_config {
                Ok(claude::Home {
                    path: home.path.clone(),
                    default_config: true,
                })
            } else {
                claude::validate_home(&home.path)
            }
        })
        .transpose()
}

struct PickerInspection(claude::usage::Inspection);

impl InspectionSource for PickerInspection {
    fn discovery_warnings(&self) -> Vec<String> {
        self.0.warnings.clone()
    }

    fn inspect(
        &self,
        emit: &mut dyn FnMut(HomeUpdate) -> Result<(), String>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<(), String> {
        self.0.inspect(
            &mut |index, details| emit(HomeUpdate { index, details }),
            cancelled,
        )
    }
}
