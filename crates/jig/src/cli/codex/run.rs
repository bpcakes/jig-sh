use std::path::PathBuf;

use anyhow::{Result, bail};
use jig_agents::agent_provider::{AgentProvider, SessionProvider};
use jig_agents::codex::provider::Codex;

use super::{CodexCommand, CodexLaunchOpts, CodexResumeOpts, render};
use crate::cli::agent_run;
use crate::progress::CliProgress;

pub(in crate::cli) fn run_codex_command(command: CodexCommand, json_output: bool) -> Result<()> {
    match command {
        CodexCommand::Homes(opts) => agent_run::homes(
            &Codex,
            opts.usage,
            json_output,
            render::format_codex_homes_summary,
        ),
        CodexCommand::Launch(opts) => run_codex_launch(opts, json_output),
        CodexCommand::Resume(opts) => run_codex_resume(opts, json_output),
    }
}

fn run_codex_resume(opts: CodexResumeOpts, json_output: bool) -> Result<()> {
    if json_output && !opts.dry_run {
        bail!("--json can be used with `jig codex resume` only when --dry-run is present");
    }
    let session_id = jig_agents::codex::normalize_session_id(&opts.session_id)?;
    let home = match opts.home {
        Some(home) => Codex.resolve(&home)?,
        None if json_output => {
            resolve_session_with_signal_supervision(&session_id, &mut |_, _| {})?
        }
        None => resolve_resume_home_with_cli_progress(&session_id)?,
    };
    let codex_args = resume_codex_args(session_id, opts.codex_args);

    let mut prepared = Codex.prepare(&home, &codex_args)?;
    prepared.report = jig_agents::codex::resume_dry_run_report(&home, &codex_args);
    agent_run::finish::<Codex>(
        prepared,
        opts.dry_run,
        json_output,
        render::format_codex_resume_summary,
    )
}

fn resolve_resume_home_with_cli_progress(session_id: &str) -> Result<PathBuf> {
    let progress = CliProgress::new("codex resume");
    progress.header("find the Codex home containing the session");
    let result = resolve_session_with_signal_supervision(session_id, &mut |completed, total| {
        progress.step("inspect homes", format!("{completed}/{total}"))
    });
    let home = progress.log_blocked_on_err(result)?;
    progress.done("found the session's Codex home");
    Ok(home)
}

fn resolve_session_with_signal_supervision(
    session_id: &str,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<PathBuf> {
    crate::signal_supervision::supervise(
        "Codex session lookup was not started because the process-wide signal session is unavailable",
        "Codex session lookup signal supervision could not retire safely",
        |cancelled| Codex.resolve_session(session_id, &cancelled, progress),
    )
}

fn resume_codex_args(
    session_id: String,
    forwarded: Vec<std::ffi::OsString>,
) -> Vec<std::ffi::OsString> {
    let mut args = vec!["resume".into(), session_id.into()];
    args.extend(forwarded);
    args
}

fn run_codex_launch(opts: CodexLaunchOpts, json_output: bool) -> Result<()> {
    agent_run::launch(
        &Codex,
        opts.home.as_deref(),
        &opts.codex_args,
        opts.dry_run,
        json_output,
        render::format_codex_launch_summary,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Codex.revalidate(&home).unwrap(),
            home.canonicalize().unwrap()
        );
        std::fs::remove_dir(&home).unwrap();
        assert!(Codex.revalidate(&home).is_err());
    }
}
