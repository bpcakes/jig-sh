use std::collections::BTreeMap;

use anyhow::Result;
use jig_ui::{
    CountsView, DashboardSnapshot, DecisionTimelineView, DecisionView, FailureView, GatesView,
    HarnessView, LoopsView, OpenPlanView, PlanSnapshot, PlanSummary, PlanTimelineView,
    ReceiptTimelineView, ReceiptView, RepoView, SessionTimelineView, TimelineItem, TimelineShow,
    ToolStatView, UiQuery,
};
#[cfg(test)]
use serde_json::Value;

use crate::context::RepoContext;
use crate::runtime::{loop_status_snapshot, work_gates_snapshot};
use crate::state::{
    DecisionStreamRecord, PlanFileError, PlanFileErrorKind, PlanStreamEvent, ReceiptStreamRecord,
    StateStreams, current_session, now_ms, plan_detail_streams, plan_receipts, read_plan_body,
    state_streams,
};
use crate::text::truncate_chars;

const TIMELINE_RECEIPT_SCAN_LIMIT: usize = 400;
const HISTORY_LIMIT: usize = 10;
const FAILURES_LIMIT: usize = 10;
const PLAN_RECEIPTS_LIMIT: usize = 50;
const FAILURE_PREVIEW_CHARS: usize = 400;
const DETAIL_PREVIEW_CHARS: usize = 1_000;
const RATIONALE_PREVIEW_CHARS: usize = 300;
const PLAN_CHANGED_PATHS_LIMIT: usize = 20;

#[cfg(test)]
pub(super) fn snapshot(ctx: &RepoContext) -> Result<Value> {
    serde_json::to_value(snapshot_with_query(ctx, UiQuery::default())?).map_err(Into::into)
}

pub(super) fn snapshot_with_query(ctx: &RepoContext, query: UiQuery) -> Result<DashboardSnapshot> {
    let streams = state_streams(ctx, TIMELINE_RECEIPT_SCAN_LIMIT)?;
    let plans = plan_index(&streams.plan_events);
    let open_plans = open_plans_with_gates(ctx, &plans);
    let (loops, loops_error) = match loop_status_snapshot(ctx) {
        Ok(value) => match serde_json::from_value::<LoopsView>(value) {
            Ok(loops) => (Some(loops), None),
            Err(error) => (None, Some(format!("Failed to decode loop status: {error}"))),
        },
        Err(error) => (None, Some(format!("{error:#}"))),
    };
    Ok(DashboardSnapshot {
        ok: true,
        command: "ui snapshot".into(),
        generated_at_ms: now_ms(),
        repo: RepoView {
            name: ctx.repo_name().to_string(),
            default_branch: ctx.default_branch().to_string(),
            source_commit: Some(ctx.source_commit().to_string()),
            source_path: Some(ctx.source_path().to_string()),
        },
        harness: HarnessView {
            jig_version: ctx.legacy_jig_version().map(str::to_string),
            runtime_version: env!("CARGO_PKG_VERSION").to_string(),
            contract_version: u64::from(ctx.contract_version()),
        },
        current_session_id: current_session(ctx)?,
        counts: CountsView {
            sessions: streams
                .session_events
                .iter()
                .filter(|event| event.event == "start")
                .count() as u64,
            session_events: streams.session_events.len() as u64,
            plans: streams
                .plan_events
                .iter()
                .filter(|event| event.event == "open")
                .count() as u64,
            plan_events: streams.plan_events.len() as u64,
            open_plans: open_plans.len() as u64,
            decisions: streams.decisions.len() as u64,
        },
        open_plans,
        history: closed_plan_history(&plans),
        failures: recent_failures(&streams.receipts),
        tool_stats: tool_stats(&streams.receipts),
        loops,
        loops_error,
        timeline: build_timeline(&streams, query),
        timeline_show: query.show.as_str().into(),
        timeline_limit: query.limit,
    })
}

