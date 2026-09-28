use crate::dashboard::{RECORDER_SCHEMA_VERSION, RecorderRefresh, RecorderSnapshot};

use super::*;

#[derive(Debug)]
pub(crate) struct DomainState<T> {
    pub(crate) data: Option<T>,
    pub(crate) error: Option<String>,
    pub(crate) refreshing: bool,
}

impl<T> Default for DomainState<T> {
    fn default() -> Self {
        Self {
            data: None,
            error: None,
            refreshing: false,
        }
    }
}

pub(crate) struct App {
    pub(crate) status: Option<Dashboard>,
    pub(crate) recorder: DomainState<LocalDashboard>,
    pub(crate) tab: Tab,
    pub(crate) timeline_index: usize,
    pub(crate) timeline_filter: TimelineFilter,
    pub(crate) health_index: usize,
    pub(crate) detail: DetailState,
    pub(crate) runtime_notice: Option<String>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            status: None,
            recorder: DomainState::default(),
            tab: Tab::Status,
            timeline_index: 0,
            timeline_filter: TimelineFilter::All,
            health_index: 0,
            detail: DetailState::default(),
            runtime_notice: None,
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

    pub(crate) fn accept_recorder_refresh(&mut self, refresh: RecorderRefresh) -> bool {
        let RecorderRefresh {
            recorder,
            status_local,
        } = refresh;
        let published_local = self.accept_recorder_snapshot(recorder);
        if published_local {
            self.status = Some(status_local.into());
        }
        published_local
    }

    fn accept_recorder_snapshot(&mut self, snapshot: RecorderSnapshot) -> bool {
        if snapshot.schema_version != RECORDER_SCHEMA_VERSION {
            self.accept_error(format!(
                "unsupported recorder snapshot schema version {}; this TUI supports version {RECORDER_SCHEMA_VERSION}",
                snapshot.schema_version
            ));
            return false;
        }
        let timeline_id = self.selected_timeline().map(|row| row.identity.clone());
        let health_id = self.selected_health().map(|row| row.identity.clone());
        let dashboard = LocalDashboard::from(snapshot);
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
        self.recorder.data = Some(dashboard);
        self.recorder.error = None;
        self.clamp_local_selections();
        true
    }

    pub(crate) fn accept_error(&mut self, error: String) {
        self.recorder.error = Some(sanitize_text(&error));
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

    pub(crate) fn move_selection(&mut self, delta: isize) {
        match self.tab {
            Tab::Status => {}
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
            Tab::Timeline => {
                self.timeline_index = edge_index(self.timeline_len(), end);
            }
            Tab::Health => {
                self.health_index = edge_index(self.health_len(), end);
            }
        }
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
        let document = match self.tab {
            Tab::Timeline => self.selected_timeline().map(|row| row.detail.clone()),
            Tab::Health => self.selected_health().map(|row| row.detail.clone()),
            Tab::Status => None,
        };
        let (Some(document), Some(local)) = (document, self.recorder.data.as_ref()) else {
            return false;
        };
        self.detail
            .open_document(document, local.epoch_id, local.generated_at_ms);
        true
    }

    pub(crate) fn detail_is_open(&self) -> bool {
        self.detail.is_open()
    }

    pub(crate) fn close_detail(&mut self) {
        self.detail = DetailState::default();
    }

    pub(crate) fn scroll_detail(&mut self, delta: isize) {
        self.detail.scroll = moved_scroll(self.detail.scroll, delta, self.detail.scroll_limit());
    }

    pub(crate) fn scroll_detail_horizontal(&mut self, delta: isize) {
        self.detail.horizontal_scroll = moved_scroll(
            self.detail.horizontal_scroll,
            delta,
            self.detail.horizontal_limit(),
        );
    }

    pub(crate) fn move_detail_to_edge(&mut self, end: bool) {
        self.detail.scroll = if end { self.detail.scroll_limit() } else { 0 };
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
        self.timeline_index = self
            .timeline_index
            .min(self.timeline_len().saturating_sub(1));
        self.health_index = self.health_index.min(self.health_len().saturating_sub(1));
    }

    pub(crate) fn local_domain(&self) -> DomainRef<'_> {
        DomainRef {
            error: self.recorder.error.as_deref(),
            refreshing: self.recorder.refreshing,
        }
    }

    pub(crate) fn domain_has_data(&self, tab: Tab) -> bool {
        if tab == Tab::Status {
            self.status.is_some()
        } else {
            self.recorder.data.is_some()
        }
    }

    pub(crate) fn set_local_refreshing(&mut self, refreshing: bool) {
        self.recorder.refreshing = refreshing;
    }
}

pub(crate) struct DomainRef<'a> {
    pub(crate) error: Option<&'a str>,
    pub(crate) refreshing: bool,
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
