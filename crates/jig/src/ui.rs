//! `jig ui`: the unified read-only terminal dashboard and one-shot recorder.

use std::cell::Cell;
use std::io::Write;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use jig_ui::dashboard::{
    DashboardSource, PlanBasis, PlanSnapshotResult, RecorderMode, RecorderRequest, TimelineLimit,
};
use jig_ui::terminal::{DashboardOptions, InitialTab};
use jig_ui::{DashboardSnapshot, PlanSnapshot, SnapshotProvider, UiQuery, UiServer};

use crate::cli::UiOpts;
use crate::context::RepoContext;

mod snapshot;
mod source;

pub(crate) use source::RepoDashboardSource;

#[allow(dead_code)]
pub(crate) const DEFAULT_UI_PORT: u16 = jig_ui::DEFAULT_UI_PORT;

impl SnapshotProvider for RepoContext {
    fn dashboard_snapshot(&self, query: UiQuery) -> Result<DashboardSnapshot> {
        let current = crate::runtime::refreshed_repository_context(self)?;
        snapshot::snapshot_with_query(&current, query)
    }

    fn plan_snapshot(&self, plan_id: &str) -> Result<Option<PlanSnapshot>> {
        let current = crate::runtime::refreshed_repository_context(self)?;
        snapshot::plan_snapshot(&current, plan_id)
    }
}

pub(crate) fn run(ctx: RepoContext, opts: UiOpts, json_output: bool) -> Result<()> {
    let timeline_limit = usize::try_from(opts.effective_timeline_limit())
        .ok()
        .and_then(|rows| TimelineLimit::new(rows).ok())
        .context("the validated timeline limit was outside the runtime range")?;
    if json_output {
        let output_started = Cell::new(false);
        let result = supervised(|cancelled| {
            let document = json_document(ctx, opts.plan, timeline_limit, cancelled)?;
            output_started.set(true);
            write_json(&document)
        });
        return finish_json_result(result, output_started.get());
    }
    let options = work_dashboard_options(opts, timeline_limit)?;
    supervised(|cancelled| {
        jig_ui::terminal::run_with_cancellation(RepoDashboardSource::new(ctx), options, cancelled)
    })
}

fn work_dashboard_options(opts: UiOpts, timeline_limit: TimelineLimit) -> Result<DashboardOptions> {
    Ok(DashboardOptions::with_refresh_intervals(
        InitialTab::Work,
        Duration::from_secs(opts.effective_refresh_seconds()),
        Duration::from_secs(opts.effective_status_refresh_seconds()),
    )
    .with_timeline_limit(timeline_limit.get())
    .context("the validated timeline limit was outside the terminal range")?
    .with_initial_plan(opts.plan))
}

pub(crate) fn run_status(ctx: RepoContext, status_refresh_interval: Duration) -> Result<()> {
    let options = status_dashboard_options(status_refresh_interval);
    supervised(|cancelled| {
        jig_ui::terminal::run_with_cancellation(RepoDashboardSource::new(ctx), options, cancelled)
    })
}

fn status_dashboard_options(status_refresh_interval: Duration) -> DashboardOptions {
    DashboardOptions::with_refresh_intervals(
        InitialTab::Status,
        Duration::from_secs(10),
        status_refresh_interval,
    )
}

fn finish_json_result(result: Result<()>, output_started: bool) -> Result<()> {
    result.map_err(|error| {
        if output_started {
            crate::cli::json_output_already_emitted(error)
        } else {
            crate::cli::json_command_error("ui", error)
        }
    })
}

fn json_document(
    ctx: RepoContext,
    plan_id: Option<String>,
    timeline_limit: TimelineLimit,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>> {
    let source = RepoDashboardSource::new(ctx);
    if let Some(plan_id) = plan_id {
        return match source.plan(PlanBasis::Fresh, plan_id.clone(), cancelled)? {
            PlanSnapshotResult::Found(snapshot) => serialize_json(&snapshot),
            PlanSnapshotResult::NotFound => bail!("plan `{plan_id}` was not found"),
            PlanSnapshotResult::StaleRecorderEpoch => {
                bail!("fresh plan collection returned an invalid stale-epoch result")
            }
        };
    }
    let refresh = source.recorder(
        RecorderRequest {
            mode: RecorderMode::Refresh,
            timeline_limit,
        },
        cancelled,
    )?;
    serialize_json(&refresh.recorder)
}

fn serialize_json(value: &impl serde::Serialize) -> Result<Vec<u8>> {
    let mut document = serde_json::to_vec(value)?;
    document.push(b'\n');
    Ok(document)
}

fn write_json(document: &[u8]) -> Result<()> {
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    write_json_to(&mut output, document)
}

fn write_json_to(output: &mut impl Write, document: &[u8]) -> Result<()> {
    output.write_all(document)?;
    output.flush()?;
    Ok(())
}

fn supervised<T>(operation: impl FnOnce(&dyn Fn() -> bool) -> Result<T>) -> Result<T> {
    #[cfg(all(unix, not(test)))]
    {
        let signal_session = crate::doctor::DoctorSignalSession::start().map_err(|_| {
            anyhow::anyhow!("Dashboard was not started because signal supervision is unavailable")
        })?;
        let cancellation = signal_session.cancellation();
        let outcome = operation(&|| cancellation.cancelled());
        crate::codex::finish_signal_supervised(
            outcome,
            signal_session.finish(),
            "Dashboard signal supervision could not retire safely",
        )
    }
    #[cfg(any(not(unix), test))]
    {
        operation(&|| false)
    }
}

#[allow(dead_code)]
fn serve_legacy(ctx: &RepoContext, port: u16, json_output: bool) -> Result<()> {
    let server = UiServer::bind(port)?;
    let url = server.bootstrap_url();
    let origin = server.origin().to_string();
    let snapshot_path = server.snapshot_path();
    if json_output {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "command": "ui",
                "url": url,
                "origin": origin,
                "snapshot_path": snapshot_path,
            })
        );
    } else {
        println!("Jig UI serving at {url} (Ctrl-C to stop)");
        println!("Open this one-time URL to establish a browser session.");
        println!("Snapshot API after sign-in: {origin}{snapshot_path}");
    }
    std::io::stdout().flush()?;
    let result = server.serve(ctx);
    if json_output {
        return result.map_err(crate::cli::json_output_already_emitted);
    }
    result
}

#[cfg(test)]
mod tests;
