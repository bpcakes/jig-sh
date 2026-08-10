use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};
use jig_codex_tui::{Home, HomeUpdate, InspectionSource};
use jig_tui::sanitize_text;

use super::codex::{CodexCommand, CodexLaunchOpts, CodexResumeOpts};
use super::output::{HumanOutput, emit};
use crate::progress::CliProgress;

pub(super) fn run_codex_command(command: CodexCommand, json_output: bool) -> Result<()> {
    match command {
        CodexCommand::Homes(opts) => {
            let report = if json_output {
                crate::codex::homes_report(opts.usage)?
            } else {
                homes_report_with_cli_progress(opts.usage)?
            };
            emit(json_output, HumanOutput::CodexHomes, &report)
        }
        CodexCommand::Launch(opts) => run_codex_launch(opts, json_output),
        CodexCommand::Resume(opts) => run_codex_resume(opts, json_output),
    }
}

fn run_codex_resume(opts: CodexResumeOpts, json_output: bool) -> Result<()> {
    if json_output && !opts.dry_run {
        bail!("--json can be used with `jig codex resume` only when --dry-run is present");
    }
    let session_id = crate::codex::normalize_session_id(&opts.session_id)?;
    let home = match opts.home {
        Some(home) => crate::codex::resolve_launch_home(&home)?,
        None if json_output => crate::codex::resolve_resume_home(&session_id)?,
        None => resolve_resume_home_with_cli_progress(&session_id)?,
    };
    let codex_args = resume_codex_args(session_id, opts.codex_args);

    if opts.dry_run {
        let report = crate::codex::resume_dry_run_report(&home, &codex_args);
        return emit(json_output, HumanOutput::CodexResume, &report);
    }
    crate::codex::launch(&home, &codex_args)
}

fn resolve_resume_home_with_cli_progress(session_id: &str) -> Result<PathBuf> {
    let progress = CliProgress::new("codex resume");
    progress.header("find the Codex home containing the session");
    let result = crate::codex::resolve_resume_home_with_progress(session_id, |completed, total| {
        progress.step("inspect homes", format!("{completed}/{total}"))
    });
    let home = progress.log_blocked_on_err(result)?;
    progress.done("found the session's Codex home");
    Ok(home)
}

fn resume_codex_args(
    session_id: String,
    forwarded: Vec<std::ffi::OsString>,
) -> Vec<std::ffi::OsString> {
    let mut args = vec!["resume".into(), session_id.into()];
    args.extend(forwarded);
    args
}

fn homes_report_with_cli_progress(include_usage: bool) -> Result<serde_json::Value> {
    let progress = CliProgress::new("codex homes");
    progress.header("inspect local Codex homes");
    let report = crate::codex::homes_report_with_progress(include_usage, |completed, total, _| {
        progress.step("inspect homes", format!("{completed}/{total}"));
        Ok(())
    });
    let (report, _) = progress.log_blocked_on_err(report)?;
    let home_count = report["homes"].as_array().map_or(0, Vec::len);
    progress.done(format!("inspected {home_count} Codex homes"));
    Ok(report)
}

fn run_codex_launch(opts: CodexLaunchOpts, json_output: bool) -> Result<()> {
    if json_output && !opts.dry_run {
        bail!("--json can be used with `jig codex launch` only when --dry-run is present");
    }
    let home = match opts.home {
        Some(home) => crate::codex::resolve_launch_home(&home)?,
        None => {
            let Some(home) = select_home_interactively()? else {
                return Ok(());
            };
            home
        }
    };

    if opts.dry_run {
        let report = crate::codex::dry_run_report(&home, &opts.codex_args);
        return emit(json_output, HumanOutput::CodexLaunch, &report);
    }
    crate::codex::launch(&home, &opts.codex_args)
}

fn select_home_interactively() -> Result<Option<PathBuf>> {
    let inspection = crate::codex::discover_home_inspection()?;
    let homes = inspection
        .candidates()
        .into_iter()
        .map(|candidate| Home {
            path: candidate.path,
            name: candidate.name,
            current: candidate.current,
        })
        .collect::<Vec<_>>();
    if homes.is_empty() {
        return Err(no_codex_homes_error(&inspection.discovery_warnings()));
    }
    select_picker(homes, PickerInspectionSource(inspection))?
        .map(|path| revalidate_picker_home(&path))
        .transpose()
}