pub(super) fn plan_snapshot(ctx: &RepoContext, plan_id: &str) -> Result<Option<PlanSnapshot>> {
    let streams = plan_detail_streams(ctx)?;
    let plans = plan_index(&streams.plan_events);
    let Some(info) = plans.get(plan_id) else {
        return Ok(None);
    };
    let (gates, gates_error) = gates_for_plan(ctx, Some(plan_id.to_string()));
    let decisions = streams
        .decisions
        .iter()
        .rev()
        .filter(|decision| decision.plan_id.as_deref() == Some(plan_id))
        .map(decision_view)
        .collect();
    let receipts = plan_receipts(ctx, plan_id, PLAN_RECEIPTS_LIMIT)?
        .iter()
        .map(receipt_view)
        .collect();
    let (body, body_error) = plan_body_text(ctx, plan_id);
    Ok(Some(PlanSnapshot {
        ok: true,
        command: "ui plan".into(),
        generated_at_ms: now_ms(),
        plan: info.summary(plan_id),
        body,
        body_error,
        gates,
        gates_error,
        decisions,
        receipts,
        receipts_limit: PLAN_RECEIPTS_LIMIT,
    }))
}

#[derive(Default)]
struct PlanInfo {
    title: String,
    body_path: Option<String>,
    opened_at_ms: Option<u64>,
    closed_at_ms: Option<u64>,
    resolution: Option<String>,
    baseline_ref: Option<String>,
    baseline_oid: Option<String>,
    baseline_error: Option<String>,
    opened: bool,
    closed: bool,
}
impl PlanInfo {
    fn summary(&self, plan_id: &str) -> PlanSummary {
        PlanSummary {
            plan_id: plan_id.into(),
            title: self.title.clone(),
            state: if self.closed { "closed" } else { "open" }.into(),
            opened_at_ms: self.opened_at_ms,
            closed_at_ms: self.closed_at_ms,
            resolution: self.resolution.clone(),
            duration_ms: self
                .opened_at_ms
                .zip(self.closed_at_ms)
                .map(|(a, b)| b.saturating_sub(a)),
            baseline_ref: self.baseline_ref.clone(),
            baseline_oid: self.baseline_oid.clone(),
            baseline_error: self.baseline_error.clone(),
        }
    }
}

fn plan_index(events: &[PlanStreamEvent]) -> BTreeMap<String, PlanInfo> {
    let mut plans = BTreeMap::new();
    for event in events {
        let info = plans
            .entry(event.plan_id.clone())
            .or_insert_with(PlanInfo::default);
        match event.event.as_str() {
            "open" => {
                info.title = event
                    .title
                    .clone()
                    .unwrap_or_else(|| "Untitled plan".into());
                info.body_path = event.body_path.clone();
                info.baseline_ref = event
                    .baseline
                    .as_ref()
                    .map(|baseline| baseline.requested_ref.clone());
                info.baseline_oid = event.baseline.as_ref().and_then(|baseline| {
                    baseline
                        .commit_oid
                        .clone()
                        .or_else(|| baseline.empty_tree_oid.clone())
                });
                info.baseline_error = event
                    .baseline
                    .as_ref()
                    .and_then(|baseline| baseline.error.clone());
                info.opened_at_ms = Some(event.timestamp_ms);
                info.opened = true;
                info.closed = false;
                info.closed_at_ms = None;
                info.resolution = None;
            }
            "close" => {
                info.closed = true;
                info.closed_at_ms = Some(event.timestamp_ms);
                info.resolution = event.resolution.clone();
            }
            _ => {}
        }
    }
    plans
}

fn closed_plan_history(plans: &BTreeMap<String, PlanInfo>) -> Vec<PlanSummary> {
    let mut rows = plans
        .iter()
        .filter(|(_, p)| p.opened && p.closed)
        .map(|(id, p)| p.summary(id))
        .collect::<Vec<_>>();
    rows.sort_by_key(|p| std::cmp::Reverse(p.closed_at_ms.unwrap_or(0)));
    rows.truncate(HISTORY_LIMIT);
    rows
}

