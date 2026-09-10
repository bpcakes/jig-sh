use std::time::{SystemTime, UNIX_EPOCH};

use jig_tui::{
    FuzzyMatchScore, PreparedFuzzyText, RankedFuzzyText, best_ranked_fuzzy_match, format_countdown,
    format_percent, sanitize_text,
};
use serde_json::Value;

use crate::Home;

mod app;
mod configuration;
mod projection;

pub(crate) use crate::usage::WindowRole;
use crate::usage::{self, remaining_percent};
pub(crate) use app::App;
pub(crate) use projection::{Projection, UsageSnapshotAssessment};
use projection::{UsageSnapshotFreshness, WindowProjection};

const UNKNOWN: &str = "-";
const MIN_PROJECTION_ELAPSED_FRACTION: f64 = 0.1;
const STALE_PROJECTION_AFTER_SECONDS: u64 = 15 * 60;

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
    pub(crate) configuration_details: Option<Vec<(String, String)>>,
    home: Home,
    display_name: String,
    display_path: String,
    inspection: Inspection,
    search_terms: Vec<RankedFuzzyText>,
}

impl HomeRow {
    fn new(home: Home) -> Self {
        let display_name = sanitize_text(&home.name);
        let display_path = sanitize_text(&home.path.to_string_lossy());
        let inspection = Inspection::Loading;
        let search_terms = Self::prepare_search_terms(&display_name, &display_path, &inspection);
        Self {
            configuration_details: None,
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
    ) -> Vec<RankedFuzzyText> {
        let mut terms = Vec::with_capacity(5);
        terms.push(RankedFuzzyText::new(0, display_name));
        if let Inspection::Ready(details) = inspection {
            terms.extend([
                RankedFuzzyText::new(1, details.account_label()),
                RankedFuzzyText::new(2, &details.plan),
                RankedFuzzyText::new(3, &details.status),
            ]);
        }
        terms.push(RankedFuzzyText::new(4, display_path));
        terms
    }

    fn match_score(&self, query: &PreparedFuzzyText) -> Option<(usize, FuzzyMatchScore)> {
        best_ranked_fuzzy_match(&self.search_terms, query)
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
                UsageSnapshotFreshness::NotSampled,
                false,
            ),
            Inspection::Unavailable => UsageSnapshotAssessment::at(
                Projection::InspectionUnavailable,
                UsageSnapshotFreshness::NotSampled,
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
    fn from_value(mut value: Value, observed_at: u64, subscription_buckets: &[String]) -> Self {
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
                .filter_map(|value| RateLimitBucket::from_value(value, subscription_buckets))
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
        let quota_expires_at = primary_bucket.map_or_else(
            || {
                self.observed_at
                    .saturating_add(STALE_PROJECTION_AFTER_SECONDS)
            },
            |bucket| bucket.quota_expires_at(self.observed_at),
        );
        let projection_expires_at = primary_bucket
            .and_then(|bucket| bucket.projection_expires_at(self.observed_at))
            .unwrap_or(quota_expires_at);
        let has_presented_usage_sample = self.inspection_error.is_none()
            && self.usage_error.is_none()
            && self.status != "not logged in"
            && primary_bucket.is_some_and(RateLimitBucket::has_usage_sample);
        let quota_freshness = if has_presented_usage_sample {
            UsageSnapshotFreshness::sampled_at(now, quota_expires_at)
        } else {
            UsageSnapshotFreshness::NotSampled
        };
        let projection_freshness = if has_presented_usage_sample {
            UsageSnapshotFreshness::sampled_at(now, projection_expires_at)
        } else {
            UsageSnapshotFreshness::NotSampled
        };
        UsageSnapshotAssessment::at(
            self.projection(),
            quota_freshness,
            projection_freshness,
            primary_bucket.is_some_and(|bucket| bucket.subscription),
        )
    }

    fn primary_bucket(&self) -> Option<&RateLimitBucket> {
        self.buckets
            .iter()
            .find(|bucket| bucket.subscription)
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
            freshness,
            false,
        )
    }

    pub(crate) fn usage_sample_age_label_at(&self, now: u64) -> Option<String> {
        if !self.usage_snapshot_assessment_at(now).has_quota_sample() {
            return None;
        }
        let age = now.saturating_sub(self.observed_at);
        if age < 60 {
            Some("just now".into())
        } else {
            Some(format!("{} ago", format_countdown(age)))
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RateLimitBucket {
    subscription: bool,
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) plan: String,
    pub(crate) reached: String,
    pub(crate) windows: Vec<RateLimitWindow>,
}

impl RateLimitBucket {
    fn from_value(value: &Value, subscription_buckets: &[String]) -> Option<Self> {
        let object = value.as_object()?;
        let mut windows = [object.get("primary"), object.get("secondary")]
            .into_iter()
            .flatten()
            .filter_map(RateLimitWindow::from_value)
            .collect::<Vec<_>>();
        windows.sort_by_key(|window| window.duration_minutes.unwrap_or(u64::MAX));
        Some(Self {
            subscription: value
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| subscription_buckets.iter().any(|candidate| candidate == id)),
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
            [only] if self.subscription => format!(
                "{} {}",
                only.subscription_role()
                    .map(|role| role.to_string())
                    .unwrap_or_else(|| format_duration(only.duration_minutes)),
                only.remaining()
            ),
            [only] => self.generic_summary(std::slice::from_ref(only)),
            [first, second, ..] if self.subscription => [first, second]
                .into_iter()
                .map(|window| match window.subscription_role() {
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
        let mut collecting: Option<(WindowRole, f64)> = None;
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

    pub(crate) fn window_role(&self, index: usize) -> WindowRole {
        if !self.subscription {
            return WindowRole::Window;
        }
        self.windows
            .get(index)
            .and_then(RateLimitWindow::subscription_role)
            .unwrap_or(WindowRole::Window)
    }

    fn quota_expires_at(&self, observed_at: u64) -> u64 {
        let expiry_cap = observed_at.saturating_add(STALE_PROJECTION_AFTER_SECONDS);
        self.windows
            .iter()
            .filter(|window| window.has_usage_sample())
            .filter_map(|window| window.resets_at.and_then(|reset| u64::try_from(reset).ok()))
            .fold(expiry_cap, u64::min)
    }

    fn projection_expires_at(&self, observed_at: u64) -> Option<u64> {
        let expiry_cap = observed_at.saturating_add(STALE_PROJECTION_AFTER_SECONDS);
        self.windows
            .iter()
            .filter(|window| {
                !matches!(
                    window.projection_at(observed_at),
                    WindowProjection::Unavailable
                )
            })
            .fold(None, |expires_at, window| {
                let window_expires_at = window
                    .resets_at
                    .and_then(|reset| u64::try_from(reset).ok())
                    .map_or(expiry_cap, |reset| reset.min(expiry_cap));
                Some(expires_at.map_or(window_expires_at, |current: u64| {
                    current.min(window_expires_at)
                }))
            })
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
            .map(|used| format!("{} left", format_percent(remaining_percent(used))))
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
            format_percent(remaining_percent(used)),
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
        format!("resets in {}", format_countdown(remaining))
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

    fn subscription_role(&self) -> Option<WindowRole> {
        WindowRole::for_subscription(self.duration_minutes)
    }

    fn valid_used_percent(&self) -> Option<f64> {
        usage::valid_used_percent(self.used_percent)
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
    minutes
        .map(usage::format_duration)
        .unwrap_or_else(|| "?".into())
}

fn sanitize_value(value: &mut Value) {
    match value {
        Value::String(text) => *text = sanitize_text(text),
        Value::Array(values) => values.iter_mut().for_each(sanitize_value),
        Value::Object(values) => values.values_mut().for_each(sanitize_value),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[cfg(test)]
mod usage_tests;
