use time::OffsetDateTime;

use crate::dashboard::{
    AppliedLimit, BoundedRows, BoundedText, ExhaustedAttempt, Failure, HarnessObservation,
    LoopAttempt, LoopLease, LoopObservation, LoopStateError, LoopWorkflow, RecorderEpochId,
    RecorderLimits, RecorderSnapshot, ScheduledOccurrence, SnapshotError, TargetStat, TimelineRow,
};

use super::sanitize_text;

mod health;
use health::health_items;

#[derive(Clone, Debug)]
pub(crate) struct LocalDashboard {
    pub(crate) generated_at_ms: u64,
    pub(crate) epoch_id: RecorderEpochId,
    pub(crate) repo: LocalRepositoryView,
    pub(crate) harness: LocalHarnessView,
    pub(crate) failures: Vec<FailureView>,
    pub(crate) targets: Vec<TargetView>,
    pub(crate) health: Vec<HealthItemView>,
    pub(crate) timeline: Vec<TimelineItemView>,
    pub(crate) timeline_limit: usize,
    pub(crate) limits: LocalLimitsView,
    pub(crate) errors: Vec<LocalErrorView>,
}

impl From<RecorderSnapshot> for LocalDashboard {
    fn from(mut snapshot: RecorderSnapshot) -> Self {
        snapshot
            .failures
            .sort_by_key(|failure| std::cmp::Reverse(failure.ended_at_ms));
        snapshot
            .timeline
            .sort_by_key(|row| std::cmp::Reverse(row.timestamp_ms));
        let failures = snapshot
            .failures
            .into_iter()
            .map(FailureView::from)
            .collect::<Vec<_>>();
        let targets = snapshot
            .target_stats
            .into_iter()
            .map(TargetView::from)
            .collect::<Vec<_>>();
        let health = health_items(&failures, &targets, snapshot.loops.as_ref());
        Self {
            generated_at_ms: snapshot.generated_at_ms,
            epoch_id: snapshot.epoch_id,
            repo: snapshot.repo.into(),
            harness: snapshot.harness.into(),
            failures,
            targets,
            health,
            timeline: snapshot
                .timeline
                .into_iter()
                .map(TimelineItemView::from)
                .collect(),
            timeline_limit: snapshot.timeline_limit,
            limits: snapshot.limits.into(),
            errors: snapshot.errors.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LocalRepositoryView {
    pub(crate) name: String,
    pub(crate) default_branch: String,
    pub(crate) source_commit: Option<String>,
    pub(crate) source_path: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) detached: bool,
}

impl From<crate::dashboard::RepositoryObservation> for LocalRepositoryView {
    fn from(repo: crate::dashboard::RepositoryObservation) -> Self {
        Self {
            name: sanitize_text(&repo.name),
            default_branch: sanitize_text(&repo.default_branch),
            source_commit: repo.source_commit.as_deref().map(sanitize_text),
            source_path: repo.source_path.as_deref().map(sanitize_text),
            branch: repo.branch.as_deref().map(sanitize_text),
            detached: repo.detached,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LocalHarnessView {
    pub(crate) runtime_version: String,
    pub(crate) contract_version: u64,
}

impl From<HarnessObservation> for LocalHarnessView {
    fn from(harness: HarnessObservation) -> Self {
        let runtime = if harness.runtime_version.is_empty() {
            harness.jig_version.unwrap_or_else(|| "-".to_string())
        } else {
            harness.runtime_version
        };
        Self {
            runtime_version: sanitize_text(&runtime),
            contract_version: harness.contract_version,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FailureView {
    pub(crate) identity: String,
    pub(crate) run_id: String,
    pub(crate) target: String,
    pub(crate) ended_at: String,
    pub(crate) outcome: String,
    pub(crate) output: TextView,
}

impl From<Failure> for FailureView {
    fn from(failure: Failure) -> Self {
        Self {
            identity: format!("failure:{}:{}", failure.run_id, failure.target),
            run_id: sanitize_text(&failure.run_id),
            target: sanitize_text(&failure.target),
            ended_at: format_timestamp(failure.ended_at_ms),
            outcome: outcome_label(Some(&failure.conclusion), failure.exit_code),
            output: failure.output_tail.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TargetView {
    pub(crate) raw_target: String,
    pub(crate) target: String,
    pub(crate) runs: u64,
    pub(crate) failures: u64,
    pub(crate) last_status: String,
    pub(crate) last_ended_at: String,
    pub(crate) average: String,
}

impl From<TargetStat> for TargetView {
    fn from(stat: TargetStat) -> Self {
        Self {
            target: sanitize_text(&stat.target),
            raw_target: stat.target,
            runs: stat.runs,
            failures: stat.failures,
            last_status: outcome_label(stat.last_conclusion.as_deref(), None),
            last_ended_at: format_timestamp(Some(stat.last_ended_at_ms)),
            average: format_duration(Some(stat.avg_duration_ms)),
        }
    }
}

/// Explicit text for a target outcome, so no status relies on color alone.
fn outcome_label(conclusion: Option<&str>, exit_code: Option<i64>) -> String {
    let conclusion = conclusion.map_or_else(|| "unfinished".to_string(), sanitize_text);
    match exit_code {
        Some(code) if conclusion != "success" => format!("{conclusion} (exit {code})"),
        _ => conclusion,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TimelineFilter {
    All,
    Failures,
}

impl TimelineFilter {
    pub(crate) const ALL: [Self; 2] = [Self::All, Self::Failures];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Failures => "failures",
        }
    }

    pub(crate) fn matches(self, row: &TimelineItemView) -> bool {
        match self {
            Self::All => true,
            Self::Failures => row.failed,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TimelineItemView {
    pub(crate) identity: String,
    pub(crate) display_identity: String,
    pub(crate) timestamp: String,
    pub(crate) primary: String,
    pub(crate) secondary: String,
    pub(crate) failed: bool,
    pub(crate) detail: DetailDocument,
}

impl From<TimelineRow> for TimelineItemView {
    fn from(row: TimelineRow) -> Self {
        let failed = matches!(
            row.conclusion.as_deref(),
            Some("failure" | "timed_out" | "blocked")
        );
        let outcome = outcome_label(row.conclusion.as_deref(), row.exit_code);
        let mut lines = vec![
            field("Run", &row.run_id),
            field("Target", &row.target),
            field("Status", &row.status),
            format!("Conclusion: {outcome}"),
            format!(
                "Exit: {}",
                row.exit_code
                    .map_or_else(|| "—".to_string(), |code| code.to_string())
            ),
            format!("Started: {}", format_timestamp(row.started_at_ms)),
            format!("Ended: {}", format_timestamp(row.ended_at_ms)),
            format!("Duration: {}", format_duration(row.duration_ms)),
        ];
        if let Some(count) = row.finding_count {
            lines.push(format!("Findings: {count}"));
        }
        if let Some(output) = row.output_tail {
            append_text(&mut lines, "Output tail", &output.into());
        }
        TimelineItemView {
            display_identity: sanitize_text(&row.stable_identity),
            identity: row.stable_identity,
            timestamp: format_timestamp(row.timestamp_ms),
            primary: format!("{} {outcome}", sanitize_text(&row.target)),
            secondary: format!(
                "{} · {}",
                format_duration(row.duration_ms),
                sanitize_text(&row.run_id)
            ),
            failed,
            detail: DetailDocument::new("Target result", lines),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct HealthItemView {
    pub(crate) identity: String,
    pub(crate) section: &'static str,
    pub(crate) primary: String,
    pub(crate) secondary: String,
    pub(crate) detail: DetailDocument,
}

#[derive(Clone, Debug)]
pub(crate) struct DetailDocument {
    pub(crate) title: String,
    pub(crate) lines: Vec<String>,
}

impl DetailDocument {
    pub(crate) fn new(title: &str, lines: Vec<String>) -> Self {
        Self {
            title: title.to_string(),
            lines,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TextView {
    pub(crate) lines: Vec<String>,
    pub(crate) limit: LimitView,
}

impl From<BoundedText> for TextView {
    fn from(text: BoundedText) -> Self {
        let sanitized = sanitize_multiline(text.text());
        let lines = sanitized.lines().map(ToOwned::to_owned).collect();
        Self {
            lines,
            limit: LimitView {
                applied: text.applied_chars(),
                omitted: text.omitted_chars(),
            },
        }
    }
}

fn sanitize_multiline(text: &str) -> String {
    text.replace("\r\n", "\n")
        .split('\n')
        .map(|line| sanitize_text(&line.replace('\t', "    ")))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LimitView {
    pub(crate) applied: usize,
    pub(crate) omitted: Option<usize>,
}

impl LimitView {
    pub(crate) fn from_rows<T>(rows: &BoundedRows<T>) -> Self {
        Self {
            applied: rows.applied(),
            omitted: rows.omitted(),
        }
    }

    pub(crate) fn label(self, noun: &str) -> String {
        match self.omitted {
            Some(0) => format!("limit {} {noun}; none omitted", self.applied),
            Some(count) => format!("limit {} {noun}; {count} omitted", self.applied),
            None => format!("limit {} {noun}; omitted count unknown", self.applied),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LocalLimitsView {
    pub(crate) failures: LimitView,
    pub(crate) targets: LimitView,
    pub(crate) timeline: LimitView,
}

impl From<RecorderLimits> for LocalLimitsView {
    fn from(limits: RecorderLimits) -> Self {
        Self {
            failures: limits.failures.into(),
            targets: limits.target_stats.into(),
            timeline: limits.timeline.into(),
        }
    }
}

impl From<AppliedLimit> for LimitView {
    fn from(limit: AppliedLimit) -> Self {
        Self {
            applied: limit.applied,
            omitted: limit.omitted,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LocalErrorView {
    pub(crate) scope: String,
    pub(crate) code: String,
    pub(crate) subject: Option<String>,
    pub(crate) message: String,
}

impl From<SnapshotError> for LocalErrorView {
    fn from(error: SnapshotError) -> Self {
        Self {
            scope: sanitize_text(error.scope()),
            code: sanitize_text(error.code()),
            subject: error.subject_id().map(sanitize_text),
            message: sanitize_text(error.message()),
        }
    }
}

pub(crate) fn format_timestamp(timestamp_ms: Option<u64>) -> String {
    let Some(ms) = timestamp_ms else {
        return "—".to_string();
    };
    let Ok(seconds) = i64::try_from(ms / 1_000) else {
        return format!("{ms}ms");
    };
    let Ok(time) = OffsetDateTime::from_unix_timestamp(seconds) else {
        return format!("{ms}ms");
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}Z",
        time.year(),
        u8::from(time.month()),
        time.day(),
        time.hour(),
        time.minute(),
        time.second()
    )
}

pub(crate) fn format_duration(duration_ms: Option<u64>) -> String {
    match duration_ms {
        None => "—".to_string(),
        Some(ms) if ms < 1_000 => format!("{ms}ms"),
        Some(ms) if ms < 60_000 => format!("{:.1}s", ms as f64 / 1_000.0),
        Some(ms) => format!("{}m {}s", ms / 60_000, (ms % 60_000) / 1_000),
    }
}

fn field(label: &str, value: &str) -> String {
    format!("{label}: {}", sanitize_text(value))
}

fn push_optional(lines: &mut Vec<String>, label: &str, value: Option<&str>) {
    if let Some(value) = value {
        lines.push(field(label, value));
    }
}

fn append_text(lines: &mut Vec<String>, label: &str, text: &TextView) {
    lines.push(format!("{label}:"));
    lines.extend(text.lines.iter().cloned());
    lines.push(text.limit.label("characters"));
}
