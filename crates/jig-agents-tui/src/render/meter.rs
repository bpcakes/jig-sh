//! Usage meters. Used quota fills from the left with eighth-cell precision,
//! colored green through red by position; a shaded ghost runs on to where the
//! sampled pace ends up at reset; and a tick marks how far into the window
//! the sample was taken, so a fill that passes the tick is ahead of pace.

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};

use super::theme::Theme;
use crate::model::Gauge;

const EIGHTHS: [&str; 8] = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
const FULL: &str = "█";
const LINE: &str = "━";
const HALF_LINE: &str = "╸";
/// Thinner glyphs in the same color read as "not yet" on any background.
const BLOCK_GHOST: &str = "▄";
const LINE_GHOST: &str = "╌";
const TRACK: &str = "─";
const TICK: &str = "│";

/// How a meter is drawn: full-height blocks with eighth-cell precision for
/// the details, or a slim line with half-cell precision that keeps stacked
/// list rows apart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Bar {
    Block,
    Line,
}

/// How one meter is drawn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Look {
    pub(super) bar: Bar,
    /// A stale sample keeps its shape in muted color.
    pub(super) stale: bool,
    /// On the selection bar the tick uses the selection's text color.
    pub(super) selected: bool,
}

/// Exactly `width` cells of meter.
pub(super) fn meter(theme: Theme, gauge: Gauge, width: usize, look: Look) -> Vec<Span<'static>> {
    let Look {
        bar,
        stale,
        selected,
    } = look;
    if width == 0 {
        return Vec::new();
    }
    let steps = match bar {
        Bar::Block => 8,
        Bar::Line => 2,
    };
    let cells = |percent: f64| percent.clamp(0.0, 100.0) / 100.0 * width as f64;
    let fill_steps = gauge
        .used
        .map_or(0, |used| (cells(used) * steps as f64).round() as usize);
    let ghost_end = gauge
        .projected
        .map_or(0, |projected| cells(projected).ceil() as usize);
    let tick = gauge
        .elapsed
        .map(|elapsed| ((elapsed * width as f64) as usize).min(width - 1));
    let color = |cell: usize| {
        if stale {
            theme.muted()
        } else {
            theme.gradient((cell as f64 + 0.5) / width as f64)
        }
    };

    let mut spans: Vec<Span<'static>> = Vec::with_capacity(width);
    for cell in 0..width {
        let filled = fill_steps.saturating_sub(cell * steps).min(steps);
        let (symbol, style) = if Some(cell) == tick {
            // The terminal's own foreground contrasts with its background.
            let style = Style::default().fg(if selected {
                theme.bright()
            } else {
                Color::Reset
            });
            let style = if filled == steps && bar == Bar::Block {
                style.bg(color(cell))
            } else {
                style
            };
            (TICK, style.add_modifier(Modifier::BOLD))
        } else if bar == Bar::Line {
            if filled == steps {
                (LINE, Style::default().fg(color(cell)))
            } else if filled > 0 {
                (HALF_LINE, Style::default().fg(color(cell)))
            } else if cell < ghost_end {
                (LINE_GHOST, Style::default().fg(color(cell)))
            } else {
                (TRACK, Style::default().fg(theme.faint()))
            }
        } else if filled == steps {
            (FULL, Style::default().fg(color(cell)))
        } else if filled > 0 {
            (EIGHTHS[filled], Style::default().fg(color(cell)))
        } else if cell < ghost_end {
            (BLOCK_GHOST, Style::default().fg(color(cell)))
        } else {
            (TRACK, Style::default().fg(theme.faint()))
        };
        match spans.last_mut() {
            Some(last) if last.style == style => last.content.to_mut().push_str(symbol),
            _ => spans.push(Span::styled(symbol, style)),
        }
    }
    spans
}

