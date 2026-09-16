//! Pure reset-aware quota simulation.
//!
//! The scenario is deliberately narrow and documented: the estimated aggregate demand is
//! transferable between accounts, and it is served by whichever usable account's next
//! quota reset arrives first, because quota that resets soonest would otherwise be lost.
//! A gap found here is gap risk under that scenario, not proof that no allocation could
//! avoid one.
//!
//! Time is tracked as seconds after the forecast origin so reset instants, depletion
//! instants, and the horizon can be compared as one event timeline.

use std::collections::BTreeMap;

use super::{DEPLETED_PERCENT, FULL_QUOTA_PERCENT, FleetAccount, WORK_BUDGET, WindowRole};

/// Timeline resolution below which two events are treated as simultaneous.
const TIME_EPSILON: f64 = 1e-6;

pub(super) enum SimulationOutcome {
    NoGap {
        burn_observed: bool,
    },
    Gap {
        gap_at: u64,
        recovers_at: Option<u64>,
        limiting: Vec<WindowRole>,
    },
    BudgetExceeded,
}

pub(super) struct Simulation {
    accounts: Vec<Vec<SimWindow>>,
    origin: u64,
    horizon: f64,
    recovery_limit: f64,
}

#[derive(Clone, Copy, Debug)]
struct SimWindow {
    role: WindowRole,
    duration: f64,
    /// Remaining allowance of this account's own window, in reported percentage units.
    remaining: f64,
    /// Fleet-wide consumption rate charged to this dimension while this account serves the
    /// whole estimated workload.
    demand: f64,
    /// Next modeled reset, as seconds after the forecast origin.
    resets_at: f64,
}

impl Simulation {
    /// Aligns every sample to one forecast origin and pools demand per quota dimension.
    ///
    /// Accounts arrive in a stable identity order, which is also the allocation tie-break,
    /// so the result cannot depend on row order or iteration order.
    pub(super) fn new(accounts: &[FleetAccount], origin: u64, longest_window: u64) -> Self {
        let mut demand: BTreeMap<u64, f64> = BTreeMap::new();
        for account in accounts {
            for window in &account.windows {
                *demand.entry(window.duration).or_default() += window.rate;
            }
        }
        let accounts = accounts
            .iter()
            .map(|account| {
                account
                    .windows
                    .iter()
                    .map(|window| {
                        let (remaining, resets_at) = state_at_origin(
                            window.used_percent,
                            window.rate,
                            window.duration,
                            window.resets_at,
                            account.observed_at,
                            origin,
                        );
                        SimWindow {
                            role: window.role,
                            duration: window.duration as f64,
                            remaining,
                            demand: demand.get(&window.duration).copied().unwrap_or_default(),
                            resets_at,
                        }
                    })
                    .collect()
            })
            .collect();
        let horizon = longest_window as f64;
        Self {
            accounts,
            origin,
            horizon,
            recovery_limit: horizon * 2.0,
        }
    }

    pub(super) fn run(mut self) -> SimulationOutcome {
        if !self.burn_observed() {
            return SimulationOutcome::NoGap {
                burn_observed: false,
            };
        }
        let event_cost = self.accounts.iter().map(Vec::len).sum::<usize>().max(1);
        let mut work = 0;
        let mut elapsed = 0.0;
        let gap_at = loop {
            work += event_cost;
            if work > WORK_BUDGET {
                return SimulationOutcome::BudgetExceeded;
            }
            if elapsed >= self.horizon {
                return SimulationOutcome::NoGap {
                    burn_observed: true,
                };
            }
            let Some(index) = self.select(elapsed) else {
                break elapsed;
            };
            let mut target = self.horizon;
            if let Some(reset) = self.next_reset_after(elapsed) {
                target = target.min(reset);
            }
            // Depletion that lands on a reset or the horizon within rounding noise is not
            // an earlier event: charging exactly to a restoring reset must not be reported
            // as a sub-second outage.
            if let Some(depletion) = self.depletion(index) {
                let depleted_at = elapsed + depletion;
                if depleted_at < target - TIME_EPSILON {
                    target = depleted_at;
                }
            }
            let step = (target - elapsed).max(0.0);
            self.charge(index, step);
            elapsed = target;
            self.apply_resets(elapsed);
        };

        let limiting = self.limiting_roles();
        let gap_at = self.origin.saturating_add(gap_at.max(0.0).floor() as u64);
        loop {
            work += event_cost;
            if work > WORK_BUDGET {
                return SimulationOutcome::BudgetExceeded;
            }
            // No work is served during a gap, so only resets can restore capacity. A
            // returning window is not a recovery unless it makes some account usable
            // across every one of its constraints.
            let Some(reset) = self
                .next_reset_after(elapsed)
                .filter(|reset| *reset <= self.recovery_limit)
            else {
                return SimulationOutcome::Gap {
                    gap_at,
                    recovers_at: None,
                    limiting,
                };
            };
            elapsed = reset;
            self.apply_resets(elapsed);
            if self.select(elapsed).is_some() {
                return SimulationOutcome::Gap {
                    gap_at,
                    recovers_at: Some(self.origin.saturating_add(elapsed.max(0.0).ceil() as u64)),
                    limiting,
                };
            }
        }
    }

