use serde::Deserialize;
use serde_json::Value;

mod wire;

use wire::*;

const STATUS_SCHEMA_VERSION: u64 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Tab {
    Overview,
    Packages,
    Blockers,
}

impl Tab {
    pub(crate) const ALL: [Self; 3] = [Self::Overview, Self::Packages, Self::Blockers];

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Overview => 0,
            Self::Packages => 1,
            Self::Blockers => 2,
        }
    }

    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Overview => "1 Overview",
            Self::Packages => "2 Packages",
            Self::Blockers => "3 Blockers",
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
    pub(crate) providers: Vec<ProviderView>,
    pub(crate) errors: Vec<CollectionErrorView>,
}

impl Dashboard {
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
}

#[derive(Clone, Debug, Default)]
pub(crate) struct LoopView {
    pub(crate) leases: usize,
    pub(crate) attempts: usize,
    pub(crate) exhausted_attempts: usize,
}

impl LoopView {
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
}

#[derive(Clone, Debug)]
pub(crate) struct ProviderView {
    pub(crate) id: String,
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
                package
                    .blockers
                    .iter()
                    .enumerate()
                    .map(move |(index, blocker)| BlockerItemView {
                        key: format!("{}:{}:{index}", package.id, blocker.code),
                        package_id: package.id.clone(),
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

        Self {
            id: fallback(wire.id, "<unknown>"),
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
    pub(crate) title: String,
    pub(crate) specification: FacetView,
    pub(crate) implementation: FacetView,
    pub(crate) verification: FacetView,
    pub(crate) dependencies: Vec<String>,
    pub(crate) acceptance_complete: u64,
    pub(crate) acceptance_total: u64,
    pub(crate) blockers: Vec<BlockerView>,
    pub(crate) evidence: Vec<EvidenceView>,
}

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
            id,
            specification: wire.specification.into(),
            implementation: wire.implementation.into(),
            verification: wire.verification.into(),
            dependencies: wire.dependencies,
            acceptance_complete,
            acceptance_total,
            blockers: wire.blockers.into_iter().map(Into::into).collect(),
            evidence: wire.evidence.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FacetView {
    pub(crate) state: String,
    pub(crate) category: String,
    pub(crate) summary: Option<String>,
    pub(crate) source: Option<SourceView>,
}

impl From<FacetWire> for FacetView {
    fn from(wire: FacetWire) -> Self {
        Self {
            state: fallback(wire.state, "unknown"),
            category: fallback(wire.category, "unknown"),
            summary: wire.summary,
            source: wire.source.map(Into::into),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BlockerView {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) related_work_package: Option<String>,
    pub(crate) source: Option<SourceView>,
}

impl From<BlockerWire> for BlockerView {
    fn from(wire: BlockerWire) -> Self {
        Self {
            code: fallback(wire.code, "unknown"),
            message: fallback(wire.message, "No blocker message was provided."),
            related_work_package: wire.related_work_package,
            source: wire.source.map(Into::into),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BlockerItemView {
    pub(crate) key: String,
    pub(crate) package_id: String,
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
}

impl From<EvidenceWire> for EvidenceView {
    fn from(wire: EvidenceWire) -> Self {
        Self {
            kind: fallback(wire.kind, "unknown"),
            reference: fallback(wire.reference, "<unknown>"),
            source: wire.source.map(Into::into),
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
    pub(crate) line: Option<u64>,
    pub(crate) column: Option<u64>,
}

impl SourceView {
    pub(crate) fn display(&self) -> String {
        match (self.line, self.column) {
            (Some(line), Some(column)) => format!("{}:{line}:{column}", self.path),
            (Some(line), None) => format!("{}:{line}", self.path),
            _ => self.path.clone(),
        }
    }
}

impl From<SourceWire> for SourceView {
    fn from(wire: SourceWire) -> Self {
        Self {
            path: fallback(wire.path, "<unknown>"),
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

impl From<CollectionErrorWire> for CollectionErrorView {
    fn from(wire: CollectionErrorWire) -> Self {
        Self {
            scope: fallback(wire.scope, "unknown"),
            code: fallback(wire.code, "unknown"),
            message: fallback(wire.message, "Collection failed without a message."),
        }
    }
}

#[derive(Debug)]
pub(crate) struct App {
    pub(crate) dashboard: Option<Dashboard>,
    pub(crate) last_error: Option<String>,
    pub(crate) refreshing: bool,
    pub(crate) refresh_queued: bool,
    pub(crate) tab: Tab,
    pub(crate) provider_index: usize,
    pub(crate) package_index: usize,
    pub(crate) blocker_index: usize,
    pub(crate) blocked_only: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            dashboard: None,
            last_error: None,
            refreshing: false,
            refresh_queued: false,
            tab: Tab::Overview,
            provider_index: 0,
            package_index: 0,
            blocker_index: 0,
            blocked_only: false,
        }
    }
}

impl App {
    pub(crate) fn accept_snapshot(&mut self, value: Value) {
        let provider_id = self.current_provider().map(|provider| provider.id.clone());
        let package_id = self.selected_package().map(|package| package.id.clone());
        let blocker_key = self.selected_blocker().map(|blocker| blocker.key.clone());

        let dashboard = match Dashboard::from_value(value) {
            Ok(dashboard) => dashboard,
            Err(error) => {
                self.accept_error(error);
                return;
            }
        };
        self.dashboard = Some(dashboard);
        self.last_error = None;

        self.provider_index = provider_id
            .as_deref()
            .and_then(|id| {
                self.dashboard
                    .as_ref()?
                    .providers
                    .iter()
                    .position(|provider| provider.id == id)
            })
            .unwrap_or(0);
        self.package_index = package_id
            .as_deref()
            .and_then(|id| {
                self.package_rows()
                    .iter()
                    .position(|package| package.id == id)
            })
            .unwrap_or(0);
        self.blocker_index = blocker_key
            .as_deref()
            .and_then(|key| {
                self.current_provider()?
                    .blockers
                    .iter()
                    .position(|blocker| blocker.key == key)
            })
            .unwrap_or(0);
        self.clamp_selections();
    }

    pub(crate) fn accept_error(&mut self, error: String) {
        self.last_error = Some(sanitize_text(&error));
    }

    pub(crate) fn current_provider(&self) -> Option<&ProviderView> {
        self.dashboard.as_ref()?.providers.get(self.provider_index)
    }

    pub(crate) fn package_rows(&self) -> Vec<&PackageView> {
        self.current_provider()
            .map(|provider| {
                provider
                    .packages
                    .iter()
                    .filter(|package| !self.blocked_only || !package.blockers.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn selected_package(&self) -> Option<&PackageView> {
        self.package_rows().get(self.package_index).copied()
    }

    pub(crate) fn selected_blocker(&self) -> Option<&BlockerItemView> {
        self.current_provider()?.blockers.get(self.blocker_index)
    }

    pub(crate) fn select_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }

    pub(crate) fn cycle_tab(&mut self, backwards: bool) {
        let len = Tab::ALL.len();
        let index = if backwards {
            (self.tab.index() + len - 1) % len
        } else {
            (self.tab.index() + 1) % len
        };
        self.tab = Tab::ALL[index];
    }

    pub(crate) fn switch_provider(&mut self, backwards: bool) {
        let len = self
            .dashboard
            .as_ref()
            .map(|dashboard| dashboard.providers.len())
            .unwrap_or(0);
        if len == 0 {
            return;
        }
        self.provider_index = if backwards {
            (self.provider_index + len - 1) % len
        } else {
            (self.provider_index + 1) % len
        };
        self.package_index = 0;
        self.blocker_index = 0;
    }

    pub(crate) fn toggle_blocked_only(&mut self) {
        self.blocked_only = !self.blocked_only;
        self.package_index = 0;
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        match self.tab {
            Tab::Overview => {}
            Tab::Packages => {
                self.package_index =
                    moved_index(self.package_index, self.package_rows().len(), delta)
            }
            Tab::Blockers => {
                let len = self
                    .current_provider()
                    .map(|provider| provider.blockers.len())
                    .unwrap_or(0);
                self.blocker_index = moved_index(self.blocker_index, len, delta);
            }
        }
    }

    pub(crate) fn move_to_edge(&mut self, end: bool) {
        match self.tab {
            Tab::Overview => {}
            Tab::Packages => {
                let len = self.package_rows().len();
                self.package_index = if end { len.saturating_sub(1) } else { 0 };
            }
            Tab::Blockers => {
                let len = self
                    .current_provider()
                    .map(|provider| provider.blockers.len())
                    .unwrap_or(0);
                self.blocker_index = if end { len.saturating_sub(1) } else { 0 };
            }
        }
    }

    fn clamp_selections(&mut self) {
        self.package_index = self
            .package_index
            .min(self.package_rows().len().saturating_sub(1));
        self.blocker_index = self.blocker_index.min(
            self.current_provider()
                .map(|provider| provider.blockers.len())
                .unwrap_or(0)
                .saturating_sub(1),
        );
    }
}

fn moved_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    current
        .saturating_add_signed(delta)
        .min(len.saturating_sub(1))
}

fn fallback(value: String, fallback: &str) -> String {
    nonempty(value).unwrap_or_else(|| fallback.to_owned())
}

fn nonempty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn array_len(value: &Value, key: &str) -> usize {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

fn sanitize_value(value: &mut Value) {
    match value {
        Value::String(text) => *text = sanitize_text(text),
        Value::Array(values) => values.iter_mut().for_each(sanitize_value),
        Value::Object(values) => values.values_mut().for_each(sanitize_value),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn sanitize_text(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect()
}