fn gates_for_plan(
    ctx: &RepoContext,
    plan_id: Option<String>,
) -> (Option<GatesView>, Option<String>) {
    match work_gates_snapshot(ctx, plan_id) {
        Ok(value) => match serde_json::from_value(value) {
            Ok(gates) => (Some(gates), None),
            Err(error) => (None, Some(format!("Failed to decode gate status: {error}"))),
        },
        Err(error) => (None, Some(format!("{error:#}"))),
    }
}

fn open_plans_with_gates(
    ctx: &RepoContext,
    plans: &BTreeMap<String, PlanInfo>,
) -> Vec<OpenPlanView> {
    plans
        .iter()
        .filter(|(_, p)| p.opened && !p.closed)
        .map(|(id, p)| {
            let (gates, gates_error) = gates_for_plan(ctx, Some(id.clone()));
            OpenPlanView {
                plan_id: id.clone(),
                title: p.title.clone(),
                body_path: p.body_path.clone(),
                opened_at_ms: p.opened_at_ms,
                baseline_ref: p.baseline_ref.clone(),
                baseline_oid: p.baseline_oid.clone(),
                baseline_error: p.baseline_error.clone(),
                gates,
                gates_error,
            }
        })
        .collect()
}

fn recent_failures(receipts: &[ReceiptStreamRecord]) -> Vec<FailureView> {
    receipts
        .iter()
        .filter(|r| r.exit_status != 0)
        .take(FAILURES_LIMIT)
        .map(|r| FailureView {
            id: r.id.clone(),
            tool_name: r.tool_name.clone(),
            plan_id: r.plan_id.clone(),
            ended_at_ms: Some(r.ended_at_ms),
            exit_status: r.exit_status,
            stderr_preview: truncate_chars(&r.stderr_preview, FAILURE_PREVIEW_CHARS),
        })
        .collect()
}

