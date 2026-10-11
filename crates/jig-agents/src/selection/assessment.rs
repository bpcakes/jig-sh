use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Reason {
    Eligible,
    InspectionUnavailable,
    InspectionError,
    SignedOut,
    UsageUnavailable,
    InvalidUsage,
    StaleUsage,
    Exhausted,
}

#[derive(Debug, Serialize)]
pub(super) struct Window {
    pub bucket: String,
    pub bucket_name: Option<String>,
    pub window: &'static str,
    pub duration_minutes: Option<u64>,
    pub resets_at: Option<i64>,
}

#[derive(Debug, Serialize)]
pub(super) struct Assessment {
    pub reason: Reason,
    pub remaining_percent: Option<f64>,
    pub limiting_window: Option<Window>,
}

impl Assessment {
    pub fn unavailable(reason: Reason) -> Self {
        Self {
            reason,
            remaining_percent: None,
            limiting_window: None,
        }
    }
}

/// Compare every reported window conservatively: forwarded model/configuration
/// arguments are opaque, so an additional bucket cannot be assumed irrelevant.
pub(super) fn assess(value: &Value, subscription_bucket: &str, now: i64) -> Assessment {
    match assess_windows(value, subscription_bucket, now) {
        Ok((remaining, window)) => Assessment {
            reason: if remaining > 0.0 {
                Reason::Eligible
            } else {
                Reason::Exhausted
            },
            remaining_percent: Some(remaining),
            limiting_window: Some(window),
        },
        Err(reason) => Assessment::unavailable(reason),
    }
}

fn assess_windows(
    value: &Value,
    subscription_bucket: &str,
    now: i64,
) -> Result<(f64, Window), Reason> {
    if !value["inspection_error"].is_null() {
        return Err(Reason::InspectionError);
    }
    if value["account"].is_null() {
        return Err(Reason::SignedOut);
    }
    if !value["usage_error"].is_null() {
        return Err(Reason::UsageUnavailable);
    }
    let buckets = value["rate_limits"]
        .as_array()
        .ok_or(Reason::UsageUnavailable)?;
    if !buckets.iter().any(|bucket| {
        bucket["id"] == subscription_bucket
            && (!bucket["primary"].is_null() || !bucket["secondary"].is_null())
    }) {
        return Err(Reason::UsageUnavailable);
    }
    let mut worst: Option<(f64, Window)> = None;
    for bucket in buckets {
        let id = bucket["id"].as_str().ok_or(Reason::InvalidUsage)?;
        if bucket["primary"].is_null() && bucket["secondary"].is_null() {
            return Err(Reason::UsageUnavailable);
        }
        if !bucket["reached"].is_null() {
            return Err(Reason::Exhausted);
        }
        for key in ["primary", "secondary"] {
            let window = &bucket[key];
            if window.is_null() {
                continue;
            }
            if window["invalid"] == true {
                return Err(Reason::InvalidUsage);
            }
            let used = window["used_percent"]
                .as_f64()
                .filter(|used| used.is_finite() && *used >= 0.0)
                .ok_or(Reason::InvalidUsage)?;
            let duration = if window["duration_minutes"].is_null() {
                None
            } else {
                Some(
                    window["duration_minutes"]
                        .as_u64()
                        .filter(|duration| *duration > 0)
                        .ok_or(Reason::InvalidUsage)?,
                )
            };
            let seconds = duration
                .map(|duration| {
                    i64::try_from(duration)
                        .ok()
                        .and_then(|duration| duration.checked_mul(60))
                        .ok_or(Reason::InvalidUsage)
                })
                .transpose()?;
            let reset = if window["resets_at"].is_null() {
                None
            } else {
                let reset = window["resets_at"].as_i64().ok_or(Reason::InvalidUsage)?;
                if reset <= now {
                    return Err(Reason::StaleUsage);
                }
                if seconds.is_some_and(|seconds| {
                    reset.checked_sub(seconds).is_none_or(|start| start > now)
                }) {
                    return Err(Reason::InvalidUsage);
                }
                Some(reset)
            };
            let remaining = (100.0 - used).max(0.0);
            if worst.as_ref().is_none_or(|(score, _)| remaining < *score) {
                worst = Some((
                    remaining,
                    Window {
                        bucket: id.to_owned(),
                        bucket_name: bucket["name"].as_str().map(str::to_owned),
                        window: key,
                        duration_minutes: duration,
                        resets_at: reset,
                    },
                ));
            }
        }
    }
    worst.ok_or(Reason::UsageUnavailable)
}
