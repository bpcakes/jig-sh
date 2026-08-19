use std::{
    cell::Cell,
    collections::HashSet,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use jig_tui::{FuzzyMatchScore, PreparedFuzzyText, format_percent, sanitize_text};
use serde_json::Value;

use crate::{Home, HomeUpdate};

// agentic-loc-exception: state projection and picker behavior remain co-located while focused projection types live in model/projection.rs.

mod projection;

pub(crate) use projection::{Projection, UsageSnapshotAssessment};
use projection::{UsageSnapshotFreshness, WindowProjection};

const UNKNOWN: &str = "-";
const MIN_PROJECTION_ELAPSED_FRACTION: f64 = 0.1;
const STALE_PROJECTION_AFTER_SECONDS: u64 = 15 * 60;

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

    pub(crate) fn selected_path(&self) -> Option<PathBuf> {
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
struct SearchTerm {
    priority: usize,
    text: PreparedFuzzyText,
}

impl SearchTerm {
    fn new(priority: usize, text: &str) -> Self {
        Self {
            priority,
            text: PreparedFuzzyText::new(text),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct HomeRow {
    home: Home,
    display_name: String,
    display_path: String,
    inspection: Inspection,
    search_terms: Vec<SearchTerm>,
}

impl HomeRow {
    fn new(home: Home) -> Self {
        let display_name = sanitize_text(&home.name);
        let display_path = sanitize_text(&home.path.to_string_lossy());
        let inspection = Inspection::Loading;
        let search_terms = Self::prepare_search_terms(&display_name, &display_path, &inspection);
        Self {
            home,
            display_name,
            display_path,
            inspection,
            search_terms,
        }
    }

    pub(crate) fn is_current(&self) -> bool {
        self.home.current
    }

    pub(crate) fn display_name(&self) -> &str {
        &self.display_name
    }

    pub(crate) fn display_path(&self) -> &str {
        &self.display_path
    }

    pub(crate) fn inspection(&self) -> &Inspection {
        &self.inspection
    }

    fn set_inspection(&mut self, inspection: Inspection) {
        let search_terms =
            Self::prepare_search_terms(&self.display_name, &self.display_path, &inspection);
        self.inspection = inspection;
        self.search_terms = search_terms;
    }

    fn prepare_search_terms(
        display_name: &str,
        display_path: &str,
        inspection: &Inspection,
    ) -> Vec<SearchTerm> {
        let mut terms = Vec::with_capacity(5);
        terms.push(SearchTerm::new(0, display_name));
        if let Inspection::Ready(details) = inspection {
            terms.extend([
                SearchTerm::new(1, details.account_label()),
                SearchTerm::new(2, &details.plan),
                SearchTerm::new(3, &details.status),
            ]);
        }
        terms.push(SearchTerm::new(4, display_path));
        terms
    }

    fn match_score(&self, query: &PreparedFuzzyText) -> Option<(usize, FuzzyMatchScore)> {
        self.search_terms
            .iter()
            .filter_map(|term| {
                term.text
                    .match_score(query)
                    .map(|score| (term.priority, score))
            })
            .min()
    }

    pub(crate) fn account(&self) -> String {
        match &self.inspection {
            Inspection::Loading => "loading…".into(),
            Inspection::Unavailable => "unavailable".into(),
            Inspection::Ready(details) => details.account_label().to_owned(),
        }
    }

    pub(crate) fn usage(&self) -> String {
        match &self.inspection {
            Inspection::Loading => "loading…".into(),
            Inspection::Unavailable => "unavailable".into(),
            Inspection::Ready(details) => details.usage_summary(),
        }
    }

    #[cfg(test)]
    pub(crate) fn projection(&self) -> Projection {
        match &self.inspection {
            Inspection::Loading => Projection::Loading,
            Inspection::Unavailable => Projection::InspectionUnavailable,
            Inspection::Ready(details) => details.projection(),
        }
    }

    pub(crate) fn usage_snapshot_assessment_at(&self, now: u64) -> UsageSnapshotAssessment {
        match &self.inspection {
            Inspection::Ready(details) => details.usage_snapshot_assessment_at(now),
            Inspection::Loading => UsageSnapshotAssessment::at(
                Projection::Loading,
                UsageSnapshotFreshness::NotSampled,
                false,
            ),
            Inspection::Unavailable => UsageSnapshotAssessment::at(
                Projection::InspectionUnavailable,
                UsageSnapshotFreshness::NotSampled,
                false,
            ),
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
    observed_at: u64,
}

impl Details {
    fn from_value(mut value: Value, observed_at: u64) -> Self {
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
            observed_at,
        }
    }

    pub(crate) fn account_label(&self) -> &str {
        if self.email != UNKNOWN {
            &self.email
        } else {
            &self.account_type
        }
    }

    pub(crate) fn usage_summary(&self) -> String {
        if let Some(error) = &self.inspection_error {
            return format!("error: {error}");
        }
        if let Some(error) = &self.usage_error {
            return format!("error: {error}");
        }
        let Some(bucket) = self.primary_bucket() else {
            return if self.status == "not logged in" {
                "not signed in".into()
            } else {
                "unavailable".into()
            };
        };
        bucket.summary()
    }

    fn projection(&self) -> Projection {
        if let Some(projection) = self.blocking_projection() {
            return projection;
        }
        let Some(bucket) = self.primary_bucket() else {
            return Projection::Unavailable;
        };
        bucket.projection_at(self.observed_at)
    }

    fn usage_snapshot_assessment_at(&self, now: u64) -> UsageSnapshotAssessment {
        let primary_bucket = self.primary_bucket();
        let expires_at = primary_bucket.map_or_else(
            || {
                self.observed_at
                    .saturating_add(STALE_PROJECTION_AFTER_SECONDS)
            },
            |bucket| bucket.projection_expires_at(self.observed_at),
        );
        let has_presented_usage_sample = self.inspection_error.is_none()
            && self.usage_error.is_none()
            && self.status != "not logged in"
            && primary_bucket.is_some_and(RateLimitBucket::has_usage_sample);
        let freshness = if has_presented_usage_sample {
            UsageSnapshotFreshness::sampled_at(now, expires_at)
        } else {
            UsageSnapshotFreshness::NotSampled
        };
        UsageSnapshotAssessment::at(
            self.projection(),
            freshness,
            primary_bucket.is_some_and(|bucket| bucket.id == "codex"),
        )
    }

    fn primary_bucket(&self) -> Option<&RateLimitBucket> {
        self.buckets
            .iter()
            .find(|bucket| bucket.id == "codex")
            .or_else(|| self.buckets.first())
    }

    fn blocking_projection(&self) -> Option<Projection> {
        if self.inspection_error.is_some() {
            return Some(Projection::InspectionError);
        }
        if self.usage_error.is_some() {
            return Some(Projection::UsageError);
        }
        if self.status == "not logged in" {
            return Some(Projection::SignedOut);
        }
        if self.status != "authenticated" {
            return Some(Projection::Unavailable);
        }
        None
    }

    pub(crate) fn window_usage_snapshot_assessment_at(
        &self,
        bucket: &RateLimitBucket,
        index: usize,
        now: u64,
    ) -> UsageSnapshotAssessment {
        let expires_at = bucket
            .windows
            .get(index)
            .and_then(|window| window.resets_at.and_then(|reset| u64::try_from(reset).ok()))
            .map_or_else(
                || {
                    self.observed_at
                        .saturating_add(STALE_PROJECTION_AFTER_SECONDS)
                },
                |reset| {
                    reset.min(
                        self.observed_at
                            .saturating_add(STALE_PROJECTION_AFTER_SECONDS),
                    )
                },
            );
        let freshness = bucket
            .windows
            .get(index)
            .filter(|window| window.has_usage_sample())
            .map_or(UsageSnapshotFreshness::NotSampled, |_| {
                UsageSnapshotFreshness::sampled_at(now, expires_at)
            });
        UsageSnapshotAssessment::at(
            bucket.window_projection_at(index, self.observed_at),
            freshness,
            false,
        )
    }

    pub(crate) fn sample_age_label_at(&self, now: u64) -> String {
        let age = now.saturating_sub(self.observed_at);
        if age < 60 {
            "just now".into()
        } else if age < 3_600 {
            format!("{}m ago", age / 60)
        } else if age < 86_400 {
            format!("{}h ago", age / 3_600)
        } else {
            format!("{}d ago", age / 86_400)
        }
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
            [only] if self.id == "codex" => format!(
                "{} {}",
                only.codex_role()
                    .map(str::to_owned)
                    .unwrap_or_else(|| format_duration(only.duration_minutes)),
                only.remaining()
            ),
            [only] => self.generic_summary(std::slice::from_ref(only)),
            [first, second, ..] if self.id == "codex" => [first, second]
                .into_iter()
                .map(|window| match window.codex_role() {
                    Some(role) => format!("{role} {}", window.remaining()),
                    None => format!(
                        "{} {}",
                        format_duration(window.duration_minutes),
                        window.remaining()
                    ),
                })
                .collect::<Vec<_>>()
                .join(", "),
            windows => self.generic_summary(windows),
        }
    }

    fn generic_summary(&self, windows: &[RateLimitWindow]) -> String {
        let summary = windows
            .iter()
            .map(|window| {
                format!(
                    "{} {}",
                    format_duration(window.duration_minutes),
                    window.remaining()
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        if self.label() == UNKNOWN {
            summary
        } else {
            format!("{} {summary}", self.label())
        }
    }

    fn projection_at(&self, now: u64) -> Projection {
        if self.windows.is_empty() {
            return Projection::Unavailable;
        }

        let mut worst: Option<Projection> = None;
        let mut collecting: Option<(&'static str, f64)> = None;
        let mut incomplete = false;
        for (index, window) in self.windows.iter().enumerate() {
            let role = self.window_role(index);
            match window.projection_at(now) {
                WindowProjection::Unavailable => incomplete = true,
                WindowProjection::Collecting { remaining_percent } => {
                    incomplete = true;
                    if collecting
                        .is_none_or(|(_, current_remaining)| remaining_percent < current_remaining)
                    {
                        collecting = Some((role, remaining_percent));
                    }
                }
                projection => {
                    let Some((candidate, score)) =
                        Projection::from_scored_window(role, projection, false)
                    else {
                        continue;
                    };
                    if worst
                        .as_ref()
                        .and_then(Projection::severity_score)
                        .is_none_or(|worst_score| score < worst_score)
                    {
                        worst = Some(candidate);
                    }
                }
            }
        }

        worst
            .map(|projection| projection.with_partial(incomplete))
            .or_else(|| {
                collecting.map(|(role, remaining_percent)| Projection::Collecting {
                    role,
                    remaining_percent,
                })
            })
            .unwrap_or(Projection::Unavailable)
    }

    pub(crate) fn window_role(&self, index: usize) -> &'static str {
        if self.id != "codex" {
            return "window";
        }
        self.windows
            .get(index)
            .and_then(RateLimitWindow::codex_role)
            .unwrap_or("window")
    }

    fn projection_expires_at(&self, observed_at: u64) -> u64 {
        self.windows
            .iter()
            .filter(|window| {
                !matches!(
                    window.projection_at(observed_at),
                    WindowProjection::Unavailable
                )
            })
            .filter_map(|window| window.resets_at.and_then(|reset| u64::try_from(reset).ok()))
            .fold(
                observed_at.saturating_add(STALE_PROJECTION_AFTER_SECONDS),
                u64::min,
            )
    }

    fn has_usage_sample(&self) -> bool {
        self.windows.iter().any(RateLimitWindow::has_usage_sample)
    }

    pub(crate) fn window_projection_at(&self, index: usize, now: u64) -> Projection {
        let role = self.window_role(index);
        match self
            .windows
            .get(index)
            .map(|window| window.projection_at(now))
        {
            None => Projection::Unavailable,
            Some(projection) => Projection::from_window(role, projection, false),
        }
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

    pub(crate) fn remaining(&self) -> String {
        self.valid_used_percent()
            .map(|used| format!("{} left", format_percent((100.0 - used).max(0.0))))
            .unwrap_or_else(|| "remaining unavailable".into())
    }

    pub(crate) fn usage_detail(&self) -> String {
        let Some(used) = self.valid_used_percent() else {
            return format!(
                "usage unavailable · {} window",
                format_duration(self.duration_minutes)
            );
        };
        format!(
            "{} used · {} left · {} window",
            format_percent(used),
            format_percent((100.0 - used).max(0.0)),
            format_duration(self.duration_minutes)
        )
    }

    pub(crate) fn reset_label_at(&self, now: u64) -> String {
        let Some(timestamp) = self.resets_at.and_then(|value| u64::try_from(value).ok()) else {
            return "reset unknown".into();
        };
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

    fn projection_at(&self, now: u64) -> WindowProjection {
        let Some(used) = self.valid_used_percent() else {
            return WindowProjection::Unavailable;
        };
        if used >= 100.0 {
            return WindowProjection::Exhausted;
        }
        let Some(duration) = self
            .duration_minutes
            .filter(|duration| *duration > 0)
            .and_then(|duration| duration.checked_mul(60))
        else {
            return WindowProjection::Unavailable;
        };
        let Some(reset) = self.resets_at.and_then(|reset| u64::try_from(reset).ok()) else {
            return WindowProjection::Unavailable;
        };
        let Some(start) = reset.checked_sub(duration) else {
            return WindowProjection::Unavailable;
        };
        let Some(elapsed) = now.checked_sub(start).filter(|elapsed| *elapsed < duration) else {
            return WindowProjection::Unavailable;
        };
        let elapsed_fraction = elapsed as f64 / duration as f64;
        // Zero measured usage is immediately actionable: regardless of how
        // young the window is, it has the full quota headroom the picker is
        // ranking for. Nonzero burn rates still wait for the warmup threshold.
        if used == 0.0 {
            return WindowProjection::Remaining { percent: 100.0 };
        }
        if elapsed_fraction < MIN_PROJECTION_ELAPSED_FRACTION {
            return WindowProjection::Collecting {
                remaining_percent: 100.0 - used,
            };
        }

        let projected_used = used / elapsed_fraction;
        let score = 100.0 - projected_used;
        if score >= 0.0 {
            WindowProjection::Remaining { percent: score }
        } else {
            let exhaustion_fraction = elapsed_fraction * (100.0 / used);
            let seconds = ((1.0 - exhaustion_fraction) * duration as f64)
                .max(0.0)
                .round() as u64;
            WindowProjection::ExhaustsEarly { seconds, score }
        }
    }

    fn codex_role(&self) -> Option<&'static str> {
        match self.duration_minutes {
            Some(300) => Some("5h"),
            Some(10_080) => Some("weekly"),
            _ => None,
        }
    }

    fn valid_used_percent(&self) -> Option<f64> {
        self.used_percent
            .filter(|used| used.is_finite() && *used >= 0.0)
    }

    fn has_usage_sample(&self) -> bool {
        self.valid_used_percent().is_some()
    }
}

pub(crate) fn unix_timestamp_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
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
