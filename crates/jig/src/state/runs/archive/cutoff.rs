//! The `--before` cutoff accepted by `jig state archive`.

use anyhow::{Context, Result, bail};
use time::{Date, Month};

/// Parses a `YYYY-MM-DD` date, interpreted as UTC midnight, or a Unix
/// millisecond timestamp.
pub(super) fn parse_archive_before_ms(value: &str) -> Result<u64> {
    let value = value.trim();
    if value.is_empty() {
        bail!("--before must not be empty");
    }
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        return value
            .parse::<u64>()
            .with_context(|| format!("Invalid --before millisecond timestamp: {value}"));
    }

    let (year, month, day) = parse_utc_date(value)?;
    let month = Month::try_from(month as u8)
        .with_context(|| format!("Invalid --before month in {value}"))?;
    let date = Date::from_calendar_date(year, month, day as u8)
        .with_context(|| format!("Invalid --before date: {value}"))?;
    let timestamp_ms = date.midnight().assume_utc().unix_timestamp() * 1_000;
    if timestamp_ms < 0 {
        bail!("--before date must be on or after 1970-01-01: {value}");
    }
    Ok(timestamp_ms as u64)
}

fn parse_utc_date(value: &str) -> Result<(i32, u32, u32)> {
    let parts = value.split('-').collect::<Vec<_>>();
    if parts.len() != 3 {
        bail!(
            "Unsupported --before value '{value}'. Use YYYY-MM-DD or a Unix millisecond timestamp."
        );
    }
    let year = parts[0]
        .parse::<i32>()
        .with_context(|| format!("Invalid --before year in {value}"))?;
    if year < 1970 {
        bail!("--before date must be on or after 1970-01-01: {value}");
    }
    let month = parts[1]
        .parse::<u32>()
        .with_context(|| format!("Invalid --before month in {value}"))?;
    let day = parts[2]
        .parse::<u32>()
        .with_context(|| format!("Invalid --before day in {value}"))?;
    if !(1..=12).contains(&month) {
        bail!("Invalid --before month in {value}");
    }
    if day == 0 {
        bail!("Invalid --before day in {value}");
    }
    Ok((year, month, day))
}

#[cfg(test)]
mod tests {
    use super::parse_archive_before_ms;

    #[test]
    fn cutoff_accepts_utc_dates_and_millisecond_timestamps() {
        assert_eq!(parse_archive_before_ms("1970-01-02").unwrap(), 86_400_000);
        assert_eq!(parse_archive_before_ms(" 1000 ").unwrap(), 1_000);
    }

    #[test]
    fn cutoff_rejects_malformed_and_pre_epoch_values() {
        for value in [
            "",
            "2026-13-01",
            "2026-02-30",
            "1969-12-31",
            "yesterday",
            "2026-1",
        ] {
            assert!(parse_archive_before_ms(value).is_err(), "{value:?}");
        }
    }
}
