//! Reset-aware collective quota forecast for a cohort of Codex subscription accounts.
//!
//! This answers one question: at the estimated aggregate pace, with work transferable
//! between the participating accounts, does the modeled pool reach a period in which no
//! account can accept work before quota becomes available again?
//!
//! It is a subscription-quota forecast, not a promise that a running session migrates or
//! that provider, model, credit, or concurrency limits are satisfied. The estimator is the
//! same window-average pace the per-account projection uses, so it inherits that basis:
//! usage divided by elapsed window time, not a measured recent series.

use jig_tui::format_countdown;

use crate::usage::WindowRole;

mod cohort;
mod simulation;

pub(super) use cohort::forecast;

/// Bucket identity that enables the fleet forecast. Other providers and generic buckets
/// deliberately keep the existing per-account presentation only.
pub(crate) const CODEX_SUBSCRIPTION_BUCKET: &str = "codex";

/// One account window's full allowance in the reported percentage units.
const FULL_QUOTA_PERCENT: f64 = 100.0;

/// Remaining quota at or below this percentage cannot serve work. It absorbs the rounding
/// residue of charging a window exactly to depletion.
const DEPLETED_PERCENT: f64 = 1e-9;

/// Upper bound on simulated work, counted as window visits rather than events, so a very
/// large discovered-home list cannot make the forecast expensive. Hostile or absurd
/// metadata returns a stated budget limit instead of running a long or unbounded loop.
const WORK_BUDGET: usize = 1_000_000;

/// Why a discovered home is not part of the forecast cohort.
///
/// Declaration order is the display order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum FleetExclusion {
    Loading,
    InspectionUnavailable,
    InspectionError,
    UsageError,
    SignedOut,
    NotAuthenticated,
    NoSubscriptionQuota,
    IncompleteWindowData,
    DuplicateWindowDurations,
    UnknownCapacityClass,
    UnknownIdentity,
    SharedQuotaPool,
    ConflictingSamples,
}

impl FleetExclusion {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Loading => "still loading",
            Self::InspectionUnavailable => "inspection unavailable",
            Self::InspectionError => "inspection error",
            Self::UsageError => "usage error",
            Self::SignedOut => "signed out",
            Self::NotAuthenticated => "not authenticated",
            Self::NoSubscriptionQuota => "no Codex subscription quota",
            Self::IncompleteWindowData => "incomplete window data",
            Self::DuplicateWindowDurations => "ambiguous window durations",
            Self::UnknownCapacityClass => "unknown plan capacity",
            Self::UnknownIdentity => "unknown account identity",
            Self::SharedQuotaPool => "shared quota pool",
            Self::ConflictingSamples => "conflicting samples for one account",
        }
    }
}

/// Why the cohort cannot be forecast at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FleetUnsupported {
    /// No home supplied comparable Codex subscription quota.
    NoEligibleAccount,
    /// Pooling across different quota capacities needs comparable capacity weights, which
    /// the normalized inspection payload does not supply.
    MixedCapacityClasses,
    /// The cohort reported different window layouts, so the quota dimensions cannot be
    /// matched without inventing a missing constraint.
    MixedWindowSchemas,
    /// Carrying an older sample to the common forecast origin exceeded its bounded event
    /// budget.
    AlignmentBudget,
    /// The event or arithmetic budget was reached before the horizon.
    ForecastBudget,
}

impl FleetUnsupported {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::NoEligibleAccount => "no comparable Codex quota data",
            Self::MixedCapacityClasses => "mixed capacities need comparable quota weights",
            Self::MixedWindowSchemas => "mixed window layouts cannot be pooled",
            Self::AlignmentBudget => "sample alignment budget reached before the forecast origin",
            Self::ForecastBudget => "forecast budget reached before the horizon",
        }
    }
}

/// The modeled collective result.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FleetOutcome {
    /// Account usage is still arriving, or pace evidence for at least one participating
    /// window is still warming up. Never a substitute for zero burn.
    Collecting,
    /// No modeled gap through the finite horizon.
    NoGap {
        /// Whether any participating window reported nonzero usage. All-zero usage means
        /// no observed burn, not proven sustainability.
        burn_observed: bool,
    },
    /// Every account is already unable to serve work at the forecast origin.
    BlockedNow {
        recovers_at: Option<u64>,
        limiting: Vec<WindowRole>,
    },
    /// The scenario reaches a period in which no account can serve work.
    GapRisk {
        gap_at: u64,
        recovers_at: Option<u64>,
        limiting: Vec<WindowRole>,
    },
    Unsupported(FleetUnsupported),
}

