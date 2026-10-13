//! Fleet quota and reset-aware outlook, independent of the selected home.

use jig_tui::format_percent;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use super::{
    View, fleet_style,
    meter::{Bar, Look, meter},
    pad, panel, truncate,
};
use crate::model::{FleetAssessment, FleetCapacityWindow, FleetOutcome};
use crate::usage::remaining_percent;

pub(super) fn draw_fleet(frame: &mut Frame, area: Rect, view: &View<'_>) {
    let Some(fleet) = &view.fleet else {
        return;
    };
    let compact = area.height <= 3;
    let title = if area.width < 70 {
        "fleet"
    } else {
        "fleet capacity"
    };
    let mut block = panel(view, title, &fleet.coverage_label(), false);
    if area.width >= 70
        && let Some(capacity) = fleet.capacity()
    {
        let available = format!(
            "{}/{} usable at sample",
            capacity.usable_accounts,
            fleet.coverage().included()
        );
        block = block.title(
            Line::from(Span::styled(
                format!(" {available} "),
                fleet_style(view.theme, fleet),
            ))
            .right_aligned(),
        );
    }
    if !compact && fleet.capacity().is_some() {
        let legend = if area.width >= 96 {
            " █ used · ▄ pace/window · work transferable · earliest reset used first "
        } else {
            " █ used quota · ▄ pace/window "
        };
        block = block.title_bottom(
            Line::from(Span::styled(
                legend,
                Style::default().fg(view.theme.muted()),
            ))
            .right_aligned(),
        );
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = usize::from(inner.width);
    let lines = if compact {
        vec![compact_line(view, fleet, width)]
    } else {
        full_lines(view, fleet, width)
    };
    frame.render_widget(Paragraph::new(lines), inner);
}

fn full_lines(view: &View<'_>, fleet: &FleetAssessment, width: usize) -> Vec<Line<'static>> {
    let style = fleet_style(view.theme, fleet);
    let outcome = if fleet.is_stale() {
        "fleet forecast stale · press r to refresh".to_owned()
    } else if fleet.capacity().is_none() {
        fleet.summary_label_at(view.now)
    } else {
        fleet.outcome_label_at(view.now)
    };
    let symbol = match fleet.outcome() {
        FleetOutcome::NoGap {
            burn_observed: true,
        } if !fleet.is_stale() => "✓",
        FleetOutcome::GapRisk { .. } | FleetOutcome::BlockedNow { .. } => "▲",
        _ => "·",
    };
    let mut lines = vec![Line::from(Span::styled(
        truncate(&format!("{symbol} {outcome}"), width),
        style.bold(),
    ))];
    if let Some(capacity) = fleet.capacity() {
        let role_width = capacity
            .windows
            .iter()
            .map(|window| window.role.to_string().width())
            .max()
            .unwrap_or(0)
            .min(12);
        lines.extend(
            capacity
                .windows
                .iter()
                .take(2)
                .map(|window| window_line(view, fleet, window, role_width, width)),
        );
        lines.push(Line::from(Span::styled(
            truncate(
                "Equal quotas · window-average pace · periodic approximation · banked resets excluded",
                width,
            ),
            Style::default().fg(view.theme.muted()),
        )));
        let limiting = fleet
            .limiting_label()
            .map_or_else(String::new, |role| format!(" · limiting: {role}"));
        let horizon = format!("{} forecast horizon{limiting}", fleet.horizon_label());
        lines.push(Line::from(Span::styled(
            truncate(&horizon, width),
            Style::default().fg(view.theme.muted()),
        )));
    }
    // Explain excluded inputs in the global panel, rather than repeating the
    // entire fleet explanation for each selected home.
    if let Some(excluded) = fleet
        .detail_lines_at(view.now)
        .into_iter()
        .find_map(|(label, value)| (label == "Accounts").then_some(value))
        .filter(|_| !fleet.coverage().is_complete())
    {
        let explanation = Line::from(Span::styled(
            truncate(&excluded, width),
            Style::default().fg(view.theme.muted()),
        ));
        if lines.len() >= 5 {
            lines[4] = explanation;
        } else {
            lines.push(explanation);
        }
    }
    lines
}

fn window_line(
    view: &View<'_>,
    fleet: &FleetAssessment,
    window: &FleetCapacityWindow,
    role_width: usize,
    width: usize,
) -> Line<'static> {
    let label = format!(
        "{}  ",
        pad(&truncate(&window.role.to_string(), role_width), role_width)
    );
    let remaining = remaining_percent(window.gauge.used.unwrap_or_default());
    let pace = window.gauge.projected.map_or_else(
        || {
            if fleet.pace_collecting() {
                "collecting pace"
            } else {
                "pace unavailable"
            }
            .to_owned()
        },
        |used| format!("~{} / window", format_percent(used)),
    );
    let mut tail = format!("  {} left · {pace}", format_percent(remaining));
    if label.width() + 8 + tail.width() > width {
        tail = format!("  {} left", format_percent(remaining));
    }
    let meter_width = width.saturating_sub(label.width() + tail.width()).min(64);
    let mut spans = vec![Span::styled(label, Style::default().bold())];
    spans.extend(meter(
        view.theme,
        window.gauge,
        meter_width,
        Look {
            bar: Bar::Block,
            stale: fleet.is_stale(),
            selected: false,
        },
    ));
    spans.push(Span::styled(
        tail,
        if fleet.is_stale() {
            Style::default().fg(view.theme.muted())
        } else {
            Style::default()
        },
    ));
    Line::from(spans)
}

fn compact_line(view: &View<'_>, fleet: &FleetAssessment, width: usize) -> Line<'static> {
    let style = fleet_style(view.theme, fleet);
    let outcome = if fleet.is_stale() {
        "stale · r refresh".to_owned()
    } else {
        fleet
            .outcome_label_at(view.now)
            .split(';')
            .next()
            .unwrap_or_default()
            .replace("no gap projected", "no gap")
            .replace("collecting pace evidence", "collecting")
    };
    let window = fleet.capacity().and_then(|capacity| {
        capacity.windows.iter().max_by(|left, right| {
            left.gauge
                .used
                .unwrap_or_default()
                .total_cmp(&right.gauge.used.unwrap_or_default())
        })
    });
    let Some(window) = window else {
        return Line::from(Span::styled(truncate(&outcome, width), style));
    };
    let remaining = remaining_percent(window.gauge.used.unwrap_or_default());
    let tail = format!(" {} left · {outcome}", format_percent(remaining));
    let meter_width = 8.min(width.saturating_sub(18));
    let mut spans = meter(
        view.theme,
        window.gauge,
        meter_width,
        Look {
            bar: Bar::Line,
            stale: fleet.is_stale(),
            selected: false,
        },
    );
    spans.push(Span::styled(
        truncate(&tail, width.saturating_sub(meter_width)),
        style,
    ));
    Line::from(spans)
}
