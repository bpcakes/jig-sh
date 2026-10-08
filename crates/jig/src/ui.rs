//! `jig ui`: the unified read-only terminal dashboard and one-shot recorder.

use std::cell::Cell;
use std::io::Write;
use std::time::Duration;

use anyhow::{Context, Result};
use jig_dashboard::{DashboardSource, RecorderMode, RecorderRequest, TimelineLimit};
use jig_ui::terminal::{DashboardOptions, InitialTab};

use jig_context::RepoContext;

mod source;

pub(crate) use source::RepoDashboardSource;

/// The interactive dashboard's inputs. The caller owns flag parsing and
/// defaults.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DashboardRequest {
    pub(crate) timeline_limit: u64,
    pub(crate) refresh_interval: Duration,
}

/// A failed one-shot recorder document. Once output has started, the caller
/// must not follow the partial document with an error document.
#[derive(Debug)]
pub(crate) struct RecorderJsonError {
    pub(crate) error: anyhow::Error,
    pub(crate) output_started: bool,
}

pub(crate) fn run(ctx: RepoContext, request: DashboardRequest) -> Result<()> {
    let options = timeline_dashboard_options(request)?;
    supervised(|cancelled| {
        jig_ui::terminal::run_with_cancellation(RepoDashboardSource::new(ctx), options, cancelled)
    })
}

/// Writes one local recorder snapshot to stdout as a JSON document.
pub(crate) fn write_recorder_json(
    ctx: RepoContext,
    timeline_limit: u64,
) -> std::result::Result<(), RecorderJsonError> {
    let output_started = Cell::new(false);
    let result = runtime_timeline_limit(timeline_limit).and_then(|timeline_limit| {
        supervised(|cancelled| {
            let document = json_document(ctx, timeline_limit, cancelled)?;
            output_started.set(true);
            write_json(&document)
        })
    });
    recorder_json_result(result, output_started.get())
}

fn recorder_json_result(
    result: Result<()>,
    output_started: bool,
) -> std::result::Result<(), RecorderJsonError> {
    result.map_err(|error| RecorderJsonError {
        error,
        output_started,
    })
}

fn runtime_timeline_limit(rows: u64) -> Result<TimelineLimit> {
    usize::try_from(rows)
        .ok()
        .and_then(|rows| TimelineLimit::new(rows).ok())
        .context("the validated timeline limit was outside the runtime range")
}

fn timeline_dashboard_options(request: DashboardRequest) -> Result<DashboardOptions> {
    let timeline_limit = runtime_timeline_limit(request.timeline_limit)?;
    DashboardOptions::new(InitialTab::Timeline, request.refresh_interval)
        .with_timeline_limit(timeline_limit.get())
        .context("the validated timeline limit was outside the terminal range")
}

pub(crate) fn run_status(ctx: RepoContext, refresh_interval: Duration) -> Result<()> {
    let options = status_dashboard_options(refresh_interval);
    supervised(|cancelled| {
        jig_ui::terminal::run_with_cancellation(RepoDashboardSource::new(ctx), options, cancelled)
    })
}

fn status_dashboard_options(refresh_interval: Duration) -> DashboardOptions {
    DashboardOptions::new(InitialTab::Status, refresh_interval)
}

fn json_document(
    ctx: RepoContext,
    timeline_limit: TimelineLimit,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>> {
    let source = RepoDashboardSource::new(ctx);
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
        let signal_session = crate::signal_supervision::SignalSession::start().map_err(|_| {
            anyhow::anyhow!("Dashboard was not started because signal supervision is unavailable")
        })?;
        let cancellation = signal_session.cancellation();
        let outcome = operation(&|| cancellation.cancelled());
        crate::signal_supervision::finish(
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

#[cfg(test)]
mod tests;
