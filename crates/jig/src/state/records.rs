//! Durable state-stream record schemas and their compatibility serde contracts.
//!
//! Keep filesystem, locking, and JSONL traversal behavior out of this module.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::git_receipts::DiffStat;

#[derive(Clone, Debug)]
pub(super) enum SessionEvent {
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

#[derive(Clone, Debug)]
pub(super) enum PlanEvent {
    Open {
        id: String,
        plan_id: String,
        timestamp_ms: u64,
        title: String,
        body_path: Option<String>,
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
pub(super) struct SessionEventEnvelope {
    pub(super) id: String,
    pub(super) session_id: String,
    pub(super) event: String,
    pub(super) timestamp_ms: u64,
    pub(super) outcome: Option<String>,
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
            } => LegacyPlanEvent {
                id: id.clone(),
                plan_id: plan_id.clone(),
                event: "open".into(),
                timestamp_ms: *timestamp_ms,
                title: Some(title.clone()),
                body_path: body_path.clone(),
                resolution: None,
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
            "open" => Self::open(
                legacy.id,
                legacy.plan_id,
                legacy.timestamp_ms,
                legacy.title.unwrap_or_else(|| "Untitled plan".into()),
                legacy.body_path,
            ),
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
pub(super) struct ReceiptRecord {
    pub(super) id: String,
    pub(super) session_id: Option<String>,
    pub(super) plan_id: Option<String>,
    pub(super) tool_name: String,
    pub(super) args: Value,
    #[serde(default)]
    pub(super) invoked_command_key: Option<String>,
    pub(super) started_at_ms: u64,
    pub(super) ended_at_ms: u64,
    pub(super) exit_status: i32,
    pub(super) stdout_preview: String,
    pub(super) stderr_preview: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) evidence: Option<Value>,
    pub(super) changed_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) changed_path_count: Option<usize>,
    #[serde(default)]
    pub(super) changed_paths_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) changed_paths_digest: Option<String>,
    pub(super) diff_stat: DiffStat,
    #[serde(default)]
    pub(super) git_status_error: Option<String>,
    #[serde(default)]
    pub(super) git_diff_stat_error: Option<String>,
    #[serde(default)]
    pub(super) worktree_fingerprint: Option<String>,
    #[serde(default)]
    pub(super) worktree_fingerprint_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub(super) struct DecisionRecord {
    pub(super) id: String,
    pub(super) session_id: Option<String>,
    pub(super) plan_id: Option<String>,
    pub(super) title: String,
    pub(super) selected_option: String,
    pub(super) rationale: String,
    pub(super) alternatives: Vec<String>,
    pub(super) timestamp_ms: u64,
}
