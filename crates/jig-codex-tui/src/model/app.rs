use std::{cell::Cell, collections::HashSet};

use jig_tui::{PreparedFuzzyText, sanitize_text};

use super::{Details, ExitState, Focus, HomeRow, Inspection, unix_timestamp_now};
use crate::{Home, HomeUpdate};

#[derive(Clone, Debug)]
pub(crate) struct App {
    pub(crate) subscription_buckets: Vec<String>,
    pub(crate) configuration_title: Option<String>,
    pub(crate) static_configuration: bool,
    pub(crate) rows: Vec<HomeRow>,
    pub(crate) selected: Option<usize>,
    pub(crate) filter: String,
    pub(crate) searching: bool,
    pub(crate) focus: Focus,
    pub(crate) detail_scroll: u16,
    detail_scroll_limit: Cell<u16>,
    list_offset: Cell<usize>,
    list_viewport_height: Cell<u16>,
    pub(crate) completed: usize,
    pub(crate) inspection_finished: bool,
    pub(crate) inspection_error: Option<String>,
    inspection_error_messages: HashSet<String>,
    pub(crate) discovery_warnings: Vec<String>,
    pub(crate) tick: usize,
    pub(crate) exit_state: Option<ExitState>,
}

impl App {
    pub(crate) fn new(homes: Vec<Home>, discovery_warnings: Vec<String>) -> Self {
        let selected = homes
            .iter()
            .position(|home| home.current)
            .or((!homes.is_empty()).then_some(0));
        Self {
            subscription_buckets: vec!["codex".into(), "claude".into()],
            configuration_title: None,
            static_configuration: false,
            rows: homes.into_iter().map(HomeRow::new).collect(),
            selected,
            filter: String::new(),
            searching: false,
            focus: Focus::Homes,
            detail_scroll: 0,
            detail_scroll_limit: Cell::new(0),
            list_offset: Cell::new(0),
            list_viewport_height: Cell::new(0),
            completed: 0,
            inspection_finished: false,
            inspection_error: None,
            inspection_error_messages: HashSet::new(),
            discovery_warnings: discovery_warnings
                .into_iter()
                .map(|warning| sanitize_text(&warning))
                .collect(),
            tick: 0,
            exit_state: None,
        }
    }

