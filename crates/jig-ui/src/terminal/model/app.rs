#[cfg(test)]
use serde_json::Value;

use crate::dashboard::{
    PlanBasis, PlanSnapshotResult, RECORDER_SCHEMA_VERSION, RecorderRefresh, RecorderSnapshot,
    StatusRefresh, StatusSnapshot,
};

use super::*;

#[derive(Debug)]
pub(crate) struct DomainState<T> {
    pub(crate) data: Option<T>,
    pub(crate) error: Option<String>,
    pub(crate) refreshing: bool,
    pub(crate) refresh_queued: bool,
}

impl<T> Default for DomainState<T> {
    fn default() -> Self {
        Self {
            data: None,
            error: None,
            refreshing: false,
            refresh_queued: false,
        }
    }
}

#[derive(Debug)]
pub(crate) struct App {
    pub(crate) status: DomainState<Dashboard>,
    pub(crate) recorder: DomainState<LocalDashboard>,
    pub(crate) tab: Tab,
    pub(crate) provider_index: usize,
    pub(crate) package_index: usize,
    pub(crate) blocker_index: usize,
    pub(crate) work_index: usize,
    pub(crate) timeline_index: usize,
    pub(crate) timeline_filter: TimelineFilter,
    pub(crate) health_index: usize,
    pub(crate) blocked_only: bool,
    pub(crate) package_detail: PackageDetailState,
    pub(crate) detail: DetailState,
    pub(crate) runtime_notice: Option<String>,
    pending_plan_request: Option<PendingPlanRequest>,
}

#[derive(Debug)]
struct PendingPlanRequest {
    basis: PlanBasis,
    plan_id: String,
}

impl Default for App {
    fn default() -> Self {
        Self {
            status: DomainState::default(),
            recorder: DomainState::default(),
            tab: Tab::Status,
            provider_index: 0,
            package_index: 0,
            blocker_index: 0,
            work_index: 0,
            timeline_index: 0,
            timeline_filter: TimelineFilter::All,
            health_index: 0,
            blocked_only: false,
            package_detail: PackageDetailState::default(),
            detail: DetailState::default(),
            runtime_notice: None,
            pending_plan_request: None,
        }
    }
}

impl App {
    pub(crate) fn new(tab: Tab) -> Self {
        Self {
            tab,
            ..Self::default()
        }
    }

    pub(crate) fn request_initial_plan(&mut self, plan_id: String) {
        self.detail.request_plan(plan_id);
    }

    pub(crate) fn shrink_timeline_limit(&mut self, timeline_limit: usize) {
        let Some(recorder) = &mut self.recorder.data else {
            return;
        };
        if timeline_limit >= recorder.timeline_limit {
            return;
        }
        let retained = recorder.timeline.len().min(timeline_limit);
        let removed = recorder.timeline.len().saturating_sub(retained);
        recorder.timeline.truncate(retained);
        recorder.timeline_limit = timeline_limit;
        recorder.limits.timeline.applied = timeline_limit;
        recorder.limits.timeline.omitted = recorder
            .limits
            .timeline
            .omitted
            .map(|omitted| omitted.saturating_add(removed));
        self.clamp_local_selections();
    }

    #[cfg(test)]
    pub(crate) fn accept_snapshot(&mut self, value: Value) {
        let dashboard = match Dashboard::from_value(value) {
            Ok(dashboard) => dashboard,
            Err(error) => {
                self.accept_error(Tab::Status, error);
                return;
            }
        };
        self.replace_dashboard(dashboard);
    }

    pub(crate) fn accept_status_refresh(&mut self, refresh: StatusRefresh) -> bool {
        let StatusRefresh { status, recorder } = refresh;
        let published_local = self.accept_recorder_snapshot(recorder);
        if published_local {
            self.recorder.refresh_queued = false;
        }
        self.accept_status_snapshot(status);
        published_local
    }

    pub(crate) fn accept_recorder_refresh(&mut self, refresh: RecorderRefresh) -> bool {
        let RecorderRefresh {
            recorder,
            status_local,
        } = refresh;
        let published_local = self.accept_recorder_snapshot(recorder);
        if published_local && let Some(dashboard) = &mut self.status.data {
            dashboard.apply_local_snapshot(status_local);
        }
        published_local
    }

