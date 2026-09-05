use std::collections::{BTreeMap, HashMap};

use anyhow::{Context, Result};
use jig_ui::dashboard::*;
use sha2::{Digest, Sha256};

use crate::context::RepoContext;
use crate::state::{
    DashboardDecisionRecord, DashboardPlanEvent, DashboardReceiptRecord, DashboardSessionEvent,
    JsonlRecordTooLarge, PlanFileError, PlanFileErrorKind, RawJsonlRecord, current_session,
    read_plan_body, read_receipts_reverse_with_cancellation, receipt_diff_summary,
    scan_dashboard_jsonl_raw,
};

const STATUS_RECENT_ROWS: usize = 10;
pub(in crate::ui::source) const MAX_AGGREGATION_KEYS: usize = 4_096;

pub(super) struct LocalObservationEpoch {
    id: RecorderEpochId,
    observed_at_ms: u64,
    context: RepoContext,
    repository: StatusRepositoryObservation,
    status_repository_errors: Vec<StatusCollectionError>,
    current_session_id: Option<String>,
    current_session_error: Option<SnapshotError>,
    sessions: StreamSection<SessionFacts>,
    plans: StreamSection<PlanFacts>,
    decisions: StreamSection<DecisionFacts>,
    receipts: StreamSection<ReceiptFacts>,
    loops: Option<StatusLoopObservation>,
    loop_error: Option<SnapshotError>,
    gates: BTreeMap<String, GateFacts>,
}

#[derive(Clone)]
struct StreamSection<T> {
    data: T,
    error: Option<SnapshotError>,
}

#[derive(Clone, Default)]
struct SessionFacts {
    starts: u64,
    events: u64,
    timeline: Vec<TimelineRow>,
}

#[derive(Clone, Default)]
struct PlanFacts {
    distinct: BTreeMap<String, PlanInfo>,
    open_events: u64,
    events: u64,
    timeline: Vec<TimelineRow>,
    gate_errors: BTreeMap<String, String>,
}

#[derive(Clone, Default)]
struct DecisionFacts {
    count: u64,
    recent: Vec<StatusDecisionSummary>,
    timeline: Vec<TimelineRow>,
}

#[derive(Clone, Default)]
struct ReceiptFacts {
    count: u64,
    failed: u64,
    recent: Vec<StatusReceiptSummary>,
    failures: Vec<Failure>,
    tool_stats: Vec<ToolStat>,
    tool_count: usize,
    timeline: Vec<TimelineRow>,
}

#[derive(Clone, Default)]
struct MutableReceiptFacts {
    count: u64,
    failed: u64,
    recent: Vec<StatusReceiptSummary>,
    failures: Vec<Failure>,
    tools: BTreeMap<String, MutableToolStat>,
    timeline: Vec<TimelineRow>,
}

#[derive(Clone)]
struct MutableToolStat {
    runs: u64,
    failures: u64,
    total_duration_ms: u64,
    last_exit_status: i64,
    last_ended_at_ms: u64,
}

#[derive(Clone, Default)]
struct PlanInfo {
    title: String,
    body_path: Option<String>,
    opened_at_ms: Option<u64>,
    closed_at_ms: Option<u64>,
    resolution: Option<String>,
    baseline: Option<crate::state::PlanBaseline>,
    opened: bool,
    closed: bool,
}

#[derive(Clone, Default)]
struct GateFacts {
    status: Option<StatusGateReport>,
    recorder: Option<GatesObservation>,
    error: Option<String>,
}

