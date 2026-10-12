//! Cohort assembly: which discovered homes form a comparable forecast pool, and what quota
//! state each contributes.
//!
//! Eligibility lives here so the engine in `simulation` only ever sees accounts that already
//! share a capacity class and a window layout.

use std::collections::{BTreeMap, BTreeSet};

use super::super::{
    Details, HomeRow, Inspection, MIN_PROJECTION_ELAPSED_FRACTION, RateLimitBucket,
    RateLimitWindow, UNKNOWN,
};
use super::simulation::{Simulation, SimulationOutcome};
use super::{
    CODEX_SUBSCRIPTION_BUCKET, FleetAccount, FleetCoverage, FleetExclusion, FleetForecast,
    FleetOutcome, FleetUnsupported, FleetWindow,
};
use crate::usage::{self, WindowRole};

#[cfg(test)]
mod tests;

struct Candidate {
    account: FleetAccount,
    capacity_class: String,
    expires_at: u64,
    warming: bool,
}

impl Candidate {
    /// Whether two homes reported the same quota state for the same account.
    ///
    /// Compares the observed facts rather than derived rates, which are a function of those
    /// facts and a shared observation time.
    fn describes_same_quota(&self, other: &Self) -> bool {
        self.capacity_class == other.capacity_class
            && self.account.windows.len() == other.account.windows.len()
            && self
                .account
                .windows
                .iter()
                .zip(&other.account.windows)
                .all(|(left, right)| {
                    left.duration == right.duration
                        && left.used_percent == right.used_percent
                        && left.resets_at == right.resets_at
                })
    }
}

/// What one quota pool retained across the homes that reported it.
enum Pool {
    Sample(Candidate),
    /// Homes disagreed about the same account at the same observation time, so no sample can
    /// be called the freshest. `observed_at` is retained so a later sample can still resolve
    /// the disagreement.
    Conflicted {
        observed_at: u64,
    },
}

struct PoolEntry {
    pool: Pool,
    homes: usize,
}

impl PoolEntry {
    fn new(candidate: Candidate) -> Self {
        Self {
            pool: Pool::Sample(candidate),
            homes: 1,
        }
    }

    /// Folds another home's report of the same account into this pool.
    ///
    /// Observation timestamps have one-second resolution, so two homes inspected in the same
    /// second can tie. Retaining whichever home was discovered first would let row order pick
    /// between disagreeing readings, and that choice can change the forecast rather than only
    /// its presentation. A genuine tie is therefore unresolved, not silently decided.
    fn merge(&mut self, candidate: Candidate) {
        self.homes += 1;
        let incoming = candidate.account.observed_at;
        let retained = match &self.pool {
            Pool::Sample(existing) => existing.account.observed_at,
            Pool::Conflicted { observed_at } => *observed_at,
        };
        if incoming > retained {
            self.pool = Pool::Sample(candidate);
            return;
        }
        if incoming < retained {
            return;
        }
        if let Pool::Sample(existing) = &self.pool
            && existing.describes_same_quota(&candidate)
        {
            return;
        }
        self.pool = Pool::Conflicted {
            observed_at: retained,
        };
    }
}

