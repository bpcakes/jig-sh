use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;
use serde_json::Value;

use crate::dashboard::{
    STATUS_SCHEMA_VERSION, StatusLocalSnapshot, StatusLoopObservation, StatusOutcome,
    StatusSnapshot, StatusWorkSnapshot,
};

mod app;
mod detail;
mod local;
mod package_detail;
mod support;
mod typed;
#[cfg(test)]
mod wire;

pub(crate) use app::*;
pub(crate) use detail::*;
pub(crate) use jig_tui::sanitize_text;
pub(crate) use local::*;
use package_detail::{AcceptanceCheckView, PackageDetailState};
pub(crate) use package_detail::{DETAIL_SECTION_ITEM_LIMIT, EXTENSION_ROW_LIMIT};
#[cfg(test)]
use support::fallback;
#[cfg(test)]
use support::{array_len, sanitize_value};
use support::{moved_index, nonempty};
pub(super) use typed::category_name;
#[cfg(test)]
use wire::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Tab {
    Status,
    Packages,
    Blockers,
    Work,
    Timeline,
    Health,
}

impl Tab {
    pub(crate) const ALL: [Self; 6] = [
        Self::Status,
        Self::Packages,
        Self::Blockers,
        Self::Work,
        Self::Timeline,
        Self::Health,
    ];

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Status => 0,
            Self::Packages => 1,
            Self::Blockers => 2,
            Self::Work => 3,
            Self::Timeline => 4,
            Self::Health => 5,
        }
    }

    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Status => "1 Status",
            Self::Packages => "2 Packages",
            Self::Blockers => "3 Blockers",
            Self::Work => "4 Work",
            Self::Timeline => "5 Timeline",
            Self::Health => "6 Health",
        }
    }

    pub(crate) const fn is_status_domain(self) -> bool {
        matches!(self, Self::Status | Self::Packages | Self::Blockers)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Dashboard {
    pub(crate) outcome: String,
    pub(crate) observed_at_ms: u64,
    pub(crate) repository: RepositoryView,
    pub(crate) work: WorkView,
    pub(crate) loops: LoopView,
    pub(crate) providers: Vec<ProviderView>,
    pub(crate) errors: Vec<CollectionErrorView>,
}

impl Dashboard {
    #[cfg(test)]
    pub(crate) fn from_value(mut value: Value) -> Result<Self, String> {
        sanitize_value(&mut value);
        let wire: AggregateWire = serde_json::from_value(value)
            .map_err(|error| format!("status aggregate could not be decoded: {error}"))?;
        if wire.schema_version != STATUS_SCHEMA_VERSION {
            return Err(format!(
                "unsupported status aggregate schema version {}; this TUI supports version {STATUS_SCHEMA_VERSION}",
                wire.schema_version
            ));
        }

        Ok(Self {
            outcome: fallback(wire.outcome, "unknown"),
            observed_at_ms: wire.observed_at_ms,
            repository: wire.repository.into(),
            work: WorkView::from_value(&wire.work),
            loops: LoopView::from_value(&wire.loops),
            providers: wire.providers.into_iter().map(ProviderView::from).collect(),
            errors: wire.errors.into_iter().map(Into::into).collect(),
        })
    }

    pub(crate) fn from_snapshot(snapshot: StatusSnapshot) -> Result<Self, String> {
        if snapshot.schema_version != STATUS_SCHEMA_VERSION {
            return Err(format!(
                "unsupported status aggregate schema version {}; this TUI supports version {STATUS_SCHEMA_VERSION}",
                snapshot.schema_version
            ));
        }

        Ok(Self {
            outcome: match snapshot.outcome {
                StatusOutcome::Complete => "complete",
                StatusOutcome::Partial => "partial",
            }
            .to_string(),
            observed_at_ms: snapshot.observed_at_ms,
            repository: snapshot.repository.into(),
            work: WorkView::from_snapshot(&snapshot.work),
            loops: LoopView::from_snapshot(snapshot.loops.as_ref()),
            providers: snapshot
                .providers
                .into_iter()
                .map(ProviderView::from_status)
                .collect(),
            errors: snapshot.errors.into_iter().map(Into::into).collect(),
        })
    }

    pub(crate) fn apply_local_snapshot(&mut self, snapshot: StatusLocalSnapshot) {
        self.observed_at_ms = snapshot.observed_at_ms;
        self.repository = snapshot.repository.into();
        self.work = WorkView::from_snapshot(&snapshot.work);
        self.loops = LoopView::from_snapshot(snapshot.loops.as_ref());
        self.errors = snapshot.errors.into_iter().map(Into::into).collect();
        self.outcome = if self.errors.is_empty()
            && self
                .providers
                .iter()
                .all(|provider| provider.status == "complete")
        {
            "complete"
        } else {
            "partial"
        }
        .to_string();
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

#[cfg(test)]
impl From<RepositoryWire> for RepositoryView {
    fn from(wire: RepositoryWire) -> Self {
        Self {
            name: fallback(wire.name, "<unknown>"),
            default_branch: fallback(wire.default_branch, "<unknown>"),
            head_revision: wire.head_revision,
            branch: wire.branch,
            detached: wire.detached,
            dirty: wire.dirty,
            upstream: wire.upstream.map(Into::into),
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

#[cfg(test)]
impl From<UpstreamWire> for UpstreamView {
    fn from(wire: UpstreamWire) -> Self {
        Self {
            reference: fallback(wire.reference, "<unknown>"),
            ahead: wire.ahead,
            behind: wire.behind,
            state: fallback(wire.state, "unknown"),
            basis: fallback(wire.basis, "unknown"),
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
    #[cfg(test)]
    fn from_value(work: &Value) -> Self {
        let state = work.get("state").unwrap_or(&Value::Null);
        let gates = work
            .get("gates")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        Self {
            open_plans: state
                .get("counts")
                .and_then(|counts| counts.get("open_plans"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
            current_session_id: state
                .get("current_session_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            gate_snapshots: gates.len(),
            gate_errors: gates
                .iter()
                .filter(|gate| gate.get("error").is_some_and(|error| !error.is_null()))
                .count(),
        }
    }

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
    pub(crate) leases: usize,
    pub(crate) attempts: usize,
    pub(crate) exhausted_attempts: usize,
}

impl LoopView {
    #[cfg(test)]
    fn from_value(loops: &Value) -> Self {
        Self {
            leases: array_len(loops, "leases"),
            attempts: array_len(loops, "attempts"),
            exhausted_attempts: loops
                .get("needs_attention")
                .map(|attention| array_len(attention, "exhausted_attempts"))
                .unwrap_or(0),
        }
    }

    fn from_snapshot(loops: Option<&StatusLoopObservation>) -> Self {
        loops.map_or_else(Self::default, |loops| Self {
            leases: loops.leases.len(),
            attempts: loops.attempts.len(),
            exhausted_attempts: loops.needs_attention.exhausted_attempts.len(),
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ProviderView {
    pub(crate) id: String,
    pub(crate) display_id: String,
    pub(crate) display_name: Option<String>,
    pub(crate) adapter_version: Option<String>,
    pub(crate) status: String,
    pub(crate) duration_ms: u64,
    pub(crate) summary: SummaryView,
    pub(crate) input_freshness: Vec<InputFreshnessView>,
    pub(crate) packages: Vec<PackageView>,
    pub(crate) blockers: Vec<BlockerItemView>,
    pub(crate) diagnostics: Vec<DiagnosticView>,
    pub(crate) error: Option<ProviderErrorView>,
}

#[cfg(test)]
impl From<ProviderWire> for ProviderView {
    fn from(wire: ProviderWire) -> Self {
        let report = wire.report.unwrap_or_default();
        let packages = report
            .work_packages
            .into_iter()
            .map(PackageView::from)
            .collect::<Vec<_>>();
        let blockers = packages
            .iter()
            .flat_map(|package| {
                let mut occurrences = HashMap::<BlockerAnchor, usize>::new();
                package.blockers.iter().map(move |blocker| BlockerItemView {
                    key: {
                        let anchor = BlockerAnchor::new(&package.id, blocker);
                        let occurrence = occurrences.entry(anchor.clone()).or_default();
                        let key = BlockerKey {
                            anchor,
                            occurrence: *occurrence,
                        };
                        *occurrence += 1;
                        key
                    },
                    display_package_id: package.display_id.clone(),
                    package_title: package.title.clone(),
                    specification: package.specification.clone(),
                    implementation: package.implementation.clone(),
                    verification: package.verification.clone(),
                    blocker: blocker.clone(),
                })
            })
            .collect::<Vec<_>>();
        let summary = wire
            .summary
            .map(SummaryView::from)
            .unwrap_or_else(|| SummaryView::from_packages(&packages, report.diagnostics.len()));

        let id = fallback(wire.id, "<unknown>");
        Self {
            display_id: sanitize_text(&id),
            id,
            display_name: report.provider.display_name,
            adapter_version: nonempty(report.provider.adapter_version),
            status: fallback(wire.status, "unknown"),
            duration_ms: wire.duration_ms,
            summary,
            input_freshness: wire.input_freshness.into_iter().map(Into::into).collect(),
            packages,
            blockers,
            diagnostics: report.diagnostics.into_iter().map(Into::into).collect(),
            error: wire.error.map(Into::into),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SummaryView {
    pub(crate) work_packages: u64,
    pub(crate) work_packages_with_blockers: u64,
    pub(crate) blockers: u64,
    pub(crate) acceptance_checks: u64,
    pub(crate) diagnostics: DiagnosticCounts,
    pub(crate) specification: CategoryCounts,
    pub(crate) implementation: CategoryCounts,
    pub(crate) verification: CategoryCounts,
    pub(crate) acceptance: CategoryCounts,
}

impl SummaryView {
    #[cfg(test)]
    fn from_packages(packages: &[PackageView], diagnostics: usize) -> Self {
        let mut summary = Self {
            work_packages: packages.len() as u64,
            diagnostics: DiagnosticCounts {
                total: diagnostics as u64,
                ..DiagnosticCounts::default()
            },
            ..Self::default()
        };
        for package in packages {
            if !package.blockers.is_empty() {
                summary.work_packages_with_blockers += 1;
            }
            summary.blockers += package.blockers.len() as u64;
            summary.acceptance_checks += package.acceptance_total;
            summary.specification.add(&package.specification.category);
            summary.implementation.add(&package.implementation.category);
            summary.verification.add(&package.verification.category);
            summary.acceptance.complete += package.acceptance_complete;
            summary.acceptance.pending += package
                .acceptance_total
                .saturating_sub(package.acceptance_complete);
        }
        summary
    }
}

#[cfg(test)]
impl From<SummaryWire> for SummaryView {
    fn from(wire: SummaryWire) -> Self {
        Self {
            work_packages: wire.work_packages,
            work_packages_with_blockers: wire.work_packages_with_blockers,
            blockers: wire.blockers,
            acceptance_checks: wire.acceptance_checks,
            diagnostics: wire.diagnostics,
            specification: wire.specification,
            implementation: wire.implementation,
            verification: wire.verification,
            acceptance: wire.acceptance,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct CategoryCounts {
    pub(crate) unknown: u64,
    pub(crate) pending: u64,
    pub(crate) ready: u64,
    pub(crate) active: u64,
    pub(crate) blocked: u64,
    pub(crate) complete: u64,
    pub(crate) failed: u64,
}

impl CategoryCounts {
    #[cfg(test)]
    fn add(&mut self, category: &str) {
        match category {
            "pending" => self.pending += 1,
            "ready" => self.ready += 1,
            "active" => self.active += 1,
            "blocked" => self.blocked += 1,
            "complete" => self.complete += 1,
            "failed" => self.failed += 1,
            _ => self.unknown += 1,
        }
    }

    pub(crate) const fn total(&self) -> u64 {
        self.unknown
            + self.pending
            + self.ready
            + self.active
            + self.blocked
            + self.complete
            + self.failed
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct DiagnosticCounts {
    pub(crate) total: u64,
    pub(crate) info: u64,
    pub(crate) warning: u64,
    pub(crate) error: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct InputFreshnessView {
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) path: Option<String>,
    pub(crate) expected_revision: Option<String>,
    pub(crate) observed_revision: Option<String>,
    pub(crate) dirty: Option<bool>,
    pub(crate) status: String,
    pub(crate) reason: Option<String>,
}

#[cfg(test)]
impl From<InputFreshnessWire> for InputFreshnessView {
    fn from(wire: InputFreshnessWire) -> Self {
        Self {
            name: fallback(wire.name, "<unknown>"),
            kind: fallback(wire.kind, "unknown"),
            path: wire.path,
            expected_revision: wire.expected_revision,
            observed_revision: wire.observed_revision,
            dirty: wire.dirty,
            status: fallback(wire.status, "unknown"),
            reason: wire.reason,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PackageView {
    pub(crate) id: String,
    pub(crate) display_id: String,
    pub(crate) title: String,
    pub(crate) specification: FacetView,
    pub(crate) implementation: FacetView,
    pub(crate) verification: FacetView,
    pub(crate) dependencies: Vec<String>,
    pub(crate) acceptance_complete: u64,
    pub(crate) acceptance_total: u64,
    pub(crate) acceptance_checks: Vec<AcceptanceCheckView>,
    pub(crate) blockers: Vec<BlockerView>,
    pub(crate) evidence: Vec<EvidenceView>,
    pub(crate) extensions: BTreeMap<String, Value>,
}

#[cfg(test)]
impl From<PackageWire> for PackageView {
    fn from(wire: PackageWire) -> Self {
        let acceptance_total = wire.acceptance_checks.len() as u64;
        let acceptance_complete = wire
            .acceptance_checks
            .iter()
            .filter(|check| check.category == "complete")
            .count() as u64;
        let id = fallback(wire.id, "<unknown>");
        Self {
            title: wire.title.unwrap_or_else(|| id.clone()),
            display_id: sanitize_text(&id),
            id,
            specification: wire.specification.into(),
            implementation: wire.implementation.into(),
            verification: wire.verification.into(),
            dependencies: wire.dependencies,
            acceptance_complete,
            acceptance_total,
            acceptance_checks: wire.acceptance_checks.into_iter().map(Into::into).collect(),
            blockers: wire.blockers.into_iter().map(Into::into).collect(),
            evidence: wire.evidence.into_iter().map(Into::into).collect(),
            extensions: wire.extensions,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FacetView {
    pub(crate) state: String,
    pub(crate) category: String,
    pub(crate) summary: Option<String>,
    pub(crate) source: Option<SourceView>,
    pub(crate) digest: Option<String>,
}

#[cfg(test)]
impl From<FacetWire> for FacetView {
    fn from(wire: FacetWire) -> Self {
        Self {
            state: fallback(wire.state, "unknown"),
            category: fallback(wire.category, "unknown"),
            summary: wire.summary,
            source: wire.source.map(Into::into),
            digest: wire.digest,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BlockerView {
    pub(crate) code: String,
    pub(crate) display_code: String,
    pub(crate) message: String,
    pub(crate) related_work_package: Option<String>,
    pub(crate) display_related_work_package: Option<String>,
    pub(crate) source: Option<SourceView>,
}

#[cfg(test)]
impl From<BlockerWire> for BlockerView {
    fn from(wire: BlockerWire) -> Self {
        let code = fallback(wire.code, "unknown");
        let related_work_package = wire.related_work_package;
        Self {
            display_code: sanitize_text(&code),
            code,
            message: fallback(wire.message, "No blocker message was provided."),
            display_related_work_package: related_work_package.as_deref().map(sanitize_text),
            related_work_package,
            source: wire.source.map(Into::into),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct BlockerAnchor {
    package_id: String,
    code: String,
    related_work_package: Option<String>,
    source_path: Option<String>,
}

impl BlockerAnchor {
    fn new(package_id: &str, blocker: &BlockerView) -> Self {
        Self {
            package_id: package_id.to_owned(),
            code: blocker.code.clone(),
            related_work_package: blocker.related_work_package.clone(),
            source_path: blocker.source.as_ref().map(|source| source.path.clone()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BlockerKey {
    anchor: BlockerAnchor,
    occurrence: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct BlockerItemView {
    pub(crate) key: BlockerKey,
    pub(crate) display_package_id: String,
    pub(crate) package_title: String,
    pub(crate) specification: FacetView,
    pub(crate) implementation: FacetView,
    pub(crate) verification: FacetView,
    pub(crate) blocker: BlockerView,
}

#[derive(Clone, Debug)]
pub(crate) struct EvidenceView {
    pub(crate) kind: String,
    pub(crate) reference: String,
    pub(crate) source: Option<SourceView>,
    pub(crate) digest: Option<String>,
}

#[cfg(test)]
impl From<EvidenceWire> for EvidenceView {
    fn from(wire: EvidenceWire) -> Self {
        Self {
            kind: fallback(wire.kind, "unknown"),
            reference: fallback(wire.reference, "<unknown>"),
            source: wire.source.map(Into::into),
            digest: wire.digest,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DiagnosticView {
    pub(crate) code: String,
    pub(crate) level: String,
    pub(crate) message: String,
    pub(crate) work_package: Option<String>,
    pub(crate) source: Option<SourceView>,
}

#[cfg(test)]
impl From<DiagnosticWire> for DiagnosticView {
    fn from(wire: DiagnosticWire) -> Self {
        Self {
            code: fallback(wire.code, "unknown"),
            level: fallback(wire.level, "unknown"),
            message: fallback(wire.message, "No diagnostic message was provided."),
            work_package: wire.work_package,
            source: wire.source.map(Into::into),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SourceView {
    pub(crate) path: String,
    pub(crate) display_path: String,
    pub(crate) line: Option<u64>,
    pub(crate) column: Option<u64>,
}

impl SourceView {
    pub(crate) fn display(&self) -> String {
        match (self.line, self.column) {
            (Some(line), Some(column)) => format!("{}:{line}:{column}", self.display_path),
            (Some(line), None) => format!("{}:{line}", self.display_path),
            _ => self.display_path.clone(),
        }
    }
}

#[cfg(test)]
impl From<SourceWire> for SourceView {
    fn from(wire: SourceWire) -> Self {
        let path = fallback(wire.path, "<unknown>");
        Self {
            display_path: sanitize_text(&path),
            path,
            line: wire.line,
            column: wire.column,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ProviderErrorView {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) stderr: Option<String>,
    pub(crate) stderr_truncated: bool,
}

#[cfg(test)]
impl From<ProviderErrorWire> for ProviderErrorView {
    fn from(wire: ProviderErrorWire) -> Self {
        Self {
            code: fallback(wire.code, "unknown"),
            message: fallback(wire.message, "Provider failed without a message."),
            stderr: wire.stderr,
            stderr_truncated: wire.stderr_truncated,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CollectionErrorView {
    pub(crate) scope: String,
    pub(crate) code: String,
    pub(crate) message: String,
}

#[cfg(test)]
impl From<CollectionErrorWire> for CollectionErrorView {
    fn from(wire: CollectionErrorWire) -> Self {
        Self {
            scope: fallback(wire.scope, "unknown"),
            code: fallback(wire.code, "unknown"),
            message: fallback(wire.message, "Collection failed without a message."),
        }
    }
}