fn no_codex_homes_error(warnings: &[String]) -> anyhow::Error {
    const MESSAGE: &str = "No Codex homes found under ~/.codex or ~/.codex-*";
    if warnings.is_empty() {
        return anyhow!(MESSAGE);
    }
    let warnings = warnings
        .iter()
        .map(|warning| format!("  - {}", sanitize_text(warning)))
        .collect::<Vec<_>>()
        .join("\n");
    anyhow!("{MESSAGE}\nDiscovery warnings:\n{warnings}")
}

fn select_picker(
    homes: Vec<Home>,
    source: impl InspectionSource + 'static,
) -> Result<Option<PathBuf>> {
    #[cfg(all(unix, not(test)))]
    {
        let signal_session = crate::doctor::DoctorSignalSession::start().map_err(|_| {
            anyhow::anyhow!(
                "Codex home picker was not started because the process-wide signal session is unavailable"
            )
        })?;
        let cancellation = signal_session.cancellation();
        let result = jig_codex_tui::select_with_cancellation(homes, source, move || {
            cancellation.cancelled()
        });
        crate::codex::finish_signal_supervised(
            result,
            signal_session.finish(),
            "Codex home picker signal supervision could not retire safely",
        )
    }
    #[cfg(any(not(unix), test))]
    {
        jig_codex_tui::select(homes, source)
    }
}

struct PickerInspectionSource(crate::codex::CodexHomeInspection);

impl InspectionSource for PickerInspectionSource {
    fn discovery_warnings(&self) -> Vec<String> {
        self.0.discovery_warnings()
    }

    fn inspect(
        &self,
        emit: &mut dyn FnMut(HomeUpdate) -> std::result::Result<(), String>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> std::result::Result<(), String> {
        self.0
            .inspect(cancelled, |index, details| {
                emit(HomeUpdate { index, details }).map_err(anyhow::Error::msg)
            })
            .map_err(|error| error.to_string())
    }
}

fn revalidate_picker_home(path: &std::path::Path) -> Result<PathBuf> {
    crate::codex::resolve_launch_home(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_discovery_error_retains_sanitized_warnings() {
        let error = no_codex_homes_error(&[
            "permission denied for /tmp/.codex-private".into(),
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

    #[test]
    fn real_launch_rejects_json_before_resolving_or_starting_codex() {
        let error = run_codex_launch(
            CodexLaunchOpts {
                home: Some(PathBuf::from("does-not-need-to-exist")),
                dry_run: false,
                codex_args: Vec::new(),
            },
            true,
        )
        .unwrap_err();

        assert!(error.to_string().contains("only when --dry-run is present"));
    }

    #[test]
    fn real_resume_rejects_json_before_validating_or_resolving() {
        let error = run_codex_resume(
            CodexResumeOpts {
                session_id: "not-a-session-id".into(),
                home: Some(PathBuf::from("does-not-need-to-exist")),
                dry_run: false,
                codex_args: Vec::new(),
            },
            true,
        )
        .unwrap_err();

        assert!(error.to_string().contains("only when --dry-run is present"));
    }

    #[test]
    fn resume_arguments_keep_the_normalized_id_before_forwarded_arguments() {
        let args = resume_codex_args(
            "019fe6e4-972f-7392-aaf3-58cb652a4e20".into(),
            vec!["--search".into(), "prompt with spaces".into()],
        );

        assert_eq!(
            args,
            [
                "resume",
                "019fe6e4-972f-7392-aaf3-58cb652a4e20",
                "--search",
                "prompt with spaces",
            ]
            .map(std::ffi::OsString::from)
        );
    }

    #[test]
    fn picker_home_is_revalidated_after_inspection() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join(".codex-work");
        std::fs::create_dir(&home).unwrap();

        assert_eq!(
            revalidate_picker_home(&home).unwrap(),
            home.canonicalize().unwrap()
        );
        std::fs::remove_dir(&home).unwrap();
        assert!(revalidate_picker_home(&home).is_err());
    }
}
