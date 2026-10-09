use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use super::{
    View,
    meter::{Bar, Look, meter},
    panel, stale_projection_style, stale_usage_style,
    wrap::{Detail, wrap},
};
use crate::model::{Focus, Inspection, WindowRole, WindowView};
use jig_tui::format_percent;

const STALE_PREFIX: &str = "stale · ";
/// Long enough to read a pace tick, short enough to scan in a wide pane.
const MAX_METER: usize = 48;
const MIN_INLINE_METER: usize = 16;
const LABEL_LIMIT: usize = 18;

pub(super) fn draw_details(frame: &mut Frame, area: Rect, view: &View<'_>) {
    let app = view.app;
    let focused = app.focus == Focus::Details;
    let Some(row) = app.selected_row() else {
        app.set_detail_scroll_limit(0);
        frame.render_widget(
            Paragraph::new("No home selected.").block(panel(view, "selected home", "", focused)),
            area,
        );
        return;
    };
    let mut block = panel(view, row.display_name(), "", focused);
    if let Inspection::Ready(details) = row.inspection() {
        let summary = [details.account_label(), details.plan.as_str()]
            .into_iter()
            .filter(|text| *text != "-")
            .collect::<Vec<_>>()
            .join(" · ");
        if !summary.is_empty() {
            block = block.title(
                Line::from(Span::styled(
                    format!(" {summary} "),
                    Style::default().fg(view.theme.muted()),
                ))
                .right_aligned(),
            );
        }
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = usize::from(inner.width);
    let lines = detail_lines(view, width)
        .into_iter()
        .flat_map(|detail| wrap(detail, width))
        .collect::<Vec<_>>();
    let max_scroll =
        u16::try_from(lines.len().saturating_sub(usize::from(inner.height))).unwrap_or(u16::MAX);
    app.set_detail_scroll_limit(max_scroll);
    frame.render_widget(
        Paragraph::new(lines).scroll((app.detail_scroll.min(max_scroll), 0)),
        inner,
    );
}

/// Usage first, since it decides the choice; the account and the home's
/// identity follow as aligned label columns.
fn detail_lines(view: &View<'_>, width: usize) -> Vec<Detail> {
    let app = view.app;
    let theme = view.theme;
    let Some(row) = app.selected_row() else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    if app.selected.is_some()
        && app.selected == view.best
        && let Some(recommendation) = row.usage_snapshot_assessment_at(view.now).recommendation()
    {
        lines.push(styled(
            &format!("◆ {}", recommendation.label),
            Style::default()
                .fg(theme.best())
                .add_modifier(Modifier::BOLD),
        ));
        lines.push(blank());
    }
    let mut facts: Vec<(String, String)> = Vec::new();
    // The account's facts come first; a blank line sets the home's apart.
    let mut account_facts = 0;
    let mut errors = Vec::new();
    if !app.static_configuration {
        match row.inspection() {
            Inspection::Loading => {
                lines.push(styled(
                    "Loading account and usage… You can launch now.",
                    Style::default().fg(theme.warn()),
                ));
                lines.push(blank());
            }
            Inspection::Unavailable => {
                lines.push(styled(
                    "Inspection stopped before this home completed.",
                    Style::default().fg(theme.bad()),
                ));
                lines.push(blank());
            }
            Inspection::Ready(details) => {
                // Align every bucket's meters on one column.
                let role_width = details
                    .buckets
                    .iter()
                    .flat_map(|bucket| details.windows_at(bucket, view.now))
                    .map(|window| window_name(&window).width())
                    .max()
                    .unwrap_or(0);
                for bucket in &details.buckets {
                    lines.push(styled(
                        &format!("{} usage", bucket.label()),
                        Style::default().fg(theme.accent()).bold(),
                    ));
                    if bucket.plan != "-" && bucket.plan != details.plan {
                        lines.push(styled(&format!("Plan: {}", bucket.plan), Style::default()));
                    }
                    if bucket.reached != "-" {
                        lines.push(styled(
                            &format!("Reached: {}", bucket.reached),
                            Style::default(),
                        ));
                    }
                    let windows = details.windows_at(bucket, view.now);
                    for (position, window) in windows.iter().enumerate() {
                        if position > 0 {
                            lines.push(blank());
                        }
                        lines.extend(window_lines(view, window, role_width, width));
                    }
                    lines.push(blank());
                }
                account_facts = 4;
                facts.extend([
                    ("Account".to_owned(), details.account_label().to_owned()),
                    ("Plan".to_owned(), details.plan.clone()),
                    ("Type".to_owned(), details.account_type.clone()),
                    ("Status".to_owned(), details.status.clone()),
                ]);
                if let Some(sample_age) = details.usage_sample_age_label_at(view.now) {
                    account_facts += 1;
                    facts.push((
                        "Usage sample".to_owned(),
                        format!("{sample_age} · reopen to refresh"),
                    ));
                }
                if let Some(error) = &details.inspection_error {
                    errors.push(("Inspection", error.clone()));
                }
                if let Some(error) = &details.usage_error {
                    errors.push(("Usage", error.clone()));
                }
            }
        }
    }
    facts.extend([
        ("Name".to_owned(), row.display_name().to_owned()),
        ("Path".to_owned(), row.display_path().to_owned()),
        (
            "Current".to_owned(),
            if row.is_current() { "yes" } else { "no" }.to_owned(),
        ),
    ]);
    if let Some(details) = &row.configuration_details {
        facts.extend(details.iter().cloned());
    }
    let label_width = facts
        .iter()
        .map(|(label, _)| label.width())
        .max()
        .unwrap_or(0)
        .min(LABEL_LIMIT);
    let label_style = Style::default().fg(theme.muted());
    for (position, (label, value)) in facts.iter().enumerate() {
        if position > 0 && position == account_facts {
            lines.push(blank());
        }
        lines.push(Detail::labeled(label, value, label_width, label_style));
    }
    for (label, error) in errors {
        lines.push(error_line(theme.bad(), &format!("{label} error: "), &error));
    }
    if let Some(error) = &app.inspection_error {
        lines.push(error_line(theme.bad(), "Worker error: ", error));
    }
    for warning in &app.discovery_warnings {
        lines.push(error_line(theme.warn(), "Discovery warning: ", warning));
    }
    lines
}

/// A usage window. Wide panes put the label, a long meter, and the used quota
/// with its reset on one line; narrow ones give the meter its own line. The
/// pace follows either way.
fn window_lines(
    view: &View<'_>,
    window: &WindowView<'_>,
    role_width: usize,
    width: usize,
) -> Vec<Detail> {
    let theme = view.theme;
    let quota_stale = window.assessment.quota_is_stale();
    let projection = window.assessment.projection();
    let projection_stale = window.assessment.projection_is_stale();
    let reset = window.window.reset_label_at(view.now);
    let pace = |indent: usize| {
        let mut line = snapshot_line(
            "At current pace: ",
            &projection.outcome_label(),
            projection_stale,
            stale_projection_style(theme, projection, projection_stale),
        );
        line.line.spans.insert(0, Span::raw(" ".repeat(indent)));
        line.hang += indent;
        line
    };
    let label = super::pad(&window_name(window), role_width + 2);
    let used = window.gauge.used.map_or_else(
        || "usage unavailable".to_owned(),
        |used| format!("{} used", format_percent(used)),
    );
    let tail = format!(
        "  {}{used} · {reset}",
        if quota_stale { STALE_PREFIX } else { "" }
    );
    let meter_width = width
        .saturating_sub(label.width() + tail.width())
        .min(MAX_METER);
    if meter_width >= MIN_INLINE_METER {
        let mut spans = vec![Span::styled(label.clone(), Style::default().bold())];
        spans.extend(meter(
            theme,
            window.gauge,
            meter_width,
            block_look(quota_stale),
        ));
        spans.push(Span::styled(tail, stale_usage_style(theme, quota_stale)));
        return vec![Detail::new(Line::from(spans), 0), pace(label.width())];
    }
    // A generic window's role does not name its duration.
    let usage = if window.role == WindowRole::Window {
        window.window.usage_detail()
    } else {
        window.window.usage_amounts()
    };
    let mut meter_line = vec![Span::raw("  ")];
    meter_line.extend(meter(
        theme,
        window.gauge,
        width.saturating_sub(2).min(MAX_METER),
        block_look(quota_stale),
    ));
    vec![
        snapshot_line(
            &format!("{}: ", window.role),
            &format!("{usage} · {reset}"),
            quota_stale,
            stale_usage_style(theme, quota_stale),
        ),
        Detail::new(Line::from(meter_line), 0),
        pace(0),
    ]
}

fn block_look(stale: bool) -> Look {
    Look {
        bar: Bar::Block,
        stale,
        selected: false,
    }
}

/// The window's role, or its duration when the role is only generic.
fn window_name(window: &WindowView<'_>) -> String {
    match (window.role, window.window.duration_minutes) {
        (WindowRole::Window, Some(minutes)) => crate::usage::format_duration(minutes),
        (role, _) => role.to_string(),
    }
}

fn styled(text: &str, style: Style) -> Detail {
    Detail::new(Line::styled(text.to_owned(), style), 0)
}

fn blank() -> Detail {
    styled("", Style::default())
}

fn error_line(color: ratatui::style::Color, label: &str, error: &str) -> Detail {
    Detail::new(
        Line::styled(format!("{label}{error}"), Style::default().fg(color)),
        label.width(),
    )
}

/// A `label: value` usage line that names a stale sample in text, not only color.
fn snapshot_line(label: &str, value: &str, stale: bool, style: Style) -> Detail {
    let prefix = format!("{}{label}", if stale { STALE_PREFIX } else { "" });
    Detail::new(
        Line::styled(format!("{prefix}{value}"), style),
        prefix.width(),
    )
}
