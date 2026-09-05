#[cfg(test)]
use serde_json::Value;

use crate::dashboard::{
    RECORDER_SCHEMA_VERSION, RecorderRefresh, RecorderSnapshot, StatusRefresh, StatusSnapshot,
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
    pub(crate) recorder: DomainState<RecorderSnapshot>,
    pub(crate) tab: Tab,
    pub(crate) provider_index: usize,
    pub(crate) package_index: usize,
    pub(crate) blocker_index: usize,
    pub(crate) blocked_only: bool,
    pub(crate) package_detail: PackageDetailState,
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
            blocked_only: false,
            package_detail: PackageDetailState::default(),
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

    pub(crate) fn accept_status_refresh(&mut self, refresh: StatusRefresh) {
        let StatusRefresh { status, recorder } = refresh;
        if self.accept_recorder_snapshot(recorder) {
            self.recorder.refresh_queued = false;
        }
        self.accept_status_snapshot(status);
    }

    pub(crate) fn accept_recorder_refresh(&mut self, refresh: RecorderRefresh) {
        let RecorderRefresh {
            recorder,
            status_local,
        } = refresh;
        if self.accept_recorder_snapshot(recorder)
            && let Some(dashboard) = &mut self.status.data
        {
            dashboard.apply_local_snapshot(status_local);
        }
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
        self.recorder.data = Some(snapshot);
        self.recorder.error = None;
        true
    }

    pub(crate) fn accept_status_snapshot(&mut self, snapshot: StatusSnapshot) {
        let dashboard = match Dashboard::from_snapshot(snapshot) {
            Ok(dashboard) => dashboard,
            Err(error) => {
                self.accept_error(Tab::Status, error);
                return;
            }
        };
        self.replace_dashboard(dashboard);
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
            Tab::Status | Tab::Work | Tab::Timeline | Tab::Health => {}
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
        }
    }

    pub(crate) fn move_to_edge(&mut self, end: bool) {
        match self.tab {
            Tab::Status | Tab::Work | Tab::Timeline | Tab::Health => {}
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
    Recorder(&'a mut DomainState<RecorderSnapshot>),
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

    pub(crate) fn set_refresh_queued(&mut self, queued: bool) {
        match &mut self.state {
            DomainMutInner::Status(state) => state.refresh_queued = queued,
            DomainMutInner::Recorder(state) => state.refresh_queued = queued,
        }
    }
}