/// Used quota as a whole percentage that never rounds a partial quota up to
/// complete.
pub(super) fn used_label(gauge: Gauge) -> String {
    match gauge.used {
        Some(used) if used >= 100.0 => "100%".to_owned(),
        Some(used) => format!("{:>3}%", used.floor() as u64),
        None => "  ?%".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::theme::ColorDepth;

    fn text(spans: &[Span<'_>]) -> String {
        spans.iter().map(|span| span.content.as_ref()).collect()
    }

    fn look(bar: Bar, stale: bool) -> Look {
        Look {
            bar,
            stale,
            selected: false,
        }
    }

    fn gauge(used: Option<f64>, elapsed: Option<f64>, projected: Option<f64>) -> Gauge {
        Gauge {
            used,
            elapsed,
            projected,
        }
    }

    #[test]
    fn meter_fills_ghosts_and_ticks_in_exactly_its_width() {
        let theme = Theme::default();
        let spans = meter(
            theme,
            gauge(Some(25.0), Some(0.5), Some(50.0)),
            20,
            look(Bar::Block, false),
        );
        assert_eq!(text(&spans), "█████▄▄▄▄▄│─────────");
        let spans = meter(
            theme,
            gauge(Some(43.75), None, None),
            8,
            look(Bar::Block, false),
        );
        assert_eq!(text(&spans), "███▌────");
        for width in [0, 1, 7, 33] {
            let spans = meter(
                theme,
                gauge(Some(250.0), Some(0.99), Some(400.0)),
                width,
                look(Bar::Block, false),
            );
            assert_eq!(text(&spans).chars().count(), width);
        }
        assert_eq!(
            text(&meter(
                theme,
                gauge(None, None, None),
                5,
                look(Bar::Line, false)
            )),
            "─────"
        );
    }

    #[test]
    fn line_meters_fill_by_half_cells() {
        let theme = Theme::default();
        let line = meter(
            theme,
            gauge(Some(25.0), Some(0.75), Some(50.0)),
            10,
            look(Bar::Line, false),
        );
        assert_eq!(text(&line), "━━╸╌╌──│──");
        assert_eq!(
            text(&meter(
                theme,
                gauge(Some(5.0), None, None),
                10,
                look(Bar::Line, false)
            )),
            "╸─────────"
        );
    }

    #[test]
    fn a_fill_past_the_tick_keeps_its_color_behind_the_tick() {
        let theme = Theme::default();
        let spans = meter(
            theme,
            gauge(Some(80.0), Some(0.3), Some(100.0)),
            10,
            look(Bar::Block, false),
        );
        let tick = spans.iter().find(|span| span.content == TICK).unwrap();
        assert_eq!(tick.style.fg, Some(Color::Reset));
        assert!(matches!(tick.style.bg, Some(Color::Rgb(..))));
    }

    #[test]
    fn stale_and_monochrome_meters_keep_their_shape_without_gradient() {
        let stale = meter(
            Theme::default(),
            gauge(Some(50.0), None, None),
            4,
            look(Bar::Block, true),
        );
        assert_eq!(text(&stale), "██──");
        assert_eq!(stale[0].style.fg, Some(Theme::default().muted()));
        let plain = meter(
            Theme::new(ColorDepth::Monochrome),
            gauge(Some(50.0), Some(0.25), None),
            4,
            look(Bar::Block, false),
        );
        assert_eq!(text(&plain), "█│──");
        assert!(
            plain
                .iter()
                .all(|span| matches!(span.style.fg, Some(Color::Reset)))
        );
    }

    #[test]
    fn used_labels_never_round_up_to_complete() {
        assert_eq!(used_label(gauge(Some(99.9), None, None)), " 99%");
        assert_eq!(used_label(gauge(Some(100.0), None, None)), "100%");
        assert_eq!(used_label(gauge(Some(140.0), None, None)), "100%");
        assert_eq!(used_label(gauge(Some(4.2), None, None)), "  4%");
        assert_eq!(used_label(gauge(None, None, None)), "  ?%");
    }
}