    fn burn_observed(&self) -> bool {
        self.accounts
            .iter()
            .flatten()
            .any(|window| window.demand > 0.0)
    }

    /// Picks the usable account whose next relevant reset arrives first. A usable account
    /// must have positive remaining quota in every one of its reported dimensions, because
    /// any served work counts against all of them.
    fn select(&self, elapsed: f64) -> Option<usize> {
        self.accounts
            .iter()
            .enumerate()
            .filter(|(_, windows)| {
                windows
                    .iter()
                    .all(|window| window.remaining > DEPLETED_PERCENT)
            })
            .min_by(|(left_index, left), (right_index, right)| {
                next_reset(left, elapsed)
                    .total_cmp(&next_reset(right, elapsed))
                    .then(left_index.cmp(right_index))
            })
            .map(|(index, _)| index)
    }

    /// Seconds until the selected account exhausts its tightest constrained dimension.
    fn depletion(&self, index: usize) -> Option<f64> {
        self.accounts[index]
            .iter()
            .filter(|window| window.demand > 0.0)
            .map(|window| window.remaining / window.demand)
            .min_by(f64::total_cmp)
    }

    fn charge(&mut self, index: usize, step: f64) {
        for window in &mut self.accounts[index] {
            let remaining = window.remaining - window.demand * step;
            window.remaining = if remaining > DEPLETED_PERCENT {
                remaining.min(FULL_QUOTA_PERCENT)
            } else {
                0.0
            };
        }
    }

    /// Replaces each due window's allowance with a full quota. Unused quota is replaced,
    /// never accumulated, and a reset never touches a sibling window.
    fn apply_resets(&mut self, elapsed: f64) {
        for window in self.accounts.iter_mut().flatten() {
            if window.resets_at <= elapsed {
                window.remaining = FULL_QUOTA_PERCENT;
                window.resets_at += window.duration;
            }
        }
    }

    fn next_reset_after(&self, elapsed: f64) -> Option<f64> {
        self.accounts
            .iter()
            .flatten()
            .map(|window| window.resets_at)
            .filter(|reset| *reset > elapsed)
            .min_by(f64::total_cmp)
    }

    fn limiting_roles(&self) -> Vec<WindowRole> {
        let mut roles = Vec::new();
        for window in self.accounts.iter().flatten() {
            if window.remaining <= DEPLETED_PERCENT && !roles.contains(&window.role) {
                roles.push(window.role);
            }
        }
        roles
    }
}

fn next_reset(windows: &[SimWindow], elapsed: f64) -> f64 {
    windows
        .iter()
        .map(|window| window.resets_at)
        .filter(|reset| *reset > elapsed)
        .min_by(f64::total_cmp)
        .unwrap_or(f64::INFINITY)
}

/// Projects one observed window forward to the common forecast origin.
///
/// Samples older than the origin are advanced by their own observed pace, and their own
/// resets are applied on the way, so a fixed sample is never reinterpreted as a declining
/// rate. Returns the remaining allowance and the next reset as seconds after the origin.
fn state_at_origin(
    used_percent: f64,
    rate: f64,
    duration: u64,
    resets_at: u64,
    observed_at: u64,
    origin: u64,
) -> (f64, f64) {
    let (last_reset, allowance) = if resets_at <= origin {
        let periods = (origin - resets_at) / duration;
        (
            resets_at.saturating_add(periods.saturating_mul(duration)),
            FULL_QUOTA_PERCENT,
        )
    } else {
        (observed_at, FULL_QUOTA_PERCENT - used_percent)
    };
    let remaining = (allowance - rate * origin.saturating_sub(last_reset) as f64)
        .clamp(0.0, FULL_QUOTA_PERCENT);
    let next_reset = if resets_at <= origin {
        last_reset.saturating_add(duration)
    } else {
        resets_at
    };
    (remaining, next_reset.saturating_sub(origin) as f64)
}
