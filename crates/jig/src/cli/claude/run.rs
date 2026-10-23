use std::ffi::OsString;

use anyhow::{Result, bail};
use jig_agents::agent_provider::{AgentProvider, SessionProvider};
use jig_agents::claude::Home;
use jig_agents::claude::provider::Claude;
use jig_execution::progress::CliProgress;

use super::{ClaudeCommand, ClaudeResumeOpts, render};
use crate::cli::agent_run;

pub(in crate::cli) fn run_claude_command(command: ClaudeCommand, json_output: bool) -> Result<()> {
    match command {
        ClaudeCommand::Homes(opts) => {
            agent_run::homes(&Claude, opts.usage, json_output, render::homes_summary)
        }
        ClaudeCommand::Launch(opts) => agent_run::launch(
            &Claude,
            opts.home.as_deref(),
            &opts.claude_args,
            opts.auto_select,
            opts.dry_run,
            json_output,
            render::launch_summary,
        ),
        ClaudeCommand::Resume(opts) => run_claude_resume(opts, json_output),
    }
}

fn run_claude_resume(opts: ClaudeResumeOpts, json_output: bool) -> Result<()> {
    if json_output && !opts.dry_run {
        bail!("--json can be used with `jig claude resume` only when --dry-run is present");
    }
    let session_id = jig_agents::claude::normalize_session_id(&opts.session_id)?;
    let home = match opts.home {
        Some(home) => Claude.resolve(&home)?,
        None if json_output => {
            resolve_session_with_signal_supervision(&session_id, &mut |_, _| {})?
        }
        None => resolve_resume_home_with_cli_progress(&session_id)?,
    };
    let claude_args = resume_claude_args(session_id, opts.claude_args);

    let mut prepared = Claude.prepare(&home, &claude_args)?;
    prepared.report = jig_agents::claude::resume_dry_run_report(&home, &claude_args);
    agent_run::finish::<Claude>(prepared, opts.dry_run, json_output, render::resume_summary)
}

fn resolve_resume_home_with_cli_progress(session_id: &str) -> Result<Home> {
    let progress = CliProgress::new("claude resume");
    progress.header("find the Claude home containing the session");
    let result = resolve_session_with_signal_supervision(session_id, &mut |completed, total| {
        progress.step("inspect homes", format!("{completed}/{total}"))
    });
    let home = progress.log_blocked_on_err(result)?;
    progress.done("found the session's Claude home");
    Ok(home)
}

fn resolve_session_with_signal_supervision(
    session_id: &str,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Home> {
    crate::signal_supervision::supervise(
        "Claude session lookup was not started because the process-wide signal session is unavailable",
        "Claude session lookup signal supervision could not retire safely",
        |cancelled| Claude.resolve_session(session_id, &cancelled, progress),
    )
}

fn resume_claude_args(session_id: String, forwarded: Vec<OsString>) -> Vec<OsString> {
    let mut args = vec!["--resume".into(), session_id.into()];
    args.extend(forwarded);
    args
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn real_resume_rejects_json_before_validating_or_resolving() {
        let error = run_claude_resume(
            ClaudeResumeOpts {
                session_id: "not-a-session-id".into(),
                home: Some(PathBuf::from("does-not-need-to-exist")),
                dry_run: false,
                claude_args: Vec::new(),
            },
            true,
        )
        .unwrap_err();

        assert!(error.to_string().contains("only when --dry-run is present"));
    }

    #[test]
    fn resume_arguments_keep_the_normalized_id_before_forwarded_arguments() {
        let args = resume_claude_args(
            "2f4c6e8a-1b3d-4f5a-8c7e-9d0b1a2c3e4f".into(),
            vec!["--model".into(), "prompt with spaces".into()],
        );

        assert_eq!(
            args,
            [
                "--resume",
                "2f4c6e8a-1b3d-4f5a-8c7e-9d0b1a2c3e4f",
                "--model",
                "prompt with spaces",
            ]
            .map(OsString::from)
        );
    }
}
