//! Durable state-stream record schemas and their compatibility serde contracts.
//!
//! Keep filesystem, locking, and JSONL traversal behavior out of this module.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::git_receipts::DiffStat;
use jig_contract::{Finding, RunConclusion, RunPlan, TargetId, TargetRunResult};

#[derive(Clone, Debug)]
pub(crate) enum SessionEvent {
    Start {
        id: String,
        session_id: String,
        timestamp_ms: u64,
        summary: Value,
    },
    End {
        id: String,
        session_id: String,
        timestamp_ms: u64,
        outcome: Option<String>,
    },
    Unknown {
        id: String,
        session_id: String,
        event: String,
        timestamp_ms: u64,
    },
}

impl SessionEvent {
    pub(super) const fn start(
        id: String,
        session_id: String,
        timestamp_ms: u64,
        summary: Value,
    ) -> Self {
        Self::Start {
            id,
            session_id,
            timestamp_ms,
            summary,
        }
    }

    pub(super) const fn end(
        id: String,
        session_id: String,
        timestamp_ms: u64,
        outcome: Option<String>,
    ) -> Self {
        Self::End {
            id,
            session_id,
            timestamp_ms,
            outcome,
        }
    }

    pub(super) const fn is_start(&self) -> bool {
        matches!(self, Self::Start { .. })
    }

    pub(super) fn session_id(&self) -> &str {
        match self {
            Self::Start { session_id, .. }
            | Self::End { session_id, .. }
            | Self::Unknown { session_id, .. } => session_id,
        }
    }

    pub(super) const fn timestamp_ms(&self) -> u64 {
        match self {
            Self::Start { timestamp_ms, .. }
            | Self::End { timestamp_ms, .. }
            | Self::Unknown { timestamp_ms, .. } => *timestamp_ms,
        }
    }