    pub(crate) fn visible_indices(&self) -> Vec<usize> {
        if self.filter.is_empty() {
            return (0..self.rows.len()).collect();
        }
        let filter = PreparedFuzzyText::new(&self.filter);
        let mut matches = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| row.match_score(&filter).map(|score| (score, index)))
            .collect::<Vec<_>>();
        matches.sort_by_key(|(score, index)| (*score, *index));
        matches.into_iter().map(|(_, index)| index).collect()
    }

    pub(crate) fn selected_row(&self) -> Option<&HomeRow> {
        self.selected.and_then(|index| self.rows.get(index))
    }

    #[cfg(test)]
    pub(crate) fn selected_path(&self) -> Option<std::path::PathBuf> {
        self.selected_row().map(|row| row.home.path.clone())
    }

    pub(crate) fn best_projection_index_at(&self, now: u64) -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        for index in self.visible_indices() {
            let row = &self.rows[index];
            let Some(recommendation) = row.usage_snapshot_assessment_at(now).recommendation()
            else {
                continue;
            };
            if best.is_none_or(|(_, best_score)| recommendation.score > best_score) {
                best = Some((index, recommendation.score));
            }
        }
        best.map(|(index, _)| index)
    }

    pub(crate) fn apply_update(&mut self, update: HomeUpdate) {
        self.apply_update_at(update, unix_timestamp_now());
    }

    pub(crate) fn apply_update_at(&mut self, update: HomeUpdate, observed_at: u64) {
        let Some(row) = self.rows.get_mut(update.index) else {
            self.record_inspection_error(&format!(
                "inspection returned unknown home index {}",
                update.index
            ));
            return;
        };
        if !matches!(row.inspection(), Inspection::Ready(_)) {
            self.completed += 1;
        }
        row.set_inspection(Inspection::Ready(Details::from_value(
            update.details,
            observed_at,
            &self.subscription_buckets,
        )));
        self.reconcile_selection();
    }

    pub(crate) fn finish_inspection(&mut self, error: Option<String>) {
        self.inspection_finished = true;
        if let Some(error) = error {
            self.record_inspection_error(&error);
        }
        for row in &mut self.rows {
            if matches!(row.inspection(), Inspection::Loading) {
                row.set_inspection(Inspection::Unavailable);
            }
        }
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let visible = self.visible_indices();
        if visible.is_empty() {
            self.selected = None;
            return;
        }
        let position = self
            .selected
            .and_then(|selected| visible.iter().position(|index| *index == selected))
            .unwrap_or(0);
        let next = position.saturating_add_signed(delta).min(visible.len() - 1);
        let selected = Some(visible[next]);
        if self.selected != selected {
            self.detail_scroll = 0;
        }
        self.selected = selected;
    }

    pub(crate) fn move_to_edge(&mut self, end: bool) {
        let visible = self.visible_indices();
        let selected = if end {
            visible.last().copied()
        } else {
            visible.first().copied()
        };
        if self.selected != selected {
            self.detail_scroll = 0;
        }
        self.selected = selected;
    }

    pub(crate) fn push_filter(&mut self, character: char) {
        if !character.is_control() {
            self.filter.push(character);
            self.reset_list_viewport();
            self.select_best_filter_match();
        }
    }

    pub(crate) fn pop_filter(&mut self) {
        self.filter.pop();
        self.reset_list_viewport();
        self.select_best_filter_match();
    }

    pub(crate) fn clear_filter(&mut self) {
        self.filter.clear();
        self.reset_list_viewport();
        self.reconcile_selection();
    }

    pub(crate) fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Homes => Focus::Details,
            Focus::Details => Focus::Homes,
        };
    }

    pub(crate) fn begin_exit(&mut self, exit_state: ExitState) {
        self.exit_state = Some(exit_state);
    }

    pub(crate) fn scroll_details(&mut self, delta: i16) {
        let max_scroll = self.detail_scroll_limit.get();
        self.detail_scroll = self
            .detail_scroll
            .min(max_scroll)
            .saturating_add_signed(delta)
            .min(max_scroll);
    }

    pub(crate) fn move_details_to_edge(&mut self, end: bool) {
        self.detail_scroll = if end {
            self.detail_scroll_limit.get()
        } else {
            0
        };
    }

    pub(crate) fn set_detail_scroll_limit(&self, max_scroll: u16) {
        self.detail_scroll_limit.set(max_scroll);
    }

    pub(crate) fn list_offset_for_viewport(&self, height: u16) -> usize {
        if self.list_viewport_height.replace(height) != height {
            self.list_offset.set(0);
        }
        self.list_offset.get()
    }

    pub(crate) fn set_list_offset(&self, offset: usize) {
        self.list_offset.set(offset);
    }

    fn reset_list_viewport(&self) {
        self.list_offset.set(0);
    }

    fn record_inspection_error(&mut self, error: &str) {
        let error = sanitize_text(error);
        if !self.inspection_error_messages.insert(error.clone()) {
            return;
        }
        match &mut self.inspection_error {
            Some(existing) => {
                existing.push_str("; ");
                existing.push_str(&error);
            }
            None => self.inspection_error = Some(error),
        }
    }

    fn reconcile_selection(&mut self) {
        let visible = self.visible_indices();
        if !self
            .selected
            .is_some_and(|selected| visible.contains(&selected))
        {
            let selected = visible.first().copied();
            if self.selected != selected {
                self.reset_list_viewport();
            }
            self.selected = selected;
            self.detail_scroll = 0;
        }
    }

    fn select_best_filter_match(&mut self) {
        if self.filter.is_empty() {
            self.reconcile_selection();
            return;
        }
        let selected = self.visible_indices().first().copied();
        if self.selected != selected {
            self.detail_scroll = 0;
            self.reset_list_viewport();
        }
        self.selected = selected;
    }
}
