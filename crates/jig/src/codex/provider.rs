use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::agent_provider::{
    AgentProvider, Choice, Discovery, HomeInspection, Metadata, PreparedLaunch, SessionProvider,
};

pub(crate) struct Codex;

impl AgentProvider for Codex {
    type Home = PathBuf;
    const METADATA: Metadata = Metadata {
        command: "codex",
        homes_command: "codex homes",
        name: "Codex",
        executable: "codex",
        executable_env: "JIG_CODEX_BIN",
        usage: true,
        inspect_on_list: true,
        subscription_bucket: Some("codex"),
    };

    fn resolve(&self, input: &Path) -> Result<Self::Home> {
        super::resolve_launch_home(input)
    }
    fn revalidate(&self, home: &Self::Home) -> Result<Self::Home> {
        self.resolve(home)
    }

    fn discover(&self) -> Result<Discovery<Self::Home>> {
        let inspection = super::discover_home_inspection()?;
        let warnings = inspection.discovery_warnings();
        let choices = inspection
            .candidates()
            .into_iter()
            .map(|home| Choice {
                selection: home.path.clone(),
                path: home.path,
                name: home.name,
                current: home.current,
                details: Vec::new(),
            })
            .collect::<Vec<_>>();
        if choices.is_empty() {
            return Err(no_homes_error(&warnings));
        }
        Ok(Discovery {
            choices,
            warnings,
            inspection: Some(Box::new(inspection)),
        })
    }

    fn prepare(&self, home: &Self::Home, args: &[OsString]) -> Result<PreparedLaunch> {
        let bin = Self::METADATA.executable();
        let mut command = Command::new(&bin);
        command.args(args).env(super::CODEX_HOME_ENV, home);
        Ok(PreparedLaunch {
            command,
            report: super::dry_run_report(home, args),
            error_context: format!(
                "Failed to launch {} with CODEX_HOME={}",
                bin.to_string_lossy(),
                home.display()
            ),
        })
    }

    fn homes_report(
        &self,
        usage: bool,
        cancelled: &(dyn Fn() -> bool + Sync),
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Value> {
        super::homes_report_with_progress_and_cancellation(
            usage,
            |done, total, _| {
                progress(done, total);
                Ok(())
            },
            cancelled,
        )
        .map(|(report, _)| report)
    }
}

impl SessionProvider for Codex {
    fn resolve_session(
        &self,
        session: &str,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Self::Home> {
        super::resolve_resume_home_with_progress(session, progress)
    }
}

impl HomeInspection for super::CodexHomeInspection {
    fn inspect(
        &self,
        emit: &mut dyn FnMut(usize, Value) -> Result<(), String>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<(), String> {
        self.inspect(cancelled, |index, details| {
            emit(index, details).map_err(anyhow::Error::msg)
        })
        .map_err(|error| error.to_string())
    }
}

fn no_homes_error(warnings: &[String]) -> anyhow::Error {
    let mut message = "No Codex homes found under ~/.codex or ~/.codex-*".to_owned();
    if !warnings.is_empty() {
        message.push_str("\nDiscovery warnings:\n");
        message.push_str(
            &warnings
                .iter()
                .map(|warning| format!("  - {}", jig_tui::sanitize_text(warning)))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    anyhow!(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_discovery_error_retains_sanitized_warnings() {
        let error = no_homes_error(&[
            "permission denied for /tmp/.codex-example".into(),
            "unsafe\u{1b}[2J\nmessage".into(),
        ])
        .to_string();
        assert!(error.contains("No Codex homes found"));
        assert!(error.contains("Discovery warnings:"));
        assert!(error.contains("permission denied"));
        assert!(!error.contains('\u{1b}'));
        assert!(!error.contains("\nmessage"));
        assert!(error.contains("unsafe\u{fffd}[2J\u{fffd}message"));
    }
}
