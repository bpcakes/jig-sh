use anyhow::Result;

use crate::context::RepoContext;

use super::jsonl::{read_jsonl, read_receipt_window, read_receipts_reverse};
use super::receipts::receipt_diff_summary;
use super::records::{DecisionRecord, PlanBaseline, PlanEvent, ReceiptRecord, SessionEvent};
use super::sessions::read_session_events;

pub(crate) struct SessionStreamEvent {
    pub event: String,
    pub timestamp_ms: u64,
    pub session_id: String,
    pub outcome: Option<String>,
}
pub(crate) struct PlanStreamEvent {
    pub event: String,
    pub timestamp_ms: u64,
    pub plan_id: String,
    pub title: Option<String>,
    pub body_path: Option<String>,
    pub baseline: Option<PlanBaseline>,
    pub resolution: Option<String>,
}
pub(crate) struct DecisionStreamRecord {
    pub id: String,
    pub session_id: Option<String>,
    pub plan_id: Option<String>,
    pub title: String,
    pub selected_option: String,
    pub alternatives: Vec<String>,
    pub rationale: String,
    pub timestamp_ms: u64,
}
pub(crate) struct ReceiptStreamRecord {
    pub id: String,
    pub session_id: Option<String>,
    pub plan_id: Option<String>,
    pub tool_name: String,
    pub invoked_command_key: Option<String>,
    pub exit_status: i64,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub stdout_preview: String,
    pub stderr_preview: String,
    pub changed_paths: Vec<String>,
    pub diff_summary: Option<String>,
}

impl From<SessionEvent> for SessionStreamEvent {
    fn from(event: SessionEvent) -> Self {
        match event {
            SessionEvent::Start {
                session_id,
                timestamp_ms,
                ..
            } => Self {
                event: "start".into(),
                timestamp_ms,
                session_id,
                outcome: None,
            },
            SessionEvent::End {
                session_id,
                timestamp_ms,
                outcome,
                ..
            } => Self {
                event: "end".into(),
                timestamp_ms,
                session_id,
                outcome,
            },
            SessionEvent::Unknown {
                session_id,
                event,
                timestamp_ms,
                ..
            } => Self {
                event,
                timestamp_ms,
                session_id,
                outcome: None,
            },
        }
    }
}
impl From<PlanEvent> for PlanStreamEvent {
    fn from(event: PlanEvent) -> Self {
        match event {
            PlanEvent::Open {
                plan_id,
                timestamp_ms,
                title,
                body_path,
                baseline,
                ..
            } => Self {
                event: "open".into(),
                timestamp_ms,
                plan_id,
                title: Some(title),
                body_path,
                baseline,
                resolution: None,
            },
            PlanEvent::Append {
                plan_id,
                timestamp_ms,
                body_path,
                ..
            } => Self {
                event: "append".into(),
                timestamp_ms,
                plan_id,
                title: None,
                body_path,
                baseline: None,
                resolution: None,
            },
            PlanEvent::Close {
                plan_id,
                timestamp_ms,
                resolution,
                ..
            } => Self {
                event: "close".into(),
                timestamp_ms,
                plan_id,
                title: None,
                body_path: None,
                baseline: None,
                resolution,
            },
            PlanEvent::Unknown {
                plan_id,
                event,
                timestamp_ms,
                ..
            } => Self {
                event,
                timestamp_ms,
                plan_id,
                title: None,
                body_path: None,
                baseline: None,
                resolution: None,
            },
        }
    }
}
impl From<DecisionRecord> for DecisionStreamRecord {
    fn from(v: DecisionRecord) -> Self {
        Self {
            id: v.id,
            session_id: v.session_id,
            plan_id: v.plan_id,
            title: v.title,
            selected_option: v.selected_option,
            alternatives: v.alternatives,
            rationale: v.rationale,
            timestamp_ms: v.timestamp_ms,
        }
    }
}
impl From<ReceiptRecord> for ReceiptStreamRecord {
    fn from(v: ReceiptRecord) -> Self {
        let diff_summary = Some(receipt_diff_summary(&v));
        Self {
            id: v.id,
            session_id: v.session_id,
            plan_id: v.plan_id,
            tool_name: v.tool_name,
            invoked_command_key: v.invoked_command_key,
            exit_status: i64::from(v.exit_status),
            started_at_ms: v.started_at_ms,
            ended_at_ms: v.ended_at_ms,
            stdout_preview: v.stdout_preview,
            stderr_preview: v.stderr_preview,
            changed_paths: v.changed_paths,
            diff_summary,
        }
    }
}

pub(crate) struct StateStreams {
    pub(crate) session_events: Vec<SessionStreamEvent>,
    pub(crate) plan_events: Vec<PlanStreamEvent>,
    pub(crate) receipts: Vec<ReceiptStreamRecord>,
    pub(crate) decisions: Vec<DecisionStreamRecord>,
}

pub(crate) struct PlanDetailStreams {
    pub(crate) plan_events: Vec<PlanStreamEvent>,
    pub(crate) decisions: Vec<DecisionStreamRecord>,
}

pub(crate) fn state_streams(ctx: &RepoContext, receipt_limit: usize) -> Result<StateStreams> {
    Ok(StateStreams {
        session_events: read_session_events(&ctx.state_file("sessions.jsonl"))?
            .into_iter()
            .map(Into::into)
            .collect(),
        plan_events: read_jsonl::<PlanEvent>(&ctx.state_file("plans.jsonl"))?
            .into_iter()
            .map(Into::into)
            .collect(),
        receipts: read_receipt_window(&ctx.state_file("receipts.jsonl"), receipt_limit)?
            .into_iter()
            .map(Into::into)
            .collect(),
        decisions: read_jsonl::<DecisionRecord>(&ctx.state_file("decisions.jsonl"))?
            .into_iter()
            .map(Into::into)
            .collect(),
    })
}

pub(crate) fn plan_receipts(
    ctx: &RepoContext,
    plan_id: &str,
    limit: usize,
) -> Result<Vec<ReceiptStreamRecord>> {
    Ok(
        read_receipts_reverse(&ctx.state_file("receipts.jsonl"), limit, |receipt| {
            receipt.plan_id.as_deref() == Some(plan_id)
        })?
        .0
        .into_iter()
        .map(Into::into)
        .collect(),
    )
}

pub(crate) fn plan_detail_streams(ctx: &RepoContext) -> Result<PlanDetailStreams> {
    Ok(PlanDetailStreams {
        plan_events: read_jsonl::<PlanEvent>(&ctx.state_file("plans.jsonl"))?
            .into_iter()
            .map(Into::into)
            .collect(),
        decisions: read_jsonl::<DecisionRecord>(&ctx.state_file("decisions.jsonl"))?
            .into_iter()
            .map(Into::into)
            .collect(),
    })
}
