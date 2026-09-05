use crate::dashboard::{
    Decision, PlanSnapshot, PlanSnapshotResult, RECORDER_SCHEMA_VERSION, Receipt, RecorderEpochId,
};
use unicode_width::UnicodeWidthStr;

use super::{
    DetailDocument, GateSetView, LimitView, LocalErrorView, TextView, format_duration,
    format_timestamp, sanitize_text,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum PlanSection {
    #[default]
    Summary,
    Body,
    Gates,
    Decisions,
    Receipts,
}

impl PlanSection {
    pub(crate) const ALL: [Self; 5] = [
        Self::Summary,
        Self::Body,
        Self::Gates,
        Self::Decisions,
        Self::Receipts,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Summary => "Summary",
            Self::Body => "Body",
            Self::Gates => "Gates",
            Self::Decisions => "Decisions",
            Self::Receipts => "Receipts",
        }
    }

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Summary => 0,
            Self::Body => 1,
            Self::Gates => 2,
            Self::Decisions => 3,
            Self::Receipts => 4,
        }
    }

    pub(crate) fn cycle(self, backwards: bool) -> Self {
        let index = self.index();
        let next = if backwards {
            (index + Self::ALL.len() - 1) % Self::ALL.len()
        } else {
            (index + 1) % Self::ALL.len()
        };
        Self::ALL[next]
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PlanDetailView {
    pub(crate) raw_plan_id: String,
    pub(crate) display_plan_id: String,
    pub(crate) title: String,
    pub(crate) state: String,
    pub(crate) is_open: bool,
    pub(crate) basis_epoch: u64,
    pub(crate) summary: DetailDocument,
    pub(crate) body: Option<DetailDocument>,
    pub(crate) gates_document: Option<DetailDocument>,
    pub(crate) decisions: Vec<DecisionDetailView>,
    pub(crate) receipts: Vec<ReceiptDetailView>,
    pub(crate) decisions_limit: LimitView,
    pub(crate) receipts_limit: LimitView,
    pub(crate) errors: Vec<LocalErrorView>,
}

impl TryFrom<PlanSnapshot> for PlanDetailView {
    type Error = String;

    fn try_from(snapshot: PlanSnapshot) -> Result<Self, Self::Error> {
        if snapshot.schema_version != RECORDER_SCHEMA_VERSION {
            return Err(format!(
                "unsupported plan snapshot schema version {}; this TUI supports version {RECORDER_SCHEMA_VERSION}",
                snapshot.schema_version
            ));
        }
        let plan = snapshot.plan;
        let mut summary = vec![
            format!("Plan: {}", sanitize_text(&plan.plan_id)),
            format!("Title: {}", sanitize_text(&plan.title)),
            format!("State: {}", sanitize_text(&plan.state)),
            format!("Opened: {}", format_timestamp(plan.opened_at_ms)),
            format!("Closed: {}", format_timestamp(plan.closed_at_ms)),
            format!("Duration: {}", format_duration(plan.duration_ms)),
        ];
        for (label, value) in [
            ("Resolution", plan.resolution.as_deref()),
            ("Baseline ref", plan.baseline_ref.as_deref()),
            ("Baseline OID", plan.baseline_oid.as_deref()),
            ("Baseline error", plan.baseline_error.as_deref()),
        ] {
            if let Some(value) = value {
                summary.push(format!("{label}: {}", sanitize_text(value)));
            }
        }
        summary.extend([
            format!(
                "Generated: {}",
                format_timestamp(Some(snapshot.generated_at_ms))
            ),
            format!(
                "Detail observed: {}",
                format_timestamp(Some(snapshot.detail_observed_at_ms))
            ),
            format!(
                "Gates observed: {}",
                format_timestamp(Some(snapshot.gates_observed_at_ms))
            ),
            format!(
                "Decisions observed: {}",
                format_timestamp(Some(snapshot.decisions_observed_at_ms))
            ),
        ]);
        let gates = snapshot.gates.map(GateSetView::from);
        let gates_document = gates.as_ref().map(gate_document);
        Ok(Self {
            display_plan_id: sanitize_text(&plan.plan_id),
            raw_plan_id: plan.plan_id,
            title: sanitize_text(&plan.title),
            is_open: plan.state == "open",
            state: sanitize_text(&plan.state),
            basis_epoch: snapshot.basis_epoch.get(),
            summary: DetailDocument::new("Plan summary", summary),
            body: snapshot.body.map(text_document),
            gates_document,
            decisions: snapshot.decisions.into_iter().map(Into::into).collect(),
            receipts: snapshot.receipts.into_iter().map(Into::into).collect(),
            decisions_limit: snapshot.limits.plan_decisions.into(),
            receipts_limit: snapshot.limits.plan_receipts.into(),
            errors: snapshot.errors.into_iter().map(Into::into).collect(),
        })
    }
}

fn text_document(text: crate::dashboard::BoundedText) -> DetailDocument {
    let text: TextView = text.into();
    let mut lines = text.lines;
    lines.push(text.limit.label("characters"));
    DetailDocument::new("Plan body", lines)
}

fn gate_document(gates: &GateSetView) -> DetailDocument {
    let mut lines = vec![format!(
        "Overall: {} · {}",
        gates.overall,
        gates.limit.label("gates")
    )];
    for gate in &gates.gates {
        lines.extend([
            format!(
                "{} [{}] {} · required {}",
                gate.id, gate.status, gate.subject, gate.required
            ),
            format!(
                "  Freshness: {} · ended {} · diff {}",
                gate.freshness, gate.ended_at, gate.diff_summary
            ),
            format!("  {}", gate.changed_limit.label("changed paths")),
            format!("  {}", gate.matching_limit.label("matching paths")),
            format!("  {}", gate.findings_limit.label("findings")),
        ]);
        lines.extend(
            gate.changed_paths
                .iter()
                .map(|path| format!("    changed {path}")),
        );
        lines.extend(
            gate.matching_paths
                .iter()
                .map(|path| format!("    matched {path}")),
        );
        lines.extend(
            gate.findings
                .iter()
                .map(|finding| format!("    {}", finding.line)),
        );
        if let Some(remediation) = &gate.remediation {
            lines.push(format!("  Recovery: {}", remediation.display));
            lines.push(format!("  Argv: {}", remediation.inert_argv));
        }
    }
    DetailDocument::new("Gate detail", lines)
}

#[derive(Clone, Debug)]
pub(crate) struct DecisionDetailView {
    pub(crate) raw_id: String,
    pub(crate) display_id: String,
    pub(crate) title: String,
    pub(crate) selected: String,
    pub(crate) document: DetailDocument,
}

impl From<Decision> for DecisionDetailView {
    fn from(decision: Decision) -> Self {
        Self {
            raw_id: decision.id.clone(),
            display_id: sanitize_text(&decision.id),
            title: sanitize_text(&decision.title),
            selected: sanitize_text(&decision.selected_option),
            document: decision_document(&decision),
        }
    }
}

fn decision_document(decision: &Decision) -> DetailDocument {
    let rationale: TextView = decision.rationale.clone().into();
    let mut lines = vec![
        format!("Decision: {}", sanitize_text(&decision.id)),
        format!("Title: {}", sanitize_text(&decision.title)),
        format!("Selected: {}", sanitize_text(&decision.selected_option)),
        format!(
            "Observed: {}",
            format_timestamp(Some(decision.timestamp_ms))
        ),
    ];
    for (label, value) in [
        ("Plan", decision.plan_id.as_deref()),
        ("Session", decision.session_id.as_deref()),
    ] {
        if let Some(value) = value {
            lines.push(format!("{label}: {}", sanitize_text(value)));
        }
    }
    lines.push(format!(
        "Alternatives: {}",
        decision
            .alternatives
            .iter()
            .map(|value| sanitize_text(value))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    append_preview(&mut lines, "Rationale", &rationale);
    DetailDocument::new("Decision detail", lines)
}

#[derive(Clone, Debug)]
pub(crate) struct ReceiptDetailView {
    pub(crate) raw_id: String,
    pub(crate) display_id: String,
    pub(crate) tool: String,
    pub(crate) status: String,
    pub(crate) document: DetailDocument,
}

impl From<Receipt> for ReceiptDetailView {
    fn from(receipt: Receipt) -> Self {
        let changed_limit = LimitView::from_rows(&receipt.changed_paths);
        let stdout: TextView = receipt.stdout_preview.into();
        let stderr: TextView = receipt.stderr_preview.into();
        let mut lines = vec![
            format!("Receipt: {}", sanitize_text(&receipt.id)),
            format!("Tool: {}", sanitize_text(&receipt.tool_name)),
            format!("Exit: {}", receipt.exit_status),
            format!("Started: {}", format_timestamp(receipt.started_at_ms)),
            format!("Ended: {}", format_timestamp(receipt.ended_at_ms)),
            format!("Duration: {}", format_duration(receipt.duration_ms)),
        ];
        for (label, value) in [
            ("Command key", receipt.invoked_command_key.as_deref()),
            ("Plan", receipt.plan_id.as_deref()),
            ("Session", receipt.session_id.as_deref()),
            ("Diff", receipt.diff_summary.as_deref()),
        ] {
            if let Some(value) = value {
                lines.push(format!("{label}: {}", sanitize_text(value)));
            }
        }
        lines.push("Changed paths:".to_string());
        lines.extend(
            receipt
                .changed_paths
                .items()
                .iter()
                .map(|path| format!("  {}", sanitize_text(path))),
        );
        lines.push(changed_limit.label("paths"));
        append_preview(&mut lines, "Stdout", &stdout);
        append_preview(&mut lines, "Stderr", &stderr);
        Self {
            display_id: sanitize_text(&receipt.id),
            raw_id: receipt.id,
            tool: sanitize_text(&receipt.tool_name),
            status: if receipt.exit_status == 0 {
                "pass".to_string()
            } else {
                format!("exit {}", receipt.exit_status)
            },
            document: DetailDocument::new("Receipt detail", lines),
        }
    }
}

fn append_preview(lines: &mut Vec<String>, label: &str, preview: &TextView) {
    lines.push(format!("{label}:"));
    lines.extend(preview.lines.iter().cloned());
    lines.push(preview.limit.label("characters"));
}

#[derive(Clone, Debug)]
pub(crate) enum BaseDetail {
    Document(DetailDocument),
    Plan(Box<PlanDetailView>),
}

impl DetailDocument {
    fn line_count(&self) -> usize {
        1 + self.lines.len()
    }
}

impl PlanDetailView {
    fn section_line_count(&self, section: PlanSection) -> usize {
        let content = match section {
            PlanSection::Summary => self.summary.line_count(),
            PlanSection::Body => self.body.as_ref().map_or(1, DetailDocument::line_count),
            PlanSection::Gates => self
                .gates_document
                .as_ref()
                .map_or(1, DetailDocument::line_count),
            PlanSection::Decisions => self.decisions.len(),
            PlanSection::Receipts => self.receipts.len(),
        };
        content + self.errors.len()
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DetailState {
    pub(crate) base: Option<BaseDetail>,
    pub(crate) leaf: Option<DetailDocument>,
    pub(crate) leaf_scroll: u16,
    pub(crate) horizontal_scroll: u16,
    pub(crate) section: PlanSection,
    pub(crate) section_scroll: [u16; 5],
    pub(crate) decision_index: usize,
    pub(crate) receipt_index: usize,
    pub(crate) loading_plan: Option<String>,
    pub(crate) loading_plan_basis: Option<crate::dashboard::PlanBasis>,
    pub(crate) target_plan_id: Option<String>,
    pub(crate) item_epoch: Option<RecorderEpochId>,
    pub(crate) item_generated_at_ms: Option<u64>,
    pub(crate) error: Option<String>,
    pub(crate) notice: Option<String>,
}

impl DetailState {
    pub(crate) fn is_open(&self) -> bool {
        self.base.is_some() || self.loading_plan.is_some() || self.error.is_some()
    }

    pub(crate) fn plan(&self) -> Option<&PlanDetailView> {
        match self.base.as_ref()? {
            BaseDetail::Plan(plan) => Some(plan),
            BaseDetail::Document(_) => None,
        }
    }

    pub(crate) fn scroll_limit(&self) -> u16 {
        let lines = if let Some(leaf) = &self.leaf {
            leaf.line_count()
        } else {
            match self.base.as_ref() {
                Some(BaseDetail::Document(document)) => document.line_count(),
                Some(BaseDetail::Plan(plan)) => plan.section_line_count(self.section),
                None => 0,
            }
        };
        u16::try_from(lines.saturating_sub(1)).unwrap_or(u16::MAX)
    }

    pub(crate) fn horizontal_limit(&self) -> u16 {
        let line_width = |document: &DetailDocument| {
            std::iter::once(&document.title)
                .chain(&document.lines)
                .map(|line| UnicodeWidthStr::width(line.as_str()))
                .max()
                .unwrap_or(0)
        };
        let width = if let Some(leaf) = &self.leaf {
            line_width(leaf)
        } else {
            match self.base.as_ref() {
                Some(BaseDetail::Document(document)) => line_width(document),
                Some(BaseDetail::Plan(plan)) => match self.section {
                    PlanSection::Summary => line_width(&plan.summary),
                    PlanSection::Body => plan.body.as_ref().map_or(0, line_width),
                    PlanSection::Gates => plan.gates_document.as_ref().map_or(0, line_width),
                    PlanSection::Decisions | PlanSection::Receipts => 0,
                },
                None => 0,
            }
        };
        u16::try_from(width.saturating_sub(1)).unwrap_or(u16::MAX)
    }

    fn section_scroll_limit(&self, section: PlanSection) -> u16 {
        let lines = match self.base.as_ref() {
            Some(BaseDetail::Document(document)) => document.line_count(),
            Some(BaseDetail::Plan(plan)) => plan.section_line_count(section),
            None => 0,
        };
        u16::try_from(lines.saturating_sub(1)).unwrap_or(u16::MAX)
    }

    fn clamp_scrolls(&mut self) {
        let limits = PlanSection::ALL.map(|section| self.section_scroll_limit(section));
        for (scroll, limit) in self.section_scroll.iter_mut().zip(limits) {
            *scroll = (*scroll).min(limit);
        }
        let leaf_limit = self.leaf.as_ref().map_or(0, |leaf| {
            u16::try_from(leaf.line_count().saturating_sub(1)).unwrap_or(u16::MAX)
        });
        self.leaf_scroll = self.leaf_scroll.min(leaf_limit);
    }

    pub(crate) fn open_document(
        &mut self,
        document: DetailDocument,
        epoch: RecorderEpochId,
        generated_at_ms: u64,
    ) {
        *self = Self {
            base: Some(BaseDetail::Document(document)),
            item_epoch: Some(epoch),
            item_generated_at_ms: Some(generated_at_ms),
            ..Self::default()
        };
    }

    pub(crate) fn request_plan(&mut self, plan_id: String) {
        self.loading_plan = Some(plan_id.clone());
        self.loading_plan_basis = None;
        self.target_plan_id = Some(plan_id);
        self.leaf = None;
        self.error = None;
        self.notice = None;
    }

    pub(crate) fn refresh_plan(&mut self, plan_id: String, basis: crate::dashboard::PlanBasis) {
        self.loading_plan = Some(plan_id.clone());
        self.loading_plan_basis = Some(basis);
        self.target_plan_id = Some(plan_id);
        self.error = None;
        self.notice = None;
    }

    pub(crate) fn accept_plan_result(
        &mut self,
        requested_plan_id: &str,
        result: PlanSnapshotResult,
    ) {
        if self.loading_plan.as_deref() != Some(requested_plan_id) {
            return;
        }
        self.loading_plan = None;
        self.loading_plan_basis = None;
        let leaf_was_open = self.leaf.is_some();
        let decision_id = self.plan().and_then(|plan| {
            plan.decisions
                .get(self.decision_index)
                .map(|decision| decision.raw_id.clone())
        });
        let receipt_id = self.plan().and_then(|plan| {
            plan.receipts
                .get(self.receipt_index)
                .map(|receipt| receipt.raw_id.clone())
        });
        match result {
            PlanSnapshotResult::Found(snapshot) => match PlanDetailView::try_from(*snapshot) {
                Ok(plan) if plan.raw_plan_id == requested_plan_id => {
                    self.base = Some(BaseDetail::Plan(Box::new(plan)));
                    self.leaf = None;
                    self.error = None;
                    let plan = self.plan().expect("accepted plan detail is present");
                    let next_decision_index = decision_id
                        .as_deref()
                        .and_then(|id| {
                            plan.decisions
                                .iter()
                                .position(|decision| decision.raw_id == id)
                        })
                        .unwrap_or(0);
                    let next_receipt_index = receipt_id
                        .as_deref()
                        .and_then(|id| {
                            plan.receipts
                                .iter()
                                .position(|receipt| receipt.raw_id == id)
                        })
                        .unwrap_or(0);
                    self.decision_index = next_decision_index;
                    self.receipt_index = next_receipt_index;
                    if leaf_was_open {
                        self.leaf = self.plan().and_then(|plan| match self.section {
                            PlanSection::Decisions => plan
                                .decisions
                                .get(self.decision_index)
                                .map(|decision| decision.document.clone()),
                            PlanSection::Receipts => plan
                                .receipts
                                .get(self.receipt_index)
                                .map(|receipt| receipt.document.clone()),
                            PlanSection::Summary | PlanSection::Body | PlanSection::Gates => None,
                        });
                    }
                }
                Ok(_) => self.error = Some("plan detail returned a different plan ID".to_string()),
                Err(error) => self.error = Some(error),
            },
            PlanSnapshotResult::NotFound => {
                self.base = None;
                self.leaf = None;
                self.notice = Some(format!(
                    "Plan {} is no longer available.",
                    sanitize_text(requested_plan_id)
                ));
            }
            PlanSnapshotResult::StaleRecorderEpoch => {
                self.error = Some("plan detail basis is stale; refresh and retry".to_string());
            }
        }
        self.clamp_scrolls();
    }
}
