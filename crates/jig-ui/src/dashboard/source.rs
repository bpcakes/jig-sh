use std::{error::Error, fmt};

use serde::{Deserialize, Deserializer, Serialize};

use super::{
    DEFAULT_TIMELINE_ROWS, MAX_TIMELINE_ROWS, PlanSnapshot, RecorderSnapshot, StatusLocalSnapshot,
    StatusSnapshot,
};

/// Supplies bounded dashboard observations to the serialized terminal worker.
///
/// Implementations must either poll `cancelled` around every potentially
/// blocking operation or impose an independent finite bound on that operation.
/// Once cancellation is observed, implementations should return promptly. The
/// terminal joins the sole worker before restoring terminal state, so an
/// unbounded implementation would prevent a clean quit.
pub trait DashboardSource: Send + Sync {
    fn recorder(
        &self,
        request: RecorderRequest,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<RecorderRefresh, SourceError>;

    fn status(
        &self,
        request: StatusRequest,
        phase_changed: &dyn Fn(StatusPhase),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<StatusRefresh, SourceError>;

    fn plan(
        &self,
        basis: PlanBasis,
        plan_id: String,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PlanSnapshotResult, SourceError>;
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RecorderEpochId(u64);

impl RecorderEpochId {
    pub const FIRST: Self = Self(1);

    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn checked_next(self) -> Result<Self, SourceError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| SourceError::InternalContract {
                message: "recorder epoch exhausted".to_string(),
            })
    }
}

impl From<RecorderEpochId> for u64 {
    fn from(value: RecorderEpochId) -> Self {
        value.get()
    }
}

impl<'de> Deserialize<'de> for RecorderEpochId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = u64::deserialize(deserializer)?;
        Self::new(value).ok_or_else(|| serde::de::Error::custom("recorder epoch must be non-zero"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TimelineLimit(usize);

impl TimelineLimit {
    pub const DEFAULT: Self = Self(DEFAULT_TIMELINE_ROWS);

    pub fn new(requested: usize) -> Result<Self, SourceError> {
        if (1..=MAX_TIMELINE_ROWS).contains(&requested) {
            Ok(Self(requested))
        } else {
            Err(SourceError::InternalContract {
                message: format!(
                    "timeline limit {requested} is outside 1 through {MAX_TIMELINE_ROWS}"
                ),
            })
        }
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

impl<'de> Deserialize<'de> for TimelineLimit {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(usize::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanBasis {
    RecorderEpoch(RecorderEpochId),
    Fresh,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecorderMode {
    Refresh,
    ReuseCurrent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecorderRequest {
    pub mode: RecorderMode,
    pub timeline_limit: TimelineLimit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatusRequest {
    pub timeline_limit: TimelineLimit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusPhase {
    Providers,
    LocalEpoch,
}

#[derive(Clone, Debug)]
pub struct RecorderRefresh {
    pub recorder: RecorderSnapshot,
    pub status_local: StatusLocalSnapshot,
}

#[derive(Clone, Debug)]
pub struct StatusRefresh {
    pub status: StatusSnapshot,
    pub recorder: RecorderSnapshot,
    /// Observation time for the local recorder-backed partition.
    pub local_observed_at_ms: u64,
    /// Observation time for the external-provider partition.
    pub provider_observed_at_ms: u64,
}

#[derive(Clone, Debug)]
pub enum PlanSnapshotResult {
    Found(Box<PlanSnapshot>),
    NotFound,
    StaleRecorderEpoch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionDomain {
    Repository,
    Sessions,
    Plans,
    Decisions,
    Receipts,
    Loops,
    Gates,
    Body,
}

impl CollectionDomain {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Repository => "repository",
            Self::Sessions => "state.sessions",
            Self::Plans => "state.plans",
            Self::Decisions => "state.decisions",
            Self::Receipts => "state.receipts",
            Self::Loops => "loops",
            Self::Gates => "gates",
            Self::Body => "body",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceError {
    Cancelled,
    NoCurrentEpoch,
    Collection {
        domain: CollectionDomain,
        message: String,
    },
    InternalContract {
        message: String,
    },
}

impl fmt::Display for SourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("dashboard collection cancelled"),
            Self::NoCurrentEpoch => formatter.write_str("no recorder epoch is available for reuse"),
            Self::Collection { domain, message } => {
                write!(
                    formatter,
                    "{} collection failed: {message}",
                    domain.as_str()
                )
            }
            Self::InternalContract { message } => {
                write!(formatter, "dashboard contract failed: {message}")
            }
        }
    }
}

impl Error for SourceError {}