/// Cohort coverage, kept separate from the forecast so a partial result can never be
/// presented as a result for every discovered home.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FleetCoverage {
    included: usize,
    homes: usize,
    exclusions: Vec<(FleetExclusion, usize)>,
}

impl FleetCoverage {
    #[cfg(test)]
    pub(crate) fn included(&self) -> usize {
        self.included
    }

    #[cfg(test)]
    pub(crate) fn homes(&self) -> usize {
        self.homes
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.exclusions.is_empty() && self.included == self.homes
    }

    pub(crate) fn exclusion_label(&self) -> Option<String> {
        if self.exclusions.is_empty() {
            return None;
        }
        Some(
            self.exclusions
                .iter()
                .map(|(exclusion, count)| format!("{count} {}", exclusion.label()))
                .collect::<Vec<_>>()
                .join(", "),
        )
    }
}

/// The cached, `now`-independent forecast for one inspection generation.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FleetForecast {
    outcome: FleetOutcome,
    coverage: FleetCoverage,
    origin: u64,
    horizon_at: u64,
    expires_at: Option<u64>,
    capacity_class: Option<String>,
    windows: Vec<WindowRole>,
    /// Whether a participating window is still inside its warmup. Reported alongside an
    /// observed outcome so a blocked pool never implies a trusted consumption estimate.
    pace_collecting: bool,
}

/// A forecast paired with the freshness of the samples it was built from.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FleetAssessment {
    forecast: FleetForecast,
    stale: bool,
}

impl FleetForecast {
    pub(crate) fn assess_at(&self, now: u64) -> FleetAssessment {
        let stale = self.expires_at.is_some_and(|expires_at| now >= expires_at);
        FleetAssessment {
            forecast: self.clone(),
            stale,
        }
    }
}

impl FleetAssessment {
    pub(crate) fn outcome(&self) -> &FleetOutcome {
        &self.forecast.outcome
    }

    pub(crate) fn coverage(&self) -> &FleetCoverage {
        &self.forecast.coverage
    }

    pub(crate) fn is_stale(&self) -> bool {
        self.stale
    }

    /// Compact status text for the picker header.
    pub(crate) fn summary_label_at(&self, now: u64) -> String {
        let forecast = &self.forecast;
        if let FleetOutcome::Unsupported(reason) = forecast.outcome {
            return format!("Fleet unavailable: {}", reason.label());
        }
        let coverage = if forecast.coverage.is_complete() {
            format!(
                "All accounts {}/{}",
                forecast.coverage.included, forecast.coverage.homes
            )
        } else if forecast.coverage.included == 0 {
            "Fleet".to_owned()
        } else {
            format!(
                "Fleet partial {}/{}",
                forecast.coverage.included, forecast.coverage.homes
            )
        };
        if self.stale {
            return format!("{coverage}: fleet forecast stale · reopen to refresh");
        }
        format!("{coverage}: {}", self.outcome_label_at(now))
    }

    /// Outcome text without coverage, used by the detail explanation.
    pub(crate) fn outcome_label_at(&self, now: u64) -> String {
        match &self.forecast.outcome {
            FleetOutcome::Unsupported(reason) => format!("unavailable · {}", reason.label()),
            FleetOutcome::Collecting if self.forecast.coverage.included == 0 => {
                "collecting account usage".into()
            }
            FleetOutcome::Collecting => "collecting pace evidence".into(),
            FleetOutcome::NoGap {
                burn_observed: false,
            } => format!(
                "no burn observed through {}",
                horizon_span(self.forecast.origin, self.forecast.horizon_at)
            ),
            FleetOutcome::NoGap {
                burn_observed: true,
            } => format!(
                "no gap projected through {}",
                horizon_span(self.forecast.origin, self.forecast.horizon_at)
            ),
            FleetOutcome::BlockedNow { recovers_at, .. } => {
                format!("all accounts blocked{}", recovery_suffix(*recovers_at, now))
            }
            FleetOutcome::GapRisk {
                gap_at,
                recovers_at,
                ..
            } => format!(
                "gap risk {}{}",
                eta(*gap_at, now),
                recovery_suffix(*recovers_at, now)
            ),
        }
    }