impl LocalObservationEpoch {
    pub(super) fn collect(
        context: &RepoContext,
        id: RecorderEpochId,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, SourceError> {
        Self::collect_with_repository(context, id, None, cancelled)
    }

    pub(super) fn collect_with_repository(
        context: &RepoContext,
        id: RecorderEpochId,
        observed_repository: Option<(StatusRepositoryObservation, Vec<StatusCollectionError>)>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, SourceError> {
        ensure_active(cancelled)?;
        let observed_at_ms = crate::state::now_ms();
        let (repository, status_repository_errors) = match observed_repository {
            Some(repository) => repository,
            None => {
                crate::status::dashboard_repository_snapshot_with_cancellation(context, cancelled)
                    .map_err(|error| {
                    collection_error_for(CollectionDomain::Repository, error, cancelled)
                })?
            }
        };

        let sessions = collect_sessions(context, cancelled)?;
        let plans = collect_plans(context, cancelled)?;
        let decisions = collect_decisions(context, cancelled)?;
        let open_plan_ids = plans
            .data
            .distinct
            .iter()
            .filter(|(_, plan)| plan.opened && !plan.closed)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let gate_indexes =
            crate::runtime::dashboard_gate_receipt_indexes(context, &open_plan_ids, cancelled)
                .map_err(|error| collection_error_for(CollectionDomain::Gates, error, cancelled))?;
        let (receipts, gate_indexes) = collect_receipts(context, gate_indexes, cancelled)?;
        ensure_active(cancelled)?;
        let (current_session_id, current_session_error) = match current_session(context) {
            Ok(session) => (session, None),
            Err(error) => (
                None,
                Some(SnapshotError::new(
                    CollectionDomain::Sessions,
                    SnapshotErrorCode::StreamReadFailed,
                    None,
                    format!("failed to read current session: {error:#}"),
                )),
            ),
        };

        let (loops, loop_error) = match crate::runtime::typed_loop_status_snapshot_with_cancellation(
            context, cancelled,
        ) {
            Ok(loops) => (Some(loops), None),
            Err(error) if crate::cancellation::is_status_collection_cancellation(&error) => {
                return Err(SourceError::Cancelled);
            }
            Err(error) => (
                None,
                Some(SnapshotError::new(
                    CollectionDomain::Loops,
                    SnapshotErrorCode::LoopObservationFailed,
                    None,
                    format!("{error:#}"),
                )),
            ),
        };
        ensure_active(cancelled)?;

        let open_plan_baselines = plans
            .data
            .distinct
            .iter()
            .filter(|(_, plan)| plan.opened && !plan.closed)
            .map(|(id, plan)| (id.clone(), plan.baseline.clone()))
            .collect::<BTreeMap<_, _>>();
        let gate_collection_error = plans
            .error
            .as_ref()
            .or(receipts.error.as_ref())
            .map(|error| {
                format!(
                    "gate evidence is unavailable because {} failed: {}",
                    error.scope(),
                    error.message()
                )
            });
        let mut gates = match &gate_collection_error {
            Some(message) => open_plan_baselines
                .keys()
                .map(|plan_id| {
                    (
                        plan_id.clone(),
                        GateFacts {
                            error: Some(message.clone()),
                            ..GateFacts::default()
                        },
                    )
                })
                .collect(),
            None => collect_gates(
                context,
                &open_plan_baselines,
                gate_indexes.into_indexes(),
                "open",
                cancelled,
            )?,
        };
        if gate_collection_error.is_none() {
            for (plan_id, error) in &plans.data.gate_errors {
                if open_plan_baselines.contains_key(plan_id) {
                    gates.insert(
                        plan_id.clone(),
                        GateFacts {
                            error: Some(error.clone()),
                            ..GateFacts::default()
                        },
                    );
                }
            }
        }

        Ok(Self {
            id,
            observed_at_ms,
            context: context.clone(),
            repository,
            status_repository_errors,
            current_session_id,
            current_session_error,
            sessions,
            plans,
            decisions,
            receipts,
            loops,
            loop_error,
            gates,
        })
    }

    pub(super) const fn id(&self) -> RecorderEpochId {
        self.id
    }

    pub(super) fn status_local(&self) -> StatusLocalSnapshot {
        let mut errors = self.status_repository_errors.clone();
        errors.extend(self.status_snapshot_errors().into_iter().map(status_error));
        let state_available = self.state_error().is_none();
        StatusLocalSnapshot {
            epoch_id: self.id,
            observed_at_ms: self.observed_at_ms,
            repository: self.repository.clone(),
            work: StatusWorkSnapshot {
                state: state_available.then(|| self.status_state()),
                gates: if state_available {
                    self.status_gates()
                } else {
                    Vec::new()
                },
            },
            loops: self.loops.clone(),
            errors,
        }
    }

    pub(super) fn recorder(
        &self,
        timeline_limit: TimelineLimit,
    ) -> Result<RecorderSnapshot, SourceError> {
        let mut snapshot = RecorderSnapshot::new(self.id, self.observed_at_ms, timeline_limit);
        snapshot.repo = RepositoryObservation {
            name: self.context.repo_name().to_string(),
            default_branch: self.context.default_branch().to_string(),
            source_commit: Some(self.context.source_commit().to_string()),
            source_path: Some(self.context.source_path().to_string()),
            branch: self.repository.branch.clone(),
            detached: self.repository.detached,
        };
        snapshot.harness = HarnessObservation {
            jig_version: self.context.legacy_jig_version().map(str::to_string),
            runtime_version: env!("CARGO_PKG_VERSION").to_string(),
            contract_version: u64::from(self.context.contract_version()),
        };
        snapshot
            .current_session_id
            .clone_from(&self.current_session_id);
        snapshot.counts = RecorderCounts {
            sessions: self.sessions.data.starts,
            session_events: self.sessions.data.events,
            plans: self.plans.data.open_events,
            plan_events: self.plans.data.events,
            open_plans: self.open_plan_count(),
            decisions: self.decisions.data.count,
        };
        snapshot.open_plans = self.recorder_open_plans();
        snapshot.history = self.history();
        snapshot.failures = self.receipts.data.failures.clone();
        snapshot.tool_stats = self.receipts.data.tool_stats.clone();
        snapshot.loops = self.loops.as_ref().map(recorder_loops).transpose()?;
        snapshot.timeline = self.timeline(timeline_limit.get());
        snapshot.limits = RecorderLimits {
            open_plans: root_limit(
                LimitId::OpenPlans,
                Some(
                    self.open_plan_count_usize()
                        .saturating_sub(snapshot.open_plans.len()),
                ),
            )
            .map_err(limit_error)?,
            history: root_limit(
                LimitId::History,
                Some(
                    self.closed_plan_count()
                        .saturating_sub(snapshot.history.len()),
                ),
            )
            .map_err(limit_error)?,
            failures: root_limit(
                LimitId::Failures,
                Some(
                    usize::try_from(self.receipts.data.failed)
                        .unwrap_or(usize::MAX)
                        .saturating_sub(snapshot.failures.len()),
                ),
            )
            .map_err(limit_error)?,
            tool_stats: root_limit(
                LimitId::ToolStats,
                Some(
                    self.receipts
                        .data
                        .tool_count
                        .saturating_sub(snapshot.tool_stats.len()),
                ),
            )
            .map_err(limit_error)?,
            timeline: AppliedLimit {
                applied: timeline_limit.get(),
                omitted: Some(
                    self.timeline_total()
                        .saturating_sub(snapshot.timeline.len()),
                ),
            },
        };
        snapshot.errors = self.recorder_errors();
        Ok(snapshot)
    }

    pub(super) fn plan(
        &self,
        plan_id: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PlanSnapshotResult, SourceError> {
        retained_plan(
            &self.context,
            self.id,
            self.observed_at_ms,
            &self.plans,
            &self.gates,
            plan_id,
            cancelled,
        )
    }

    pub(super) fn fresh_plan(
        context: &RepoContext,
        id: RecorderEpochId,
        plan_id: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PlanSnapshotResult, SourceError> {
        fresh_plan(context, id, plan_id, cancelled)
    }

    fn status_state(&self) -> StatusStateSnapshot {
        StatusStateSnapshot {
            ok: true,
            repo: StatusStateRepository {
                name: self.context.repo_name().to_string(),
                default_branch: self.context.default_branch().to_string(),
                source_commit: Some(self.context.source_commit().to_string()),
                source_path: Some(crate::state::public_source_path(&self.context)),
            },
            current_session_id: self.current_session_id.clone(),
            counts: StatusStateCounts {
                sessions: self.sessions.data.starts,
                session_events: self.sessions.data.events,
                plans: self.plans.data.open_events,
                plan_events: self.plans.data.events,
                open_plans: self.open_plan_count(),
                receipts: self.receipts.data.count,
                failed_receipts: self.receipts.data.failed,
                decisions: self.decisions.data.count,
            },
            open_plans: self
                .plans
                .data
                .distinct
                .iter()
                .filter(|(_, plan)| plan.opened && !plan.closed)
                .map(|(id, plan)| plan.status_open(id))
                .collect(),
            recent_receipts: self.receipts.data.recent.clone(),
            recent_decisions: self.decisions.data.recent.clone(),
        }
    }

    fn status_gates(&self) -> Vec<StatusPlanGates> {
        self.plans
            .data
            .distinct
            .iter()
            .filter(|(_, plan)| plan.opened && !plan.closed)
            .map(|(plan_id, _)| {
                let gate = self.gates.get(plan_id).cloned().unwrap_or_default();
                StatusPlanGates {
                    plan_id: plan_id.clone(),
                    snapshot: gate.status,
                    error: gate.error,
                }
            })
            .collect()
    }

    fn recorder_open_plans(&self) -> Vec<OpenPlan> {
        let mut plans = self
            .plans
            .data
            .distinct
            .iter()
            .filter(|(_, plan)| plan.opened && !plan.closed)
            .map(|(id, plan)| {
                let gate = self.gates.get(id).cloned().unwrap_or_default();
                OpenPlan {
                    plan_id: id.clone(),
                    title: plan.title.clone(),
                    body_path: plan.body_path.clone(),
                    opened_at_ms: plan.opened_at_ms,
                    baseline_ref: plan
                        .baseline
                        .as_ref()
                        .map(|value| value.requested_ref.clone()),
                    baseline_oid: plan.baseline.as_ref().and_then(|value| {
                        value
                            .commit_oid
                            .clone()
                            .or_else(|| value.empty_tree_oid.clone())
                    }),
                    baseline_error: plan.baseline.as_ref().and_then(|value| value.error.clone()),
                    gates: gate.recorder,
                    gates_error: gate.error,
                }
            })
            .collect::<Vec<_>>();
        plans.sort_by(|left, right| {
            right
                .opened_at_ms
                .cmp(&left.opened_at_ms)
                .then_with(|| left.plan_id.cmp(&right.plan_id))
        });
        plans.truncate(LimitId::OpenPlans.ceiling());
        plans
    }

    fn history(&self) -> Vec<PlanSummary> {
        let mut plans = self
            .plans
            .data
            .distinct
            .iter()
            .filter(|(_, plan)| plan.opened && plan.closed)
            .map(|(id, plan)| plan.summary(id))
            .collect::<Vec<_>>();
        plans.sort_by(|left, right| {
            right
                .closed_at_ms
                .cmp(&left.closed_at_ms)
                .then_with(|| left.plan_id.cmp(&right.plan_id))
        });
        plans.truncate(LimitId::History.ceiling());
        plans
    }

    fn timeline(&self, limit: usize) -> Vec<TimelineRow> {
        let mut rows = self
            .sessions
            .data
            .timeline
            .iter()
            .chain(&self.plans.data.timeline)
            .chain(&self.decisions.data.timeline)
            .chain(&self.receipts.data.timeline)
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            timeline_timestamp(right)
                .cmp(&timeline_timestamp(left))
                .then_with(|| left.stable_identity().cmp(right.stable_identity()))
        });
        rows.truncate(limit);
        rows
    }

    fn timeline_total(&self) -> usize {
        usize::try_from(
            self.sessions.data.events
                + self.plans.data.events
                + self.decisions.data.count
                + self.receipts.data.count,
        )
        .unwrap_or(usize::MAX)
    }

    fn open_plan_count(&self) -> u64 {
        u64::try_from(self.open_plan_count_usize()).unwrap_or(u64::MAX)
    }

    fn open_plan_count_usize(&self) -> usize {
        self.plans
            .data
            .distinct
            .values()
            .filter(|plan| plan.opened && !plan.closed)
            .count()
    }

    fn closed_plan_count(&self) -> usize {
        self.plans
            .data
            .distinct
            .values()
            .filter(|plan| plan.opened && plan.closed)
            .count()
    }

    fn snapshot_errors(&self) -> Vec<SnapshotError> {
        [
            self.sessions.error.clone(),
            self.current_session_error.clone(),
            self.plans.error.clone(),
            self.decisions.error.clone(),
            self.receipts.error.clone(),
            self.loop_error.clone(),
        ]
        .into_iter()
        .flatten()
        .chain(
            self.gates
                .iter()
                .filter_map(|(id, gate)| gate.error.as_ref().map(|error| (id, error)))
                .map(|(id, error)| {
                    SnapshotError::new(
                        CollectionDomain::Gates,
                        SnapshotErrorCode::GateObservationFailed,
                        Some(id.clone()),
                        error,
                    )
                }),
        )
        .collect()
    }

    fn state_error(&self) -> Option<&SnapshotError> {
        self.sessions
            .error
            .as_ref()
            .or(self.plans.error.as_ref())
            .or(self.receipts.error.as_ref())
            .or(self.decisions.error.as_ref())
            .or(self.current_session_error.as_ref())
    }

    fn status_snapshot_errors(&self) -> Vec<SnapshotError> {
        let state_error = self.state_error().cloned();
        let mut errors = state_error.clone().into_iter().collect::<Vec<_>>();
        if state_error.is_none() {
            errors.extend(self.gates.iter().filter_map(|(id, gate)| {
                gate.error.as_ref().map(|error| {
                    SnapshotError::new(
                        CollectionDomain::Gates,
                        SnapshotErrorCode::GateObservationFailed,
                        Some(id.clone()),
                        error,
                    )
                })
            }));
        }
        errors.extend(self.loop_error.clone());
        errors
    }

    fn recorder_errors(&self) -> Vec<SnapshotError> {
        self.status_repository_errors
            .iter()
            .map(|error| {
                let code = match error.code.as_str() {
                    "git_upstream_comparison_failed" => {
                        SnapshotErrorCode::GitUpstreamComparisonFailed
                    }
                    "git_upstream_output_invalid" => SnapshotErrorCode::GitUpstreamOutputInvalid,
                    _ => SnapshotErrorCode::GitObservationFailed,
                };
                SnapshotError::new(
                    CollectionDomain::Repository,
                    code,
                    None,
                    error.message.clone(),
                )
            })
            .chain(self.snapshot_errors())
            .collect()
    }
}

impl PlanInfo {
    fn status_open(&self, plan_id: &str) -> StatusOpenPlan {
        StatusOpenPlan {
            plan_id: plan_id.to_string(),
            title: self.title.clone(),
            body_path: self.body_path.clone(),
            baseline: self.baseline.clone().map(status_baseline),
        }
    }

    fn summary(&self, plan_id: &str) -> PlanSummary {
        PlanSummary {
            plan_id: plan_id.to_string(),
            title: self.title.clone(),
            state: if self.closed { "closed" } else { "open" }.to_string(),
            opened_at_ms: self.opened_at_ms,
            closed_at_ms: self.closed_at_ms,
            resolution: self.resolution.clone(),
            duration_ms: self
                .opened_at_ms
                .zip(self.closed_at_ms)
                .map(|(opened, closed)| closed.saturating_sub(opened)),
            baseline_ref: self
                .baseline
                .as_ref()
                .map(|value| value.requested_ref.clone()),
            baseline_oid: self.baseline.as_ref().and_then(|value| {
                value
                    .commit_oid
                    .clone()
                    .or_else(|| value.empty_tree_oid.clone())
            }),
            baseline_error: self.baseline.as_ref().and_then(|value| value.error.clone()),
        }
    }
}

mod collect;
mod plan;
mod support;

use collect::*;
use plan::*;
pub(super) use support::collection_error;
use support::*;
