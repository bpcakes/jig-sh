//! Global forecast access and explanation navigation, independent of home selection.

use super::App;
use crate::model::fleet::{self, FleetAssessment};

impl App {
    /// Only inspected pickers with a primary subscription bucket forecast the fleet.
    pub(crate) fn fleet_forecast_enabled(&self) -> bool {
        self.fleet_bucket.is_some() && !self.static_configuration
    }

    /// Search changes visible homes, never the modeled pool. Time changes freshness and
    /// countdowns, while inspection generations determine the cached forecast.
    pub(crate) fn fleet_assessment_at(&self, now: u64) -> Option<FleetAssessment> {
        if !self.fleet_forecast_enabled() {
            return None;
        }
        let bucket = self.fleet_bucket.as_deref()?;
        let mut cached = self.fleet_forecast.borrow_mut();
        if cached
            .as_ref()
            .is_none_or(|(generation, _)| *generation != self.fleet_generation)
        {
            *cached = Some((self.fleet_generation, fleet::forecast(&self.rows, bucket)));
        }
        cached.as_ref().map(|(_, forecast)| forecast.assess_at(now))
    }

    pub(crate) fn toggle_fleet_details(&mut self) {
        if self.fleet_forecast_enabled() {
            self.fleet_details_open = !self.fleet_details_open;
            self.fleet_scroll = 0;
            self.last_click = None;
        }
    }

    pub(crate) fn scroll_fleet_details(&mut self, delta: i16) {
        let limit = self.fleet_scroll_limit.get();
        self.fleet_scroll = self
            .fleet_scroll
            .min(limit)
            .saturating_add_signed(delta)
            .min(limit);
    }

    pub(crate) fn move_fleet_details_to_edge(&mut self, end: bool) {
        self.fleet_scroll = if end {
            self.fleet_scroll_limit.get()
        } else {
            0
        };
    }

    pub(crate) fn set_fleet_scroll_limit(&self, limit: u16) {
        self.fleet_scroll_limit.set(limit);
    }
}