    pub(super) fn into_summary_reference(self) -> Self {
        match self {
            Self::Start {
                id,
                session_id,
                timestamp_ms,
                ..
            } => Self::Start {
                id,
                session_id,
                timestamp_ms,
                summary: Value::Null,
            },
            event => event,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct PlanBaseline {
    pub(crate) requested_ref: String,
    pub(crate) commit_oid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) empty_tree_oid: Option<String>,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum PlanEvent {
    Open {
        id: String,
        plan_id: String,
        timestamp_ms: u64,
        title: String,
        body_path: Option<String>,
        baseline: Option<PlanBaseline>,
    },
    Append {
        id: String,
        plan_id: String,
        timestamp_ms: u64,
        body_path: Option<String>,
    },
    Close {
        id: String,
        plan_id: String,
        timestamp_ms: u64,
        resolution: Option<String>,
    },
    Unknown {
        id: String,
        plan_id: String,
        event: String,
        timestamp_ms: u64,
    },
}

impl PlanEvent {
    pub(super) const fn open(
        id: String,
        plan_id: String,
        timestamp_ms: u64,
        title: String,
        body_path: Option<String>,
    ) -> Self {
        Self::Open {
            id,
            plan_id,
            timestamp_ms,
            title,
            body_path,
            baseline: None,
        }
    }

    pub(super) const fn open_with_baseline(
        id: String,
        plan_id: String,
        timestamp_ms: u64,
        title: String,
        body_path: Option<String>,
        baseline: PlanBaseline,
    ) -> Self {
        Self::Open {
            id,
            plan_id,
            timestamp_ms,
            title,
            body_path,
            baseline: Some(baseline),
        }
    }

    pub(super) const fn append(
        id: String,
        plan_id: String,
        timestamp_ms: u64,
        body_path: Option<String>,
    ) -> Self {
        Self::Append {
            id,
            plan_id,
            timestamp_ms,
            body_path,
        }
    }

    pub(super) const fn close(
        id: String,
        plan_id: String,
        timestamp_ms: u64,
        resolution: Option<String>,
    ) -> Self {
        Self::Close {
            id,
            plan_id,
            timestamp_ms,
            resolution,
        }
    }

    pub(super) fn plan_id(&self) -> &str {
        match self {
            Self::Open { plan_id, .. }
            | Self::Append { plan_id, .. }
            | Self::Close { plan_id, .. }
            | Self::Unknown { plan_id, .. } => plan_id,
        }
    }

    pub(super) const fn timestamp_ms(&self) -> u64 {
        match self {
            Self::Open { timestamp_ms, .. }
            | Self::Append { timestamp_ms, .. }
            | Self::Close { timestamp_ms, .. }
            | Self::Unknown { timestamp_ms, .. } => *timestamp_ms,
        }
    }

    pub(super) fn body_path(&self) -> Option<&str> {
        match self {
            Self::Open { body_path, .. } | Self::Append { body_path, .. } => body_path.as_deref(),
            Self::Close { .. } | Self::Unknown { .. } => None,
        }
    }

    pub(super) const fn is_open(&self) -> bool {
        matches!(self, Self::Open { .. })
    }

    pub(super) fn baseline(&self) -> Option<&PlanBaseline> {
        match self {
            Self::Open { baseline, .. } => baseline.as_ref(),
            _ => None,
        }
    }
}

#[derive(Serialize)]
struct LegacySessionEvent {
    id: String,
    session_id: String,
    event: String,
    timestamp_ms: u64,
    outcome: Option<String>,
    summary: Option<Value>,
}

// Start summaries are durable write-time snapshots, but runtime state readers
// need only the event envelope. Keeping this read model metadata-only makes
// serde_json validate and skip a legacy `summary` through its iterative
// ignored-value path instead of materializing recursively embedded snapshots.
// Never use this lossy type to rewrite the append-only session stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct SessionEventEnvelope {
    pub(crate) id: String,
    pub(crate) session_id: String,
    pub(crate) event: String,
    pub(crate) timestamp_ms: u64,
    pub(crate) outcome: Option<String>,
}

impl SessionEventEnvelope {
    pub(super) fn into_event(self) -> SessionEvent {
        match self.event.as_str() {
            "start" => {
                SessionEvent::start(self.id, self.session_id, self.timestamp_ms, Value::Null)
            }
            "end" => SessionEvent::end(self.id, self.session_id, self.timestamp_ms, self.outcome),
            _ => SessionEvent::Unknown {
                id: self.id,
                session_id: self.session_id,
                event: self.event,
                timestamp_ms: self.timestamp_ms,
            },
        }
    }
}

impl Serialize for SessionEvent {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let legacy = match self {
            Self::Start {
                id,
                session_id,
                timestamp_ms,
                summary,
            } => LegacySessionEvent {
                id: id.clone(),
                session_id: session_id.clone(),
                event: "start".into(),
                timestamp_ms: *timestamp_ms,
                outcome: None,
                summary: Some(summary.clone()),
            },
            Self::End {
                id,
                session_id,
                timestamp_ms,
                outcome,
            } => LegacySessionEvent {
                id: id.clone(),
                session_id: session_id.clone(),
                event: "end".into(),
                timestamp_ms: *timestamp_ms,
                outcome: outcome.clone(),
                summary: None,
            },
            Self::Unknown {
                id,
                session_id,
                event,
                timestamp_ms,
            } => LegacySessionEvent {
                id: id.clone(),
                session_id: session_id.clone(),
                event: event.clone(),
                timestamp_ms: *timestamp_ms,
                outcome: None,
                summary: None,
            },
        };
        legacy.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SessionEvent {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(SessionEventEnvelope::deserialize(deserializer)?.into_event())
    }
}

#[derive(Serialize, Deserialize)]
struct LegacyPlanEvent {
    id: String,
    plan_id: String,
    event: String,
    timestamp_ms: u64,
    title: Option<String>,
    body_path: Option<String>,
    resolution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    baseline: Option<PlanBaseline>,
}

impl Serialize for PlanEvent {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let legacy = match self {
            Self::Open {
                id,
                plan_id,
                timestamp_ms,
                title,
                body_path,
                baseline,
            } => LegacyPlanEvent {
                id: id.clone(),
                plan_id: plan_id.clone(),
                event: "open".into(),
                timestamp_ms: *timestamp_ms,
                title: Some(title.clone()),
                body_path: body_path.clone(),
                resolution: None,
                baseline: baseline.clone(),
            },
            Self::Append {
                id,
                plan_id,
                timestamp_ms,
                body_path,
            } => LegacyPlanEvent {
                id: id.clone(),
                plan_id: plan_id.clone(),
                event: "append".into(),
                timestamp_ms: *timestamp_ms,
                title: None,
                body_path: body_path.clone(),
                resolution: None,
                baseline: None,
            },
            Self::Close {
                id,
                plan_id,
                timestamp_ms,
                resolution,
            } => LegacyPlanEvent {
                id: id.clone(),
                plan_id: plan_id.clone(),
                event: "close".into(),
                timestamp_ms: *timestamp_ms,
                title: None,
                body_path: None,
                resolution: resolution.clone(),
                baseline: None,
            },
            Self::Unknown {
                id,
                plan_id,
                event,
                timestamp_ms,
            } => LegacyPlanEvent {
                id: id.clone(),
                plan_id: plan_id.clone(),
                event: event.clone(),
                timestamp_ms: *timestamp_ms,
                title: None,
                body_path: None,
                resolution: None,
                baseline: None,
            },
        };
        legacy.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PlanEvent {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let legacy = LegacyPlanEvent::deserialize(deserializer)?;
        Ok(match legacy.event.as_str() {
            "open" => match legacy.baseline {
                Some(baseline) => Self::open_with_baseline(
                    legacy.id,
                    legacy.plan_id,
                    legacy.timestamp_ms,
                    legacy.title.unwrap_or_else(|| "Untitled plan".into()),
                    legacy.body_path,
                    baseline,
                ),
                None => Self::open(
                    legacy.id,
                    legacy.plan_id,
                    legacy.timestamp_ms,
                    legacy.title.unwrap_or_else(|| "Untitled plan".into()),
                    legacy.body_path,
                ),
            },
            "append" => Self::append(
                legacy.id,
                legacy.plan_id,
                legacy.timestamp_ms,
                legacy.body_path,
            ),
            "close" => Self::close(
                legacy.id,
                legacy.plan_id,
                legacy.timestamp_ms,
                legacy.resolution,
            ),
            _ => Self::Unknown {
                id: legacy.id,
                plan_id: legacy.plan_id,
                event: legacy.event,
                timestamp_ms: legacy.timestamp_ms,
            },
        })
    }
}

#[derive(Debug, Serialize, serde::Deserialize)]
pub(crate) struct ReceiptRecord {
    pub(crate) id: String,
    pub(crate) session_id: Option<String>,
    pub(crate) plan_id: Option<String>,
    pub(crate) tool_name: String,
    pub(crate) args: Value,
    #[serde(default)]
    pub(crate) invoked_command_key: Option<String>,
    pub(crate) started_at_ms: u64,
    pub(crate) ended_at_ms: u64,
    pub(crate) exit_status: i32,
    pub(crate) stdout_preview: String,
    pub(crate) stderr_preview: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) evidence: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) target: Option<TargetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) config_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) input_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) findings: Vec<Finding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) finding_count: Option<u64>,
    #[serde(default)]
    pub(crate) findings_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) findings_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) evaluated_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) valid_until_ms: Option<u64>,
    pub(crate) changed_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) changed_path_count: Option<usize>,
    #[serde(default)]
    pub(crate) changed_paths_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) changed_paths_digest: Option<String>,
    pub(crate) diff_stat: DiffStat,
    #[serde(default)]
    pub(crate) git_status_error: Option<String>,
    #[serde(default)]
    pub(crate) git_diff_stat_error: Option<String>,
    #[serde(default)]
    pub(crate) worktree_fingerprint: Option<String>,
    #[serde(default)]
    pub(crate) worktree_fingerprint_error: Option<String>,
}

/// One append-only transition in a durable target run.
///
/// This intentionally uses a string event name and optional payloads rather
/// than a tagged enum so readers can skip events written by newer runtimes.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct RunEventRecord {
    pub(super) id: String,
    pub(super) run_id: String,
    pub(super) event: String,
    pub(super) timestamp_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) work_plan_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) plan: Option<RunPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) target: Option<TargetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) result: Option<TargetRunResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) conclusion: Option<RunConclusion>,
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub(crate) struct DecisionRecord {
    pub(crate) id: String,
    pub(crate) session_id: Option<String>,
    pub(crate) plan_id: Option<String>,
    pub(crate) title: String,
    pub(crate) selected_option: String,
    pub(crate) rationale: String,
    pub(crate) alternatives: Vec<String>,
    pub(crate) timestamp_ms: u64,
}
