use std::{
    cell::Cell,
    collections::HashSet,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use jig_tui::{FuzzyMatchScore, fuzzy_match_score, sanitize_text};
use serde_json::Value;

use crate::{Home, HomeUpdate};

const UNKNOWN: &str = "-";

#[derive(Clone, Debug)]
pub(crate) struct App {
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
        let filter = self.filter.to_lowercase();
        if filter.is_empty() {
            return (0..self.rows.len()).collect();
        }
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

    pub(crate) fn selected_path(&self) -> Option<PathBuf> {
        self.selected_row().map(|row| row.home.path.clone())
    }

    pub(crate) fn apply_update(&mut self, update: HomeUpdate) {
        let Some(row) = self.rows.get_mut(update.index) else {
            self.record_inspection_error(&format!(
                "inspection returned unknown home index {}",
                update.index
            ));
            return;
        };
        if !matches!(row.inspection, Inspection::Ready(_)) {
            self.completed += 1;
        }
        row.inspection = Inspection::Ready(Details::from_value(update.details));
        self.reconcile_selection();
    }

    pub(crate) fn finish_inspection(&mut self, error: Option<String>) {
        self.inspection_finished = true;
        if let Some(error) = error {
            self.record_inspection_error(&error);
        }
        for row in &mut self.rows {
            if matches!(row.inspection, Inspection::Loading) {
                row.inspection = Inspection::Unavailable;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Focus {
    Homes,
    Details,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExitState {
    Launching,
    Cancelling,
}

#[derive(Clone, Debug)]
pub(crate) struct HomeRow {
    pub(crate) home: Home,
    pub(crate) display_name: String,
    pub(crate) display_path: String,
    pub(crate) inspection: Inspection,
}

impl HomeRow {
    fn new(home: Home) -> Self {
        let display_name = sanitize_text(&home.name);
        let display_path = sanitize_text(&home.path.to_string_lossy());
        Self {
            home,
            display_name,
            display_path,
            inspection: Inspection::Loading,
        }
    }

    fn match_score(&self, needle: &str) -> Option<(usize, FuzzyMatchScore)> {
        let mut matches = Vec::with_capacity(5);
        if let Some(score) = fuzzy_match_score(&self.display_name, needle) {
            matches.push((0, score));
        }
        if let Inspection::Ready(details) = &self.inspection {
            for (priority, field) in [
                (1, details.account_label()),
                (2, details.plan.clone()),
                (3, details.status.clone()),
            ] {
                if let Some(score) = fuzzy_match_score(&field, needle) {
                    matches.push((priority, score));
                }
            }
        }
        if let Some(score) = fuzzy_match_score(&self.display_path, needle) {
            matches.push((4, score));
        }
        matches.into_iter().min()
    }

    pub(crate) fn account(&self) -> String {
        match &self.inspection {
            Inspection::Loading => "loading…".into(),
            Inspection::Unavailable => "unavailable".into(),
            Inspection::Ready(details) => details.account_label(),
        }
    }

    pub(crate) fn plan(&self) -> &str {
        match &self.inspection {
            Inspection::Ready(details) => &details.plan,
            Inspection::Loading | Inspection::Unavailable => UNKNOWN,
        }
    }

    pub(crate) fn usage(&self) -> String {
        match &self.inspection {
            Inspection::Loading => "loading…".into(),
            Inspection::Unavailable => "unavailable".into(),
            Inspection::Ready(details) => details.usage_summary(),
        }
    }

    pub(crate) fn state(&self) -> &str {
        match &self.inspection {
            Inspection::Loading => "loading",
            Inspection::Unavailable => "unavailable",
            Inspection::Ready(details) if details.inspection_error.is_some() => "error",
            Inspection::Ready(details) => &details.status,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Inspection {
    Loading,
    Ready(Details),
    Unavailable,
}

#[derive(Clone, Debug)]
pub(crate) struct Details {
    pub(crate) account_type: String,
    pub(crate) email: String,
    pub(crate) plan: String,
    pub(crate) status: String,
    pub(crate) buckets: Vec<RateLimitBucket>,
    pub(crate) inspection_error: Option<String>,
    pub(crate) usage_error: Option<String>,
}

impl Details {
    fn from_value(mut value: Value) -> Self {
        sanitize_value(&mut value);
        let account = value.get("account").filter(|account| account.is_object());
        let inferred_status = if account.is_some() {
            "authenticated"
        } else if value.get("account").is_some_and(Value::is_null) {
            "not logged in"
        } else {
            "unknown"
        };
        Self {
            account_type: text_at(account, "type"),
            email: text_at(account, "email"),
            plan: text_at(account, "plan_type"),
            status: value
                .get("status")
                .and_then(Value::as_str)
                .filter(|status| !status.is_empty())
                .unwrap_or(inferred_status)
                .to_owned(),
            buckets: value
                .get("rate_limits")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(RateLimitBucket::from_value)
                .collect(),
            inspection_error: optional_text(&value, "inspection_error"),
            usage_error: optional_text(&value, "usage_error"),
        }
    }

    pub(crate) fn account_label(&self) -> String {
        if self.email != UNKNOWN {
            self.email.clone()
        } else {
            self.account_type.clone()
        }
    }

    pub(crate) fn usage_summary(&self) -> String {
        if let Some(error) = &self.inspection_error {
            return format!("error: {error}");
        }
        if let Some(error) = &self.usage_error {
            return format!("error: {error}");
        }
        let Some(bucket) = self
            .buckets
            .iter()
            .find(|bucket| bucket.id == "codex")
            .or_else(|| self.buckets.first())
        else {
            return if self.status == "not logged in" {
                "not signed in".into()
            } else {
                "unavailable".into()
            };
        };
        bucket.summary()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RateLimitBucket {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) plan: String,
    pub(crate) reached: String,
    pub(crate) windows: Vec<RateLimitWindow>,
}

impl RateLimitBucket {
    fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let mut windows = [object.get("primary"), object.get("secondary")]
            .into_iter()
            .flatten()
            .filter_map(RateLimitWindow::from_value)
            .collect::<Vec<_>>();
        windows.sort_by_key(|window| window.duration_minutes.unwrap_or(u64::MAX));
        Some(Self {
            id: value_str(value, "id"),
            name: value_str(value, "name"),
            plan: value_str(value, "plan_type"),
            reached: value_str(value, "reached"),
            windows,
        })
    }

    pub(crate) fn label(&self) -> &str {
        if self.name != UNKNOWN {
            &self.name
        } else {
            &self.id
        }
    }

    pub(crate) fn summary(&self) -> String {
        match self.windows.as_slice() {
            [] => "unavailable".into(),
            [only] if self.id == "codex" => format!("weekly {}", only.compact()),
            [only] => only.compact(),
            [first, second, ..] if self.id == "codex" => [first, second]
                .into_iter()
                .map(|window| match window.codex_role() {
                    Some(role) => format!("{role} {}", window.compact()),
                    None => window.compact(),
                })
                .collect::<Vec<_>>()
                .join(", "),
            windows => windows
                .iter()
                .map(RateLimitWindow::compact)
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    pub(crate) fn window_role(&self, index: usize) -> &'static str {
        if self.id != "codex" {
            return "window";
        }
        if self.windows.len() == 1 {
            return "weekly";
        }
        self.windows
            .get(index)
            .and_then(RateLimitWindow::codex_role)
            .unwrap_or("window")
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RateLimitWindow {
    pub(crate) used_percent: Option<f64>,
    pub(crate) duration_minutes: Option<u64>,
    pub(crate) resets_at: Option<i64>,
}

impl RateLimitWindow {
    fn from_value(value: &Value) -> Option<Self> {
        value.as_object()?;
        Some(Self {
            used_percent: value.get("used_percent").and_then(Value::as_f64),
            duration_minutes: value.get("duration_minutes").and_then(Value::as_u64),
            resets_at: value.get("resets_at").and_then(Value::as_i64),
        })
    }

    pub(crate) fn compact(&self) -> String {
        let used = self
            .used_percent
            .map(|used| {
                if used.fract() == 0.0 {
                    format!("{used:.0}%")
                } else {
                    format!("{used:.1}%")
                }
            })
            .unwrap_or_else(|| "-%".into());
        format!("{used}/{}", format_duration(self.duration_minutes))
    }

    pub(crate) fn reset_label(&self) -> String {
        let Some(timestamp) = self.resets_at.and_then(|value| u64::try_from(value).ok()) else {
            return "reset unknown".into();
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let Some(remaining) = timestamp
            .checked_sub(now)
            .filter(|remaining| *remaining > 0)
        else {
            return "reset due".into();
        };
        if remaining < 3_600 {
            format!("resets in {}m", remaining / 60)
        } else if remaining < 86_400 {
            format!("resets in {}h", remaining / 3_600)
        } else {
            format!("resets in {}d", remaining / 86_400)
        }
    }

    fn codex_role(&self) -> Option<&'static str> {
        match self.duration_minutes {
            Some(300) => Some("5h"),
            Some(10_080) => Some("weekly"),
            _ => None,
        }
    }
}

fn text_at(value: Option<&Value>, key: &str) -> String {
    value
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(UNKNOWN)
        .to_owned()
}

fn value_str(value: &Value, key: &str) -> String {
    text_at(Some(value), key)
}

fn optional_text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn format_duration(minutes: Option<u64>) -> String {
    match minutes {
        Some(minutes) if minutes > 0 && minutes % 1_440 == 0 => {
            format!("{}d", minutes / 1_440)
        }
        Some(minutes) if minutes > 0 && minutes % 60 == 0 => format!("{}h", minutes / 60),
        Some(minutes) => format!("{minutes}m"),
        None => "?".into(),
    }
}

fn sanitize_value(value: &mut Value) {
    match value {
        Value::String(text) => *text = sanitize_text(text),
        Value::Array(values) => values.iter_mut().for_each(sanitize_value),
        Value::Object(values) => values.values_mut().for_each(sanitize_value),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
