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
use crate::usage::remaining_percent;

/// Timeline resolution below which two events are treated as simultaneous.
const TIME_EPSILON: f64 = 1e-6;

/// Upper bound on the events needed to carry one account's sample to the forecast origin.
/// That interval is a fraction of a single inspection pass in practice.
const ALIGNMENT_EVENT_BUDGET: usize = 512;

pub(super) enum SimulationOutcome {
    NoGap {
        burn_observed: bool,
    },
    Gap {
        gap_at: u64,
        recovers_at: Option<u64>,
        limiting: Vec<WindowRole>,
    },
    AlignmentBudgetExceeded,
    BudgetExceeded,
}

pub(super) struct Simulation {
    accounts: Vec<Vec<SimWindow>>,
    origin: u64,
    horizon: f64,
    recovery_limit: f64,
    aligned: bool,
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
    /// This account's own observed pace, used only to carry its sample to the forecast
    /// origin. Zero for a window whose pace is not yet evidence.
    self_rate: f64,
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
        let mut aligned = true;
        let accounts = accounts
            .iter()
            .map(|account| {
                let mut windows = account
                    .windows
                    .iter()
                    .map(|window| SimWindow {
                        role: window.role,
                        duration: window.duration as f64,
                        remaining: remaining_percent(window.used_percent),
                        demand: demand.get(&window.duration).copied().unwrap_or_default(),
                        // A warming pace is not evidence, so it must not spend observed quota
                        // on the way to the origin. It still counts toward fleet demand, which
                        // conserves the estimated workload.
                        self_rate: if window.warming { 0.0 } else { window.rate },
                        resets_at: offset_from(window.resets_at, origin),
                    })
                    .collect::<Vec<_>>();
                aligned &= align_to_origin(&mut windows, offset_from(account.observed_at, origin));
                windows
            })
            .collect();
        let horizon = longest_window as f64;
        Self {
            accounts,
            origin,
            horizon,
            recovery_limit: horizon * 2.0,
            aligned,
        }
    }

    pub(super) fn run(mut self) -> SimulationOutcome {
        if !self.aligned {
            return SimulationOutcome::AlignmentBudgetExceeded;
        }
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
            .filter(|(_, windows)| usable(windows))
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
        charge(&mut self.accounts[index], step, |window| window.demand);
    }

    fn apply_resets(&mut self, elapsed: f64) {
        for windows in &mut self.accounts {
            apply_due_resets(windows, elapsed);
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

fn offset_from(instant: u64, origin: u64) -> f64 {
    instant as f64 - origin as f64
}

/// Carries one account's sample forward from its own observation time to the forecast origin.
///
/// The account's windows advance together rather than independently, because an account
/// cannot spend any window's allowance while another applicable window blocks work. That is
/// the same all-windows constraint the main loop enforces, and skipping it here would let a
/// blocked account's surviving allowance be consumed on paper and then delay its recovery.
/// Resets still apply per window while the account is idle.
///
/// Returns false when the interval needs more events than its budget allows.
fn align_to_origin(windows: &mut [SimWindow], observed_at: f64) -> bool {
    let mut elapsed = observed_at;
    for _ in 0..ALIGNMENT_EVENT_BUDGET {
        apply_due_resets(windows, elapsed);
        if elapsed >= 0.0 {
            return true;
        }
        let mut target = next_reset(windows, elapsed).min(0.0);
        if usable(windows) {
            if let Some(depletion) = self_depletion(windows) {
                let depleted_at = elapsed + depletion;
                if depleted_at < target - TIME_EPSILON {
                    target = depleted_at;
                }
            }
            charge(windows, target - elapsed, |window| window.self_rate);
        }
        elapsed = target;
    }
    false
}

/// Whether an account can serve work: every reported window must have allowance left,
/// because any served work counts against all of them.
fn usable(windows: &[SimWindow]) -> bool {
    windows
        .iter()
        .all(|window| window.remaining > DEPLETED_PERCENT)
}

/// Replaces each due window's allowance with a full quota. Unused quota is replaced, never
/// accumulated, and a reset never touches a sibling window.
fn apply_due_resets(windows: &mut [SimWindow], elapsed: f64) {
    for window in windows {
        if window.resets_at <= elapsed {
            window.remaining = FULL_QUOTA_PERCENT;
            window.resets_at += window.duration;
        }
    }
}

/// Seconds until this account exhausts its tightest window at its own observed pace.
fn self_depletion(windows: &[SimWindow]) -> Option<f64> {
    windows
        .iter()
        .filter(|window| window.self_rate > 0.0)
        .map(|window| window.remaining / window.self_rate)
        .min_by(f64::total_cmp)
}

fn charge(windows: &mut [SimWindow], step: f64, rate: impl Fn(&SimWindow) -> f64) {
    for window in windows {
        let remaining = window.remaining - rate(window) * step;
        window.remaining = if remaining > DEPLETED_PERCENT {
            remaining.min(FULL_QUOTA_PERCENT)
        } else {
            0.0
        };
    }
}