    fn accept_recorder_snapshot(&mut self, snapshot: RecorderSnapshot) -> bool {
        if snapshot.schema_version != RECORDER_SCHEMA_VERSION {
            self.accept_error(
                Tab::Work,
                format!(
                    "unsupported recorder snapshot schema version {}; this TUI supports version {RECORDER_SCHEMA_VERSION}",
                    snapshot.schema_version
                ),
            );
            return false;
        }
        let work_id = self.selected_work().map(|plan| plan.plan_id.clone());
        let timeline_id = self.selected_timeline().map(|row| row.identity.clone());
        let health_id = self.selected_health().map(|row| row.identity.clone());
        let dashboard = LocalDashboard::from(snapshot);
        self.work_index = work_id
            .as_deref()
            .and_then(|id| dashboard.work.iter().position(|plan| plan.plan_id == id))
            .unwrap_or(0);
        self.timeline_index = timeline_id
            .as_deref()
            .and_then(|id| {
                dashboard
                    .timeline
                    .iter()
                    .filter(|row| self.timeline_filter.matches(row))
                    .position(|row| row.identity == id)
            })
            .unwrap_or(0);
        self.health_index = health_id
            .as_deref()
            .and_then(|id| dashboard.health.iter().position(|row| row.identity == id))
            .unwrap_or(0);
        self.reconcile_plan_detail(&dashboard);
        self.recorder.data = Some(dashboard);
        self.recorder.error = None;
        self.clamp_local_selections();
        true
    }

    pub(crate) fn accept_status_snapshot(&mut self, snapshot: StatusSnapshot) -> bool {
        let dashboard = match Dashboard::from_snapshot(snapshot) {
            Ok(dashboard) => dashboard,
            Err(error) => {
                self.accept_error(Tab::Status, error);
                return false;
            }
        };
        self.replace_dashboard(dashboard);
        true
    }

    fn replace_dashboard(&mut self, dashboard: Dashboard) {
        let provider_id = self.current_provider().map(|provider| provider.id.clone());
        let package_id = self.selected_package().map(|package| package.id.clone());
        let blocker_key = self.selected_blocker().map(|blocker| blocker.key.clone());
        self.status.data = Some(dashboard);
        self.status.error = None;

        let provider_index = provider_id.as_deref().and_then(|id| {
            self.status
                .data
                .as_ref()?
                .providers
                .iter()
                .position(|provider| provider.id == id)
        });
        self.provider_index = provider_index.unwrap_or(0);
        if provider_id.is_some() && provider_index.is_none() {
            self.package_index = 0;
            self.blocker_index = 0;
            self.close_package_detail();
            self.clamp_selections();
            return;
        }
        self.package_index = package_id
            .as_deref()
            .and_then(|id| {
                self.package_rows()
                    .iter()
                    .position(|package| package.id == id)
            })
            .unwrap_or(0);
        self.blocker_index = blocker_key
            .as_ref()
            .and_then(|key| {
                self.current_provider()?
                    .blockers
                    .iter()
                    .position(|blocker| &blocker.key == key)
            })
            .unwrap_or(0);
        self.clamp_selections();
        self.reconcile_package_detail();
    }

    pub(crate) fn accept_error(&mut self, tab: Tab, error: String) {
        self.domain_mut(tab).set_error(Some(sanitize_text(&error)));
        if tab == Tab::Work
            && self.recorder.data.is_none()
            && self.detail.loading_plan_basis.is_none()
            && let Some(plan_id) = self.detail.loading_plan.clone()
        {
            self.accept_plan_error(&plan_id, error);
        }
    }

    pub(crate) fn current_provider(&self) -> Option<&ProviderView> {
        self.status
            .data
            .as_ref()?
            .providers
            .get(self.provider_index)
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
        if tab != Tab::Packages {
            self.close_package_detail();
        }
    }

    pub(crate) fn cycle_tab(&mut self, backwards: bool) {
        let len = Tab::ALL.len();
        let index = if backwards {
            (self.tab.index() + len - 1) % len
        } else {
            (self.tab.index() + 1) % len
        };
        self.tab = Tab::ALL[index];
        if self.tab != Tab::Packages {
            self.close_package_detail();
        }
    }

    pub(crate) fn switch_provider(&mut self, backwards: bool) {
        let len = self
            .status
            .data
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
        self.close_package_detail();
    }