fn tool_stats(receipts: &[ReceiptStreamRecord]) -> Vec<ToolStatView> {
    struct Stats {
        runs: u64,
        failures: u64,
        total: u64,
        last: i64,
        ended: u64,
    }
    let mut map = BTreeMap::<String, Stats>::new();
    for r in receipts {
        if r.invoked_command_key.is_none() {
            continue;
        }
        let duration = r.ended_at_ms.saturating_sub(r.started_at_ms);
        let s = map.entry(r.tool_name.clone()).or_insert(Stats {
            runs: 0,
            failures: 0,
            total: 0,
            last: r.exit_status,
            ended: r.ended_at_ms,
        });
        s.runs += 1;
        s.failures += u64::from(r.exit_status != 0);
        s.total += duration;
    }
    let mut rows = map
        .into_iter()
        .map(|(tool, s)| ToolStatView {
            tool,
            runs: s.runs,
            failures: s.failures,
            last_exit_status: s.last,
            last_ended_at_ms: s.ended,
            avg_duration_ms: s.total / s.runs.max(1),
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|r| std::cmp::Reverse(r.last_ended_at_ms));
    rows
}

fn plan_body_text(ctx: &RepoContext, plan_id: &str) -> (Option<String>, Option<String>) {
    match read_plan_body(ctx, plan_id, &|| false) {
        Ok(body) => {
            let mut text = body.text;
            if body.truncated {
                text.push('…');
            }
            (Some(text), None)
        }
        Err(error)
            if error
                .downcast_ref::<PlanFileError>()
                .is_some_and(|error| error.kind() == PlanFileErrorKind::NotFound) =>
        {
            (None, None)
        }
        Err(error) => (None, Some(format!("Failed to read plan body: {error}"))),
    }
}

fn decision_view(d: &DecisionStreamRecord) -> DecisionView {
    DecisionView {
        id: d.id.clone(),
        session_id: d.session_id.clone(),
        plan_id: d.plan_id.clone(),
        timestamp_ms: d.timestamp_ms,
        title: d.title.clone(),
        selected_option: d.selected_option.clone(),
        alternatives: d.alternatives.clone(),
        rationale: d.rationale.clone(),
    }
}
fn receipt_view(r: &ReceiptStreamRecord) -> ReceiptView {
    ReceiptView {
        timestamp_ms: Some(r.ended_at_ms),
        id: r.id.clone(),
        tool_name: r.tool_name.clone(),
        invoked_command_key: r.invoked_command_key.clone(),
        plan_id: r.plan_id.clone(),
        session_id: r.session_id.clone(),
        exit_status: r.exit_status,
        started_at_ms: Some(r.started_at_ms),
        ended_at_ms: Some(r.ended_at_ms),
        duration_ms: Some(r.ended_at_ms.saturating_sub(r.started_at_ms)),
        diff_summary: r.diff_summary.clone(),
        changed_paths: r
            .changed_paths
            .iter()
            .take(PLAN_CHANGED_PATHS_LIMIT)
            .cloned()
            .collect(),
        stdout_preview: truncate_chars(&r.stdout_preview, DETAIL_PREVIEW_CHARS),
        stderr_preview: truncate_chars(&r.stderr_preview, DETAIL_PREVIEW_CHARS),
    }
}

fn build_timeline(streams: &StateStreams, query: UiQuery) -> Vec<TimelineItem> {
    let mut entries = Vec::new();
    if matches!(query.show, TimelineShow::All | TimelineShow::Sessions) {
        entries.extend(streams.session_events.iter().map(|e| {
            TimelineItem::Session(SessionTimelineView {
                timestamp_ms: Some(e.timestamp_ms),
                event: e.event.clone(),
                session_id: e.session_id.clone(),
                outcome: e.outcome.clone(),
            })
        }));
    }
    if matches!(query.show, TimelineShow::All | TimelineShow::Plans) {
        entries.extend(streams.plan_events.iter().map(|e| {
            TimelineItem::Plan(PlanTimelineView {
                timestamp_ms: Some(e.timestamp_ms),
                event: e.event.clone(),
                plan_id: e.plan_id.clone(),
                title: e.title.clone(),
                resolution: e.resolution.clone(),
            })
        }));
    }
    if matches!(
        query.show,
        TimelineShow::All | TimelineShow::Receipts | TimelineShow::Failures
    ) {
        entries.extend(
            streams
                .receipts
                .iter()
                .filter(|r| query.show != TimelineShow::Failures || r.exit_status != 0)
                .map(|r| {
                    TimelineItem::Receipt(ReceiptTimelineView {
                        timestamp_ms: Some(r.ended_at_ms),
                        id: r.id.clone(),
                        tool_name: r.tool_name.clone(),
                        invoked_command_key: r.invoked_command_key.clone(),
                        plan_id: r.plan_id.clone(),
                        session_id: r.session_id.clone(),
                        exit_status: r.exit_status,
                        started_at_ms: Some(r.started_at_ms),
                        ended_at_ms: Some(r.ended_at_ms),
                        duration_ms: Some(r.ended_at_ms.saturating_sub(r.started_at_ms)),
                        diff_summary: r.diff_summary.clone(),
                        changed_path_count: Some(r.changed_paths.len() as u64),
                        stderr_preview: (r.exit_status != 0)
                            .then(|| truncate_chars(&r.stderr_preview, FAILURE_PREVIEW_CHARS)),
                    })
                }),
        );
    }
    if matches!(query.show, TimelineShow::All | TimelineShow::Decisions) {
        entries.extend(streams.decisions.iter().map(|d| {
            TimelineItem::Decision(DecisionTimelineView {
                timestamp_ms: Some(d.timestamp_ms),
                id: d.id.clone(),
                plan_id: d.plan_id.clone(),
                title: d.title.clone(),
                selected_option: d.selected_option.clone(),
                rationale: truncate_chars(&d.rationale, RATIONALE_PREVIEW_CHARS),
            })
        }));
    }
    entries.sort_by_key(|e| std::cmp::Reverse(e.timestamp_ms().unwrap_or(0)));
    entries.truncate(query.limit);
    entries
}
