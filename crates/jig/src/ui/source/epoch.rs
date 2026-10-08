use std::collections::BTreeMap;

use anyhow::{Context, Result};
use jig_context::RepoContext;
use jig_contract::{RunConclusion, TargetOutputTailV1};
use jig_dashboard::*;
use sha2::{Digest, Sha256};

use jig_state::{
    CompletedTargetEvent, JsonlRecordTooLarge, RawJsonlRecord, RunHistoryEvent, run_history_event,
    scan_dashboard_jsonl_raw,
};

pub(in crate::ui::source) const MAX_AGGREGATION_KEYS: usize = 4_096;

pub(super) struct LocalObservationEpoch {
    id: RecorderEpochId,
    observed_at_ms: u64,
    context: RepoContext,
    repository: StatusRepositoryObservation,
    status_repository_errors: Vec<StatusCollectionError>,
    runs: StreamSection<RunFacts>,
    loops: Option<StatusLoopObservation>,
    loop_error: Option<SnapshotError>,
}

#[derive(Clone)]
struct StreamSection<T> {
    data: T,
    error: Option<SnapshotError>,
}

#[derive(Clone, Default)]
struct RunFacts {
    count: u64,
    failed: u64,
    failures: Vec<Failure>,
    target_stats: Vec<TargetStat>,
    target_count: usize,
    timeline: Vec<TimelineRow>,
}

#[derive(Clone, Default)]
struct MutableRunFacts {
    count: u64,
    failed: u64,
    failures: Vec<Failure>,
    targets: BTreeMap<String, MutableTargetStat>,
    timeline: Vec<TimelineRow>,
}

#[derive(Clone)]
struct MutableTargetStat {
    runs: u64,
    failures: u64,
    total_duration_ms: u64,
    last_conclusion: Option<String>,
    last_ended_at_ms: u64,
}

impl LocalObservationEpoch {
    pub(super) fn collect(
        context: &RepoContext,
        id: RecorderEpochId,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, SourceError> {
        ensure_active(cancelled)?;
        let observed_at_ms = jig_state::now_ms();
        let (repository, status_repository_errors) =
            crate::status::dashboard_repository_snapshot_with_cancellation(context, cancelled)
                .map_err(|error| {
                    collection_error_for(CollectionDomain::Repository, error, cancelled)
                })?;

        let runs = collect_runs(context, cancelled)?;
        ensure_active(cancelled)?;

        let (loops, loop_error) = match crate::runtime::typed_loop_status_snapshot_with_cancellation(
            context, cancelled,
        ) {
            Ok(loops) => (Some(loops), None),
            Err(error) if jig_state::cancellation::is_status_collection_cancellation(&error) => {
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

        Ok(Self {
            id,
            observed_at_ms,
            context: context.clone(),
            repository,
            status_repository_errors,
            runs,
            loops,
            loop_error,
        })
    }

    pub(super) const fn id(&self) -> RecorderEpochId {
        self.id
    }

    pub(super) fn status_local(&self) -> StatusLocalSnapshot {
        let mut errors = self.status_repository_errors.clone();
        errors.extend(self.loop_error.clone().map(status_error));
        StatusLocalSnapshot {
            epoch_id: self.id,
            observed_at_ms: self.observed_at_ms,
            repository: self.repository.clone(),
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
        snapshot.failures = self.runs.data.failures.clone();
        snapshot.target_stats = self.runs.data.target_stats.clone();
        snapshot.loops = self.loops.as_ref().map(recorder_loops).transpose()?;
        snapshot.timeline = self.timeline(timeline_limit.get());
        snapshot.limits = RecorderLimits {
            failures: root_limit(
                LimitId::Failures,
                Some(
                    usize::try_from(self.runs.data.failed)
                        .unwrap_or(usize::MAX)
                        .saturating_sub(snapshot.failures.len()),
                ),
            )
            .map_err(limit_error)?,
            target_stats: root_limit(
                LimitId::TargetStats,
                Some(
                    self.runs
                        .data
                        .target_count
                        .saturating_sub(snapshot.target_stats.len()),
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

    fn timeline(&self, limit: usize) -> Vec<TimelineRow> {
        let mut rows = self.runs.data.timeline.clone();
        rows.sort_by(|left, right| {
            timeline_timestamp(right)
                .cmp(&timeline_timestamp(left))
                .then_with(|| left.stable_identity().cmp(right.stable_identity()))
        });
        rows.truncate(limit);
        rows
    }

    fn timeline_total(&self) -> usize {
        usize::try_from(self.runs.data.count).unwrap_or(usize::MAX)
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
            .chain(self.runs.error.clone())
            .chain(self.loop_error.clone())
            .collect()
    }
}

mod collect;
mod support;

use collect::*;
pub(super) use support::collection_error;
use support::*;
