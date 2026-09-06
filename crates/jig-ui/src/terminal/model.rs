use crate::dashboard::{
    StatusCollectionError, StatusLocalSnapshot, StatusLoopObservation, StatusRepositoryObservation,
    StatusWorkSnapshot, UpstreamObservation,
};

mod app;
mod detail;
mod local;
mod support;

pub(crate) use app::*;
pub(crate) use detail::*;
pub(crate) use jig_tui::sanitize_text;
pub(crate) use local::*;
use support::moved_index;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Tab {
    Status,
    Work,
    Timeline,
    Health,
}

impl Tab {
    pub(crate) const ALL: [Self; 4] = [Self::Status, Self::Work, Self::Timeline, Self::Health];

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Status => 0,
            Self::Work => 1,
            Self::Timeline => 2,
            Self::Health => 3,
        }
    }

    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Status => "1 Status",
            Self::Work => "2 Work",
            Self::Timeline => "3 Timeline",
            Self::Health => "4 Health",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Dashboard {
    pub(crate) outcome: String,
    pub(crate) observed_at_ms: u64,
    pub(crate) repository: RepositoryView,
    pub(crate) work: WorkView,
    pub(crate) loops: LoopView,
    pub(crate) errors: Vec<CollectionErrorView>,
}

impl From<StatusLocalSnapshot> for Dashboard {
    fn from(snapshot: StatusLocalSnapshot) -> Self {
        let outcome = if snapshot.errors.is_empty() {
            "complete"
        } else {
            "partial"
        };
        Self {
            outcome: outcome.to_string(),
            observed_at_ms: snapshot.observed_at_ms,
            repository: snapshot.repository.into(),
            work: WorkView::from_snapshot(&snapshot.work),
            loops: LoopView::from_snapshot(snapshot.loops.as_ref()),
            errors: snapshot.errors.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RepositoryView {
    pub(crate) name: String,
    pub(crate) default_branch: String,
    pub(crate) head_revision: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) detached: bool,
    pub(crate) dirty: Option<bool>,
    pub(crate) upstream: Option<UpstreamView>,
}

impl From<StatusRepositoryObservation> for RepositoryView {
    fn from(observation: StatusRepositoryObservation) -> Self {
        Self {
            name: sanitize_text(&observation.name),
            default_branch: sanitize_text(&observation.default_branch),
            head_revision: observation.head_revision.as_deref().map(sanitize_text),
            branch: observation.branch.as_deref().map(sanitize_text),
            detached: observation.detached,
            dirty: observation.dirty,
            upstream: observation.upstream.map(Into::into),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct UpstreamView {
    pub(crate) reference: String,
    pub(crate) ahead: u64,
    pub(crate) behind: u64,
    pub(crate) state: String,
    pub(crate) basis: String,
}

impl From<UpstreamObservation> for UpstreamView {
    fn from(observation: UpstreamObservation) -> Self {
        Self {
            reference: sanitize_text(&observation.reference),
            ahead: observation.ahead,
            behind: observation.behind,
            state: sanitize_text(&observation.state),
            basis: sanitize_text(&observation.basis),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct WorkView {
    pub(crate) open_plans: u64,
    pub(crate) current_session_id: Option<String>,
    pub(crate) gate_snapshots: usize,
    pub(crate) gate_errors: usize,
}

impl WorkView {
    fn from_snapshot(work: &StatusWorkSnapshot) -> Self {
        Self {
            open_plans: work
                .state
                .as_ref()
                .map_or(0, |state| state.counts.open_plans),
            current_session_id: work
                .state
                .as_ref()
                .and_then(|state| state.current_session_id.as_deref())
                .map(sanitize_text),
            gate_snapshots: work.gates.len(),
            gate_errors: work
                .gates
                .iter()
                .filter(|gate| gate.error.is_some())
                .count(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct LoopView {
    pub(crate) workflows: usize,
    pub(crate) leases: usize,
    pub(crate) attempts: usize,
    pub(crate) waiting_attempts: usize,
    pub(crate) exhausted_attempts: usize,
}

impl LoopView {
    fn from_snapshot(loops: Option<&StatusLoopObservation>) -> Self {
        loops.map_or_else(Self::default, |loops| Self {
            workflows: loops.workflows.len(),
            leases: loops.leases.len(),
            attempts: loops.attempts.len(),
            waiting_attempts: loops.waiting_attempts.len(),
            exhausted_attempts: loops.needs_attention.exhausted_attempts.len(),
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CollectionErrorView {
    pub(crate) scope: String,
    pub(crate) code: String,
    pub(crate) message: String,
}

impl From<StatusCollectionError> for CollectionErrorView {
    fn from(error: StatusCollectionError) -> Self {
        Self {
            scope: sanitize_text(&error.scope),
            code: sanitize_text(&error.code),
            message: sanitize_text(&error.message),
        }
    }
}
