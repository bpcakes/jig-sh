use jig_tui::format_percent;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum WindowProjection {
    Unavailable,
    Collecting { remaining_percent: f64 },
    Remaining { percent: f64 },
    ExhaustsEarly { seconds: u64, score: f64 },
    Exhausted,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Projection {
    Loading,
    InspectionUnavailable,
    InspectionError,
    UsageError,
    SignedOut,
    Unavailable,
    Collecting {
        role: &'static str,
        remaining_percent: f64,
    },
    Remaining {
        role: &'static str,
        percent: f64,
        partial: bool,
    },
    ExhaustsEarly {
        role: &'static str,
        seconds: u64,
        score: f64,
        partial: bool,
    },
    Exhausted {
        role: &'static str,
        partial: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Recommendation {
    pub(crate) score: f64,
    pub(crate) label: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct UsageSnapshotAssessment {
    projection: Projection,
    freshness: UsageSnapshotFreshness,
    recommendation: Option<Recommendation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum UsageSnapshotFreshness {
    NotSampled,
    Fresh,
    Stale,
}

impl UsageSnapshotFreshness {
    pub(super) fn sampled_at(now: u64, expires_at: u64) -> Self {
        if now >= expires_at {
            Self::Stale
        } else {
            Self::Fresh
        }
    }
}

impl UsageSnapshotAssessment {
    pub(super) fn at(
        projection: Projection,
        freshness: UsageSnapshotFreshness,
        recommendation_eligible: bool,
    ) -> Self {
        let recommendation = (freshness == UsageSnapshotFreshness::Fresh
            && recommendation_eligible)
            .then(|| projection.recommendation())
            .flatten();
        Self {
            projection,
            freshness,
            recommendation,
        }
    }

    pub(crate) fn projection(self) -> Projection {
        self.projection
    }

    pub(crate) fn is_stale(self) -> bool {
        self.freshness == UsageSnapshotFreshness::Stale
    }

    pub(crate) fn has_sample(self) -> bool {
        self.freshness != UsageSnapshotFreshness::NotSampled
    }

    pub(crate) fn recommendation(self) -> Option<Recommendation> {
        self.recommendation
    }
}

impl Projection {
    pub(super) fn from_window(
        role: &'static str,
        projection: WindowProjection,
        partial: bool,
    ) -> Self {
        match projection {
            WindowProjection::Unavailable => Self::Unavailable,
            WindowProjection::Collecting { remaining_percent } => Self::Collecting {
                role,
                remaining_percent,
            },
            WindowProjection::Remaining { percent } => Self::Remaining {
                role,
                percent,
                partial,
            },
            WindowProjection::ExhaustsEarly { seconds, score } => Self::ExhaustsEarly {
                role,
                seconds,
                score,
                partial,
            },
            WindowProjection::Exhausted => Self::Exhausted { role, partial },
        }
    }

    pub(super) fn from_scored_window(
        role: &'static str,
        projection: WindowProjection,
        partial: bool,
    ) -> Option<(Self, f64)> {
        match projection {
            WindowProjection::Unavailable | WindowProjection::Collecting { .. } => None,
            WindowProjection::Remaining { percent } => Some((
                Self::Remaining {
                    role,
                    percent,
                    partial,
                },
                percent,
            )),
            WindowProjection::ExhaustsEarly { seconds, score } => Some((
                Self::ExhaustsEarly {
                    role,
                    seconds,
                    score,
                    partial,
                },
                score,
            )),
            WindowProjection::Exhausted => {
                Some((Self::Exhausted { role, partial }, f64::NEG_INFINITY))
            }
        }
    }

    pub(super) fn with_partial(self, partial: bool) -> Self {
        match self {
            Self::Remaining { role, percent, .. } => Self::Remaining {
                role,
                percent,
                partial,
            },
            Self::ExhaustsEarly {
                role,
                seconds,
                score,
                ..
            } => Self::ExhaustsEarly {
                role,
                seconds,
                score,
                partial,
            },
            Self::Exhausted { role, .. } => Self::Exhausted { role, partial },
            projection => projection,
        }
    }

    pub(crate) fn label(self) -> String {
        match self {
            Self::Loading => "loading…".into(),
            Self::InspectionUnavailable => "inspection stopped".into(),
            Self::InspectionError => "inspection error".into(),
            Self::UsageError => "usage error".into(),
            Self::SignedOut => "signed out".into(),
            Self::Unavailable => "projection unavailable".into(),
            Self::Collecting {
                role,
                remaining_percent,
            } => format!(
                "{role}: {} left · collecting",
                format_percent(remaining_percent)
            ),
            Self::Remaining {
                role,
                percent,
                partial,
                ..
            } => {
                if partial {
                    format!("{role}: ~{} left · partial", format_percent(percent))
                } else {
                    format!("{role}: ~{} left at reset", format_percent(percent))
                }
            }
            Self::ExhaustsEarly {
                role,
                seconds,
                partial,
                ..
            } => {
                let label = format!("{role}: runs out {} early", format_early(seconds));
                if partial {
                    format!("{label} · partial")
                } else {
                    label
                }
            }
            Self::Exhausted { role, partial, .. } => {
                if partial {
                    format!("{role}: exhausted · partial")
                } else {
                    format!("{role}: exhausted until reset")
                }
            }
        }
    }

    pub(crate) fn outcome_label(self) -> String {
        match self {
            Self::Loading => "loading…".into(),
            Self::InspectionUnavailable => "inspection stopped".into(),
            Self::InspectionError => "inspection error".into(),
            Self::UsageError => "usage error".into(),
            Self::SignedOut => "signed out".into(),
            Self::Unavailable => "projection unavailable".into(),
            Self::Collecting {
                remaining_percent, ..
            } => format!("{} left · collecting", format_percent(remaining_percent)),
            Self::Remaining { percent, .. } => {
                format!("~{} left at reset", format_percent(percent))
            }
            Self::ExhaustsEarly { seconds, .. } => {
                format!("runs out {} early", format_early(seconds))
            }
            Self::Exhausted { .. } => "exhausted until reset".into(),
        }
    }

    pub(crate) fn list_outcome_label(self) -> String {
        match self {
            Self::Remaining {
                percent,
                partial: true,
                ..
            } => format!("~{} left · partial", format_percent(percent)),
            Self::ExhaustsEarly {
                seconds,
                partial: true,
                ..
            } => format!("{} early · partial", format_early(seconds)),
            Self::Exhausted { partial: true, .. } => "exhausted · partial".into(),
            projection => projection.outcome_label(),
        }
    }

    pub(super) fn recommendation(self) -> Option<Recommendation> {
        match self {
            Self::Remaining {
                percent,
                partial: false,
                ..
            } => Some(Recommendation {
                score: percent,
                label: "best projected headroom at reset",
            }),
            Self::ExhaustsEarly {
                score,
                partial: false,
                ..
            } => Some(Recommendation {
                score,
                label: "least projected overrun; runs out early",
            }),
            _ => None,
        }
    }

    pub(super) fn severity_score(&self) -> Option<f64> {
        match *self {
            Self::Remaining { percent, .. } => Some(percent),
            Self::ExhaustsEarly { score, .. } => Some(score),
            Self::Exhausted { .. } => Some(f64::NEG_INFINITY),
            _ => None,
        }
    }
}

fn format_elapsed(seconds: u64) -> String {
    let (value, unit) = if seconds >= 86_400 {
        (seconds as f64 / 86_400.0, "d")
    } else if seconds >= 3_600 {
        (seconds as f64 / 3_600.0, "h")
    } else {
        (seconds as f64 / 60.0, "m")
    };
    if (value - value.round()).abs() < 0.05 {
        let rounded = value.round() as u64;
        match (rounded, unit) {
            (60, "m") => "1h".into(),
            (24, "h") => "1d".into(),
            _ => format!("{rounded}{unit}"),
        }
    } else {
        format!("{value:.1}{unit}")
    }
}

fn format_early(seconds: u64) -> String {
    if seconds < 60 {
        "<1m".into()
    } else {
        format!("~{}", format_elapsed(seconds))
    }
}
