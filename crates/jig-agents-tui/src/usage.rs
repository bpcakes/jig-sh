//! Shared interpretation of normalized subscription usage for the matching CLI release.
//!
//! Transport decoding, missing-value labels, and reset/projection presentation stay with
//! each consumer. This module never reads credentials or provider responses.

use std::fmt;

/// A subscription window's reported duration, or an unclassified generic window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowRole {
    FiveHour,
    Weekly,
    DurationMinutes(u64),
    Window,
}

impl WindowRole {
    /// Classifies a subscription window without guessing a missing duration.
    pub fn for_subscription(duration_minutes: Option<u64>) -> Option<Self> {
        duration_minutes.map(|minutes| match minutes {
            300 => Self::FiveHour,
            10_080 => Self::Weekly,
            minutes => Self::DurationMinutes(minutes),
        })
    }
}

impl fmt::Display for WindowRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::FiveHour => "5h",
            Self::Weekly => "weekly",
            Self::DurationMinutes(minutes) => {
                return formatter.write_str(&format_duration(*minutes));
            }
            Self::Window => "window",
        })
    }
}

/// Legacy Codex/Claude classification retained for same-release API compatibility.
/// New provider integrations pass subscription identity explicitly to the picker.
pub fn is_subscription_bucket(id: &str) -> bool {
    matches!(id, "codex" | "claude")
}

/// Rejects missing, nonfinite, and negative usage; over-limit values remain valid.
pub fn valid_used_percent(used: Option<f64>) -> Option<f64> {
    used.filter(|used| used.is_finite() && *used >= 0.0)
}

/// Remaining quota for a percentage accepted by [`valid_used_percent`].
pub fn remaining_percent(used: f64) -> f64 {
    (100.0 - used).max(0.0)
}

/// Compact duration text, preserving zero and nonstandard windows.
pub fn format_duration(minutes: u64) -> String {
    if minutes > 0 && minutes.is_multiple_of(60 * 24) {
        format!("{}d", minutes / (60 * 24))
    } else if minutes > 0 && minutes.is_multiple_of(60) {
        format!("{}h", minutes / 60)
    } else {
        format!("{minutes}m")
    }
}
