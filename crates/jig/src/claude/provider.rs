use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use anyhow::Result;
use serde_json::Value;

use crate::agent_provider::{
    AgentProvider, Choice, Discovery, HomeInspection, Metadata, PreparedLaunch,
};

pub(crate) struct Claude;

impl AgentProvider for Claude {
    type Home = super::Home;
    const METADATA: Metadata = Metadata {
        command: "claude",
        homes_command: "claude homes",
        name: "Claude",
        executable: "claude",
        executable_env: "JIG_CLAUDE_BIN",
        usage: true,
        inspect_on_list: false,
        subscription_bucket: Some("claude"),
    };

    fn resolve(&self, input: &Path) -> Result<Self::Home> {
        super::resolve_home(input)
    }

    fn discover(&self) -> Result<Discovery<Self::Home>> {
        let homes = super::discover_homes()?;
        let selections = homes.selections();
        let choices = selections
            .iter()
            .map(|home| Choice {
                selection: home.clone(),
                path: home.path.clone(),
                name: format!(
                    "{}{}",
                    crate::home_paths::home_name(&home.path),
                    if home.default_config {
                        " [default config]"
                    } else {
                        ""
                    }
                ),
                current: homes.is_current(home),
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
        Ok(Discovery {
            choices,
            warnings: homes.warnings,
            inspection: Some(Box::new(super::usage::Inspection::new(selections, true))),
        })
    }

    fn revalidate(&self, home: &Self::Home) -> Result<Self::Home> {
        if home.default_config {
            Ok(home.clone())
        } else {
            super::validate_home(&home.path)
        }
    }

    fn prepare(&self, home: &Self::Home, args: &[OsString]) -> Result<PreparedLaunch> {
        let mut command = Command::new(Self::METADATA.executable());
        command.args(args);
        if home.default_config {
            command.env_remove(super::CONFIG_DIR_ENV);
        } else {
            command.env(super::CONFIG_DIR_ENV, &home.path);
        }
        Ok(PreparedLaunch {
            command,
            report: super::dry_run_report(home, args),
            error_context: "Failed to launch Claude; install claude or set JIG_CLAUDE_BIN".into(),
        })
    }

    fn homes_report(
        &self,
        usage: bool,
        cancelled: &(dyn Fn() -> bool + Sync),
        _progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Value> {
        let homes = super::discover_homes()?;
        if usage {
            super::usage::report(homes, cancelled)
        } else {
            Ok(homes.report())
        }
    }
}

impl HomeInspection for super::usage::Inspection {
    fn inspect(
        &self,
        emit: &mut dyn FnMut(usize, Value) -> Result<(), String>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<(), String> {
        self.inspect(emit, cancelled)
    }
}