    /// Label/value explanation lines for the detail pane. Assumptions stay visible so an
    /// equal-capacity, periodic-reset estimate is never read as a provider guarantee.
    pub(crate) fn detail_lines_at(&self, now: u64) -> Vec<(String, String)> {
        let forecast = &self.forecast;
        let mut lines = vec![("Forecast".to_owned(), self.outcome_label_at(now))];
        if self.stale {
            lines.push((
                "Freshness".to_owned(),
                "usage samples aged out · reopen to refresh".to_owned(),
            ));
        }
        lines.push((
            "Accounts".to_owned(),
            match forecast.coverage.exclusion_label() {
                Some(excluded) => format!(
                    "{} of {} discovered homes · {excluded}",
                    forecast.coverage.included, forecast.coverage.homes
                ),
                None => format!(
                    "{} of {} discovered homes",
                    forecast.coverage.included, forecast.coverage.homes
                ),
            },
        ));
        // Without a cohort there is no horizon, capacity class, or reset model to explain,
        // and inventing those lines would imply a forecast that does not exist.
        if forecast.windows.is_empty() {
            return lines;
        }
        lines.push((
            "Quota windows".to_owned(),
            forecast
                .windows
                .iter()
                .map(WindowRole::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        ));
        if let Some(limiting) = self.limiting_label() {
            lines.push(("Limiting windows".to_owned(), limiting));
        }
        // An observed outcome reported while a window is warming must still say that its
        // consumption estimate is not yet trustworthy.
        if forecast.pace_collecting && !matches!(forecast.outcome, FleetOutcome::Collecting) {
            lines.push((
                "Pace evidence".to_owned(),
                "still collecting · a participating window is inside its warmup".to_owned(),
            ));
        }
        lines.push((
            "Scenario".to_owned(),
            "work transferable between accounts · earliest reset used first".to_owned(),
        ));
        lines.push((
            "Rate basis".to_owned(),
            "window-average pace per account window".to_owned(),
        ));
        lines.push((
            "Capacity basis".to_owned(),
            match &forecast.capacity_class {
                Some(plan) => format!("equal quotas assumed across plan {plan}"),
                None => "equal quotas assumed".to_owned(),
            },
        ));
        lines.push((
            "Reset model".to_owned(),
            "periodic approximation from each reported window duration".to_owned(),
        ));
        lines.push((
            "Horizon".to_owned(),
            format!(
                "{} from the newest usage sample",
                horizon_span(forecast.origin, forecast.horizon_at)
            ),
        ));
        lines
    }

    fn limiting_label(&self) -> Option<String> {
        let limiting = match &self.forecast.outcome {
            FleetOutcome::BlockedNow { limiting, .. } | FleetOutcome::GapRisk { limiting, .. } => {
                limiting
            }
            _ => return None,
        };
        if limiting.is_empty() {
            return None;
        }
        Some(
            limiting
                .iter()
                .map(WindowRole::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        )
    }
}

fn horizon_span(origin: u64, horizon_at: u64) -> String {
    format_countdown(horizon_at.saturating_sub(origin))
}

fn recovery_suffix(recovers_at: Option<u64>, now: u64) -> String {
    match recovers_at {
        Some(recovers_at) => format!("; capacity returns {}", eta(recovers_at, now)),
        None => "; capacity return not modeled".to_owned(),
    }
}

/// Countdown to a modeled instant. An instant real time has already passed reads as `now`
/// rather than as a fresh countdown from zero.
fn eta(instant: u64, now: u64) -> String {
    match instant.checked_sub(now).filter(|remaining| *remaining > 0) {
        Some(remaining) => format!("in ~{}", format_countdown(remaining)),
        None => "now".to_owned(),
    }
}

/// One eligible account's modeled quota pool at its own observation time.
#[derive(Clone, Debug)]
pub(super) struct FleetAccount {
    identity: String,
    observed_at: u64,
    windows: Vec<FleetWindow>,
}

/// One quota dimension of one account. `duration` is both the reset period under the
/// periodic approximation and the dimension key used to pool comparable budgets.
#[derive(Clone, Copy, Debug)]
pub(super) struct FleetWindow {
    duration: u64,
    role: WindowRole,
    used_percent: f64,
    rate: f64,
    resets_at: u64,
    /// Whether too little of this window has elapsed for its pace to be evidence. The rate is
    /// still pooled into fleet demand, but it must not spend observed quota during alignment.
    warming: bool,
}