/// Builds the cohort from every discovered row and forecasts it.
///
/// Callers must pass the complete row set, never a filtered view: typing a search must not
/// change the fleet being forecast.
pub(in crate::model) fn forecast(rows: &[HomeRow]) -> FleetForecast {
    let mut exclusions: BTreeMap<FleetExclusion, usize> = BTreeMap::new();
    let mut pools: BTreeMap<String, PoolEntry> = BTreeMap::new();
    for row in rows {
        match candidate(row) {
            Err(exclusion) => *exclusions.entry(exclusion).or_default() += 1,
            // Duplicate supplied identities are conservatively one quota pool. Home paths and
            // display labels are not proof of independence, so a pool is never counted twice.
            Ok(candidate) => match pools.get_mut(&candidate.account.identity) {
                Some(entry) => entry.merge(candidate),
                None => {
                    pools.insert(
                        candidate.account.identity.clone(),
                        PoolEntry::new(candidate),
                    );
                }
            },
        }
    }

    let homes = rows.len();
    let mut candidates = Vec::with_capacity(pools.len());
    for entry in pools.into_values() {
        match entry.pool {
            Pool::Sample(candidate) => {
                if let Some(collapsed) = entry.homes.checked_sub(1).filter(|count| *count > 0) {
                    *exclusions
                        .entry(FleetExclusion::SharedQuotaPool)
                        .or_default() += collapsed;
                }
                candidates.push(candidate);
            }
            // An unresolved disagreement is missing data for that pool, not a reason to guess.
            Pool::Conflicted { .. } => {
                *exclusions
                    .entry(FleetExclusion::ConflictingSamples)
                    .or_default() += entry.homes;
            }
        }
    }
    let still_loading = exclusions.contains_key(&FleetExclusion::Loading);
    let coverage = FleetCoverage {
        included: candidates.len(),
        homes,
        exclusions: exclusions.into_iter().collect(),
    };
    if candidates.is_empty() {
        // Inspection that has not finished yet is unfinished data, not an unsupported
        // cohort.
        if still_loading {
            return collecting(coverage);
        }
        return unsupported(FleetUnsupported::NoEligibleAccount, coverage);
    }

    let capacity_classes = candidates
        .iter()
        .map(|candidate| candidate.capacity_class.as_str())
        .collect::<BTreeSet<_>>();
    if capacity_classes.len() > 1 {
        return unsupported(FleetUnsupported::MixedCapacityClasses, coverage);
    }
    let capacity_class = capacity_classes.into_iter().next().map(str::to_owned);

    let schemas = candidates
        .iter()
        .map(|candidate| {
            candidate
                .account
                .windows
                .iter()
                .map(|window| window.duration)
                .collect::<Vec<_>>()
        })
        .collect::<BTreeSet<_>>();
    if schemas.len() > 1 {
        return unsupported(FleetUnsupported::MixedWindowSchemas, coverage);
    }

    let origin = candidates
        .iter()
        .map(|candidate| candidate.account.observed_at)
        .max()
        .unwrap_or_default();
    let expires_at = candidates
        .iter()
        .map(|candidate| candidate.expires_at)
        .min();
    let warming = candidates.iter().any(|candidate| candidate.warming);
    let windows = candidates
        .first()
        .map(|candidate| {
            candidate
                .account
                .windows
                .iter()
                .map(|window| window.role)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let longest_window = candidates
        .iter()
        .flat_map(|candidate| candidate.account.windows.iter())
        .map(|window| window.duration)
        .max()
        .unwrap_or_default();
    let horizon_at = origin.saturating_add(longest_window);

    let accounts = candidates
        .into_iter()
        .map(|candidate| candidate.account)
        .collect::<Vec<_>>();
    let outcome = match Simulation::new(&accounts, origin, longest_window).run() {
        SimulationOutcome::AlignmentBudgetExceeded => {
            FleetOutcome::Unsupported(FleetUnsupported::AlignmentBudget)
        }
        SimulationOutcome::BudgetExceeded => {
            FleetOutcome::Unsupported(FleetUnsupported::ForecastBudget)
        }
        SimulationOutcome::NoGap { burn_observed } => FleetOutcome::NoGap { burn_observed },
        SimulationOutcome::Gap {
            gap_at,
            recovers_at,
            limiting,
        } if gap_at <= origin => FleetOutcome::BlockedNow {
            recovers_at,
            limiting,
        },
        SimulationOutcome::Gap {
            gap_at,
            recovers_at,
            limiting,
        } => FleetOutcome::GapRisk {
            gap_at,
            recovers_at,
            limiting,
        },
    };
    // A window inside its warmup bounds confidence in future consumption, but it must not
    // erase quota that is already gone. Whether every account is blocked now, and which
    // reported resets end that block, are both independent of the warming pace estimate:
    // blocking follows from observed remaining quota, and no work is served during a gap.
    let outcome = if warming
        && matches!(
            outcome,
            FleetOutcome::NoGap { .. } | FleetOutcome::GapRisk { .. }
        ) {
        FleetOutcome::Collecting
    } else {
        outcome
    };
    FleetForecast {
        outcome,
        coverage,
        origin,
        horizon_at,
        expires_at,
        capacity_class,
        windows,
        pace_collecting: warming,
    }
}

fn unsupported(reason: FleetUnsupported, coverage: FleetCoverage) -> FleetForecast {
    FleetForecast {
        outcome: FleetOutcome::Unsupported(reason),
        coverage,
        origin: 0,
        horizon_at: 0,
        expires_at: None,
        capacity_class: None,
        windows: Vec::new(),
        pace_collecting: false,
    }
}

fn collecting(coverage: FleetCoverage) -> FleetForecast {
    FleetForecast {
        outcome: FleetOutcome::Collecting,
        coverage,
        origin: 0,
        horizon_at: 0,
        expires_at: None,
        capacity_class: None,
        windows: Vec::new(),
        pace_collecting: false,
    }
}

fn candidate(row: &HomeRow) -> Result<Candidate, FleetExclusion> {
    let details = match row.inspection() {
        Inspection::Loading => return Err(FleetExclusion::Loading),
        Inspection::Unavailable => return Err(FleetExclusion::InspectionUnavailable),
        Inspection::Ready(details) => details,
    };
    if details.inspection_error.is_some() {
        return Err(FleetExclusion::InspectionError);
    }
    if details.usage_error.is_some() {
        return Err(FleetExclusion::UsageError);
    }
    if details.status == "not logged in" {
        return Err(FleetExclusion::SignedOut);
    }
    if details.status != "authenticated" {
        return Err(FleetExclusion::NotAuthenticated);
    }
    let bucket = details
        .buckets
        .iter()
        .find(|bucket| bucket.id == CODEX_SUBSCRIPTION_BUCKET)
        .ok_or(FleetExclusion::NoSubscriptionQuota)?;
    if bucket.windows.is_empty() {
        return Err(FleetExclusion::NoSubscriptionQuota);
    }
    if !all_durations_distinct(&bucket.windows) {
        return Err(FleetExclusion::DuplicateWindowDurations);
    }
    let capacity_class =
        capacity_class(details, bucket).ok_or(FleetExclusion::UnknownCapacityClass)?;
    // Identity comes only from supplied account metadata. Home paths are not proof that
    // two directories authenticate different quota pools.
    if details.email == UNKNOWN {
        return Err(FleetExclusion::UnknownIdentity);
    }
    let mut windows = Vec::with_capacity(bucket.windows.len());
    for window in &bucket.windows {
        windows.push(
            fleet_window(window, details.observed_at)
                .ok_or(FleetExclusion::IncompleteWindowData)?,
        );
    }
    let warming = windows.iter().any(|window| window.warming);
    Ok(Candidate {
        account: FleetAccount {
            identity: details.email.clone(),
            observed_at: details.observed_at,
            windows,
        },
        capacity_class,
        expires_at: bucket.quota_expires_at(details.observed_at),
        warming,
    })
}

fn all_durations_distinct(windows: &[RateLimitWindow]) -> bool {
    let durations = windows
        .iter()
        .map(|window| window.duration_minutes)
        .collect::<BTreeSet<_>>();
    durations.len() == windows.len()
}

/// Equal-capacity normalization is only an estimate, so it needs a reported plan to
/// stand on. A mixed or unknown plan must stay visibly unsupported instead of silently
/// treating different quotas as equal.
fn capacity_class(details: &Details, bucket: &RateLimitBucket) -> Option<String> {
    [details.plan.as_str(), bucket.plan.as_str()]
        .into_iter()
        .find(|plan| reports_capacity(plan))
        .map(str::to_owned)
}

/// Whether a plan label carries capacity information at all.
///
/// `UNKNOWN` is this crate's placeholder for a missing or empty field, while a provider can
/// also report an explicit unknown plan. Neither names a quota size, so neither may become a
/// capacity class or shadow a sibling field that does report one.
fn reports_capacity(plan: &str) -> bool {
    plan != UNKNOWN && !plan.eq_ignore_ascii_case("unknown")
}

/// Extracts one window's budget and pace, reusing the per-account validation and warmup
/// rules so the fleet cannot present a second, inconsistent interpretation.
fn fleet_window(window: &RateLimitWindow, observed_at: u64) -> Option<FleetWindow> {
    let used = usage::valid_used_percent(window.used_percent)?;
    let duration_minutes = window.duration_minutes.filter(|minutes| *minutes > 0)?;
    let duration = duration_minutes.checked_mul(60)?;
    let resets_at = u64::try_from(window.resets_at?).ok()?;
    let start = resets_at.checked_sub(duration)?;
    let elapsed = observed_at
        .checked_sub(start)
        .filter(|elapsed| *elapsed < duration)?;
    let elapsed_fraction = elapsed as f64 / duration as f64;
    let warming = used > 0.0 && elapsed_fraction < MIN_PROJECTION_ELAPSED_FRACTION;
    let rate = if used > 0.0 && elapsed > 0 {
        used / elapsed as f64
    } else {
        0.0
    };
    if !rate.is_finite() {
        return None;
    }
    Some(FleetWindow {
        duration,
        role: WindowRole::for_subscription(Some(duration_minutes))?,
        used_percent: used,
        rate,
        resets_at,
        warming,
    })
}