    pub(crate) fn toggle_blocked_only(&mut self) {
        self.blocked_only = !self.blocked_only;
        self.package_index = 0;
        self.close_package_detail();
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        match self.tab {
            Tab::Status => {}
            Tab::Packages => {
                self.package_index =
                    moved_index(self.package_index, self.package_rows().len(), delta);
            }
            Tab::Blockers => {
                let len = self
                    .current_provider()
                    .map(|provider| provider.blockers.len())
                    .unwrap_or(0);
                self.blocker_index = moved_index(self.blocker_index, len, delta);
            }
            Tab::Work => {
                self.work_index = moved_index(self.work_index, self.work_len(), delta);
            }
            Tab::Timeline => {
                self.timeline_index = moved_index(self.timeline_index, self.timeline_len(), delta);
            }
            Tab::Health => {
                self.health_index = moved_index(self.health_index, self.health_len(), delta);
            }
        }
    }

    pub(crate) fn move_to_edge(&mut self, end: bool) {
        match self.tab {
            Tab::Status => {}
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
            Tab::Work => {
                self.work_index = edge_index(self.work_len(), end);
            }
            Tab::Timeline => {
                self.timeline_index = edge_index(self.timeline_len(), end);
            }
            Tab::Health => {
                self.health_index = edge_index(self.health_len(), end);
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
        self.clamp_local_selections();
    }

    pub(crate) fn selected_work(&self) -> Option<&WorkPlanView> {
        self.recorder.data.as_ref()?.work.get(self.work_index)
    }

    pub(crate) fn timeline_rows(&self) -> Vec<&TimelineItemView> {
        self.recorder
            .data
            .as_ref()
            .map(|dashboard| {
                dashboard
                    .timeline
                    .iter()
                    .filter(|row| self.timeline_filter.matches(row))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn selected_timeline(&self) -> Option<&TimelineItemView> {
        self.timeline_rows().get(self.timeline_index).copied()
    }

    pub(crate) fn selected_health(&self) -> Option<&HealthItemView> {
        self.recorder.data.as_ref()?.health.get(self.health_index)
    }

    pub(crate) fn cycle_timeline_filter(&mut self, backwards: bool) {
        let current = TimelineFilter::ALL
            .iter()
            .position(|filter| *filter == self.timeline_filter)
            .unwrap_or(0);
        let len = TimelineFilter::ALL.len();
        let next = if backwards {
            (current + len - 1) % len
        } else {
            (current + 1) % len
        };
        self.timeline_filter = TimelineFilter::ALL[next];
        self.timeline_index = 0;
    }

    pub(crate) fn open_selected_detail(&mut self) -> bool {
        match self.tab {
            Tab::Work => self.selected_work().map(|plan| plan.plan_id.clone()),
            Tab::Timeline => {
                let Some(row) = self.selected_timeline() else {
                    return false;
                };
                if let Some(plan_id) = &row.plan_id {
                    Some(plan_id.clone())
                } else {
                    let document = row.detail.clone();
                    let local = self
                        .recorder
                        .data
                        .as_ref()
                        .expect("selected local row has data");
                    self.detail
                        .open_document(document, local.epoch_id, local.generated_at_ms);
                    return true;
                }
            }
            Tab::Health => {
                let Some(row) = self.selected_health() else {
                    return false;
                };
                let document = row.detail.clone();
                let local = self
                    .recorder
                    .data
                    .as_ref()
                    .expect("selected local row has data");
                self.detail
                    .open_document(document, local.epoch_id, local.generated_at_ms);
                return true;
            }
            Tab::Status | Tab::Packages | Tab::Blockers => return false,
        }
        .is_some_and(|plan_id| {
            self.detail.request_plan(plan_id.clone());
            self.queue_plan_request(plan_id);
            true
        })
    }

    pub(crate) fn take_plan_request(&mut self) -> Option<(PlanBasis, String)> {
        let request = self.pending_plan_request.take()?;
        Some((request.basis, request.plan_id))
    }

    pub(crate) fn accept_plan_result(
        &mut self,
        basis: PlanBasis,
        plan_id: &str,
        result: PlanSnapshotResult,
    ) {
        if let PlanBasis::RecorderEpoch(expected) = basis {
            let current = self.recorder.data.as_ref().map(|local| local.epoch_id);
            if current != Some(expected) {
                if self.pending_plan_request.is_none() {
                    self.accept_plan_error(
                        plan_id,
                        "plan detail response is from a stale recorder epoch".to_string(),
                    );
                }
                return;
            }
            if let PlanSnapshotResult::Found(snapshot) = &result
                && snapshot.basis_epoch != expected
            {
                self.accept_plan_error(
                    plan_id,
                    "plan detail returned a different recorder epoch".to_string(),
                );
                return;
            }
        }
        self.detail.accept_plan_result(plan_id, result);
    }

    pub(crate) fn accept_plan_error(&mut self, plan_id: &str, error: String) {
        if self.detail.loading_plan.as_deref() == Some(plan_id) {
            self.detail.loading_plan = None;
            self.detail.loading_plan_basis = None;
            self.detail.error = Some(sanitize_text(&error));
        }
    }

    pub(crate) fn detail_is_open(&self) -> bool {
        self.detail.is_open()
    }

    pub(crate) fn close_detail(&mut self) {
        if self.detail.leaf.take().is_none() {
            self.detail = DetailState::default();
            self.pending_plan_request = None;
        }
    }

    pub(crate) fn cycle_detail_section(&mut self, backwards: bool) {
        if self.detail.plan().is_some() && self.detail.leaf.is_none() {
            self.detail.section = self.detail.section.cycle(backwards);
            self.detail.horizontal_scroll = 0;
        }
    }

    pub(crate) fn scroll_detail(&mut self, delta: isize) {
        let limit = self.detail.scroll_limit();
        let index = self.detail.section.index();
        self.detail.section_scroll[index] =
            moved_scroll(self.detail.section_scroll[index], delta, limit);
    }

    pub(crate) fn scroll_detail_horizontal(&mut self, delta: isize) {
        self.detail.horizontal_scroll = moved_scroll(
            self.detail.horizontal_scroll,
            delta,
            self.detail.horizontal_limit(),
        );
    }

    pub(crate) fn move_detail_selection(&mut self, delta: isize) {
        if self.detail.leaf.is_some() {
            self.detail.leaf_scroll =
                moved_scroll(self.detail.leaf_scroll, delta, self.detail.scroll_limit());
            return;
        }
        let Some(plan) = self.detail.plan() else {
            self.scroll_detail(delta);
            return;
        };
        match self.detail.section {
            PlanSection::Decisions => {
                self.detail.decision_index =
                    moved_index(self.detail.decision_index, plan.decisions.len(), delta);
            }
            PlanSection::Receipts => {
                self.detail.receipt_index =
                    moved_index(self.detail.receipt_index, plan.receipts.len(), delta);
            }
            PlanSection::Summary | PlanSection::Body | PlanSection::Gates => {
                self.scroll_detail(delta);
            }
        }
    }

    pub(crate) fn open_detail_leaf_or_close(&mut self) {
        if self.detail.leaf.is_some() {
            self.detail.leaf = None;
            return;
        }
        let document = self
            .detail
            .plan()
            .and_then(|plan| match self.detail.section {
                PlanSection::Decisions => plan
                    .decisions
                    .get(self.detail.decision_index)
                    .map(|decision| decision.document.clone()),
                PlanSection::Receipts => plan
                    .receipts
                    .get(self.detail.receipt_index)
                    .map(|receipt| receipt.document.clone()),
                PlanSection::Summary | PlanSection::Body | PlanSection::Gates => None,
            });
        if let Some(document) = document {
            self.detail.leaf = Some(document);
            self.detail.leaf_scroll = 0;
            self.detail.horizontal_scroll = 0;
        } else if !matches!(self.detail.base, Some(BaseDetail::Plan(_))) {
            self.close_detail();
        }
    }

    pub(crate) fn move_detail_to_edge(&mut self, end: bool) {
        if self.detail.leaf.is_some() {
            self.detail.leaf_scroll = if end { self.detail.scroll_limit() } else { 0 };
            return;
        }
        let section = self.detail.section;
        if let Some(plan) = self.detail.plan() {
            match section {
                PlanSection::Decisions => {
                    self.detail.decision_index = edge_index(plan.decisions.len(), end);
                    return;
                }
                PlanSection::Receipts => {
                    self.detail.receipt_index = edge_index(plan.receipts.len(), end);
                    return;
                }
                PlanSection::Summary | PlanSection::Body | PlanSection::Gates => {}
            }
        }
        let index = section.index();
        self.detail.section_scroll[index] = if end { self.detail.scroll_limit() } else { 0 };
    }

    pub(crate) fn refresh_plan_detail(&mut self) -> bool {
        let Some(plan_id) = self
            .detail
            .plan()
            .map(|plan| plan.raw_plan_id.clone())
            .or_else(|| self.detail.target_plan_id.clone())
        else {
            return false;
        };
        let Some(epoch) = self.recorder.data.as_ref().map(|local| local.epoch_id) else {
            return false;
        };
        self.detail
            .refresh_plan(plan_id.clone(), PlanBasis::RecorderEpoch(epoch));
        self.queue_plan_request(plan_id);
        true
    }

    fn queue_plan_request(&mut self, plan_id: String) {
        let Some(local) = &self.recorder.data else {
            return;
        };
        self.detail.loading_plan_basis = Some(PlanBasis::RecorderEpoch(local.epoch_id));
        self.pending_plan_request = Some(PendingPlanRequest {
            basis: PlanBasis::RecorderEpoch(local.epoch_id),
            plan_id,
        });
    }

    fn work_len(&self) -> usize {
        self.recorder
            .data
            .as_ref()
            .map_or(0, |data| data.work.len())
    }

    fn timeline_len(&self) -> usize {
        self.timeline_rows().len()
    }

    fn health_len(&self) -> usize {
        self.recorder
            .data
            .as_ref()
            .map_or(0, |data| data.health.len())
    }

    fn clamp_local_selections(&mut self) {
        self.work_index = self.work_index.min(self.work_len().saturating_sub(1));
        self.timeline_index = self
            .timeline_index
            .min(self.timeline_len().saturating_sub(1));
        self.health_index = self.health_index.min(self.health_len().saturating_sub(1));
    }

    fn reconcile_plan_detail(&mut self, dashboard: &LocalDashboard) {
        let plan_id = if self.detail.loading_plan_basis == Some(PlanBasis::Fresh) {
            None
        } else {
            self.detail.loading_plan.clone().or_else(|| {
                self.detail
                    .plan()
                    .filter(|plan| plan.is_open && plan.basis_epoch != dashboard.epoch_id.get())
                    .map(|plan| plan.raw_plan_id.clone())
            })
        };
        if let Some(plan_id) = plan_id {
            self.detail.refresh_plan(
                plan_id.clone(),
                PlanBasis::RecorderEpoch(dashboard.epoch_id),
            );
            self.pending_plan_request = Some(PendingPlanRequest {
                basis: PlanBasis::RecorderEpoch(dashboard.epoch_id),
                plan_id,
            });
        }
    }

    pub(crate) fn domain(&self, tab: Tab) -> DomainRef<'_> {
        if tab.is_status_domain() {
            DomainRef {
                error: self.status.error.as_deref(),
                refreshing: self.status.refreshing,
                refresh_queued: self.status.refresh_queued,
            }
        } else {
            DomainRef {
                error: self.recorder.error.as_deref(),
                refreshing: self.recorder.refreshing,
                refresh_queued: self.recorder.refresh_queued,
            }
        }
    }

    pub(crate) fn domain_has_data(&self, tab: Tab) -> bool {
        if tab.is_status_domain() {
            self.status.data.is_some()
        } else {
            self.recorder.data.is_some()
        }
    }

    pub(crate) fn domain_mut(&mut self, tab: Tab) -> DomainMut<'_> {
        let state = if tab.is_status_domain() {
            DomainMutInner::Status(&mut self.status)
        } else {
            DomainMutInner::Recorder(&mut self.recorder)
        };
        DomainMut { state }
    }
}

pub(crate) struct DomainRef<'a> {
    pub(crate) error: Option<&'a str>,
    pub(crate) refreshing: bool,
    pub(crate) refresh_queued: bool,
}

enum DomainMutInner<'a> {
    Status(&'a mut DomainState<Dashboard>),
    Recorder(&'a mut DomainState<LocalDashboard>),
}

fn edge_index(len: usize, end: bool) -> usize {
    if end { len.saturating_sub(1) } else { 0 }
}

fn moved_scroll(current: u16, delta: isize, limit: u16) -> u16 {
    let moved = if delta.is_negative() {
        current.saturating_sub(delta.unsigned_abs().min(usize::from(u16::MAX)) as u16)
    } else {
        current.saturating_add(delta.unsigned_abs().min(usize::from(u16::MAX)) as u16)
    };
    moved.min(limit)
}

pub(crate) struct DomainMut<'a> {
    state: DomainMutInner<'a>,
}

impl DomainMut<'_> {
    pub(crate) fn set_error(&mut self, error: Option<String>) {
        match &mut self.state {
            DomainMutInner::Status(state) => state.error = error,
            DomainMutInner::Recorder(state) => state.error = error,
        }
    }

    pub(crate) fn set_refreshing(&mut self, refreshing: bool) {
        match &mut self.state {
            DomainMutInner::Status(state) => state.refreshing = refreshing,
            DomainMutInner::Recorder(state) => state.refreshing = refreshing,
        }
    }
}
