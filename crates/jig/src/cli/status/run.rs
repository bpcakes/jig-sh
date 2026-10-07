use anyhow::Result;
use jig_context::RepoContext;

use super::render;
use super::{StatusCommand, StatusOpts};
use crate::cli::output::emit;
use crate::{status, ui};

pub(in crate::cli) fn run_status_command(opts: StatusOpts, json_output: bool) -> Result<()> {
    let ctx = RepoContext::load()?;
    if let Some(StatusCommand::Run { run_id }) = &opts.command {
        let output = status_run_output(&ctx, run_id)?;
        return emit(json_output, render::format_run_status_summary, &output);
    }
    if opts.tui {
        return ui::run_status(
            ctx,
            std::time::Duration::from_secs(opts.effective_refresh_seconds()),
        );
    }
    #[cfg(all(unix, not(test)))]
    let signal_session = crate::signal_supervision::SignalSession::start().map_err(|_| {
        anyhow::anyhow!("Status was not started because signal supervision is unavailable")
    })?;
    #[cfg(all(unix, not(test)))]
    let cancellation = signal_session.cancellation();
    #[cfg(all(unix, not(test)))]
    let outcome = status::snapshot_with_cancellation(&ctx, &|| cancellation.cancelled());
    #[cfg(all(unix, not(test)))]
    let outcome = crate::signal_supervision::finish(
        outcome,
        signal_session.finish(),
        "Status signal supervision could not retire safely",
    );
    #[cfg(any(not(unix), test))]
    let outcome = status::snapshot_with_cancellation(&ctx, &|| false);
    let output = outcome?;
    emit(json_output, render::format_summary, &output)
}

fn status_run_output(ctx: &RepoContext, run_id: &str) -> Result<serde_json::Value> {
    let run = crate::state::reconcile_run_for_inspection(ctx, run_id)?;
    let mut output = serde_json::to_value(run)?;
    output["ok"] = serde_json::json!(true);
    output["command"] = serde_json::json!("status run");
    Ok(output)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::test_env::TestRepoBuilder;

    #[test]
    fn status_run_reconciles_an_abandoned_worker_before_rendering() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(["rust_test_command"])
            .write();
        let ctx = jig_context::RepoContext::load_from(temp.path()).unwrap();
        let target: jig_contract::TargetId = "repo:test".parse().unwrap();
        let plan = jig_contract::RunPlan::new(
            "run-plan_1",
            "sha256:config",
            jig_contract::SourceIdentity::new(None, "sha256:worktree"),
            vec![jig_contract::PlannedTarget::new(
                target.clone(),
                jig_contract::ActionIntent::Check,
                jig_contract::ActionRunner::command("rust_test_command"),
                "sha256:input",
            )],
            vec![vec![target]],
        );
        let (started, lease) = crate::state::start_run(&ctx, plan).unwrap();
        drop(lease);

        let output = status_run_output(&ctx, &started.result.run_id).unwrap();

        assert_eq!(output["command"], "status run");
        assert_eq!(output["result"]["status"], "completed");
        assert_eq!(output["result"]["conclusion"], "blocked");
    }
}
