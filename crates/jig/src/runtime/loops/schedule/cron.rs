use anyhow::{Result, bail};
use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use croner::Cron;

use crate::context::parse_five_field_cron;

#[derive(Clone, Debug)]
pub(in crate::runtime::loops) struct ScheduleSpec {
    expression: String,
    timezone_name: String,
    cron: Cron,
    timezone: Tz,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::runtime::loops) struct ScheduleWindow {
    pub(in crate::runtime::loops) due_at_ms: Option<u64>,
    pub(in crate::runtime::loops) next_at_ms: u64,
}

impl ScheduleSpec {
    pub(in crate::runtime::loops) fn parse(
        expression: &str,
        timezone_name: Option<&str>,
    ) -> Result<Self> {
        let cron = parse_five_field_cron(expression)?;
        let timezone_name = timezone_name.unwrap_or("UTC");
        let timezone = timezone_name
            .parse::<Tz>()
            .map_err(|_| anyhow::anyhow!("Invalid IANA timezone '{timezone_name}'"))?;
        Ok(Self {
            expression: expression.to_string(),
            timezone_name: timezone_name.to_string(),
            cron,
            timezone,
        })
    }

    pub(in crate::runtime::loops) fn expression(&self) -> &str {
        &self.expression
    }

    pub(in crate::runtime::loops) fn timezone_name(&self) -> &str {
        &self.timezone_name
    }

    pub(in crate::runtime::loops) fn window(
        &self,
        now_ms: u64,
        last_scheduled_at_ms: Option<u64>,
    ) -> Result<ScheduleWindow> {
        let now = datetime_from_ms(now_ms)?.with_timezone(&self.timezone);
        let most_recent = previous_matching(&self.cron, &now)?;
        let next = next_matching(&self.cron, &now)?;
        let most_recent_ms = timestamp_ms(most_recent)?;
        let due_at_ms = (last_scheduled_at_ms.is_none_or(|last| most_recent_ms > last))
            .then_some(most_recent_ms);
        Ok(ScheduleWindow {
            due_at_ms,
            next_at_ms: timestamp_ms(next)?,
        })
    }
}

fn previous_matching<T: TimeZone>(cron: &Cron, now: &DateTime<T>) -> Result<DateTime<T>>
where
    T::Offset: Copy,
{
    let mut candidate = cron
        .find_previous_occurrence(now, true)
        .map_err(|error| anyhow::anyhow!("Failed to find due cron occurrence: {error}"))?;
    for _ in 0..8 {
        if cron
            .is_time_matching(&candidate)
            .map_err(|error| anyhow::anyhow!("Failed to validate due cron occurrence: {error}"))?
        {
            return Ok(candidate);
        }
        candidate = cron
            .find_previous_occurrence(&candidate, false)
            .map_err(|error| anyhow::anyhow!("Failed to find due cron occurrence: {error}"))?;
    }
    bail!("Cron evaluator did not produce a valid previous occurrence")
}

fn next_matching<T: TimeZone>(cron: &Cron, now: &DateTime<T>) -> Result<DateTime<T>>
where
    T::Offset: Copy,
{
    let mut candidate = cron
        .find_next_occurrence(now, false)
        .map_err(|error| anyhow::anyhow!("Failed to find next cron occurrence: {error}"))?;
    for _ in 0..8 {
        if cron
            .is_time_matching(&candidate)
            .map_err(|error| anyhow::anyhow!("Failed to validate next cron occurrence: {error}"))?
        {
            return Ok(candidate);
        }
        candidate = cron
            .find_next_occurrence(&candidate, false)
            .map_err(|error| anyhow::anyhow!("Failed to find next cron occurrence: {error}"))?;
    }
    bail!("Cron evaluator did not produce a valid next occurrence")
}

fn datetime_from_ms(timestamp_ms: u64) -> Result<DateTime<Utc>> {
    let timestamp_ms = i64::try_from(timestamp_ms)
        .map_err(|_| anyhow::anyhow!("Schedule timestamp exceeds supported range"))?;
    DateTime::from_timestamp_millis(timestamp_ms)
        .ok_or_else(|| anyhow::anyhow!("Schedule timestamp is outside Chrono's supported range"))
}

fn timestamp_ms<T: TimeZone>(timestamp: DateTime<T>) -> Result<u64> {
    u64::try_from(timestamp.timestamp_millis())
        .map_err(|_| anyhow::anyhow!("Cron occurrence predates the Unix epoch"))
}
