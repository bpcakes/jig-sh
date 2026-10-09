use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::model::{App, ExitState, Projection, unix_timestamp_now};
#[cfg(test)]
use layout::ListStyle;
use layout::{MIN_HEIGHT, MIN_WIDTH};
use list::draw_list;

mod configuration;
mod details;
mod footer;
mod layout;
mod list;

#[cfg(test)]
mod tests;

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;
const GOOD: Color = Color::Green;
const WARN: Color = Color::Yellow;
const BAD: Color = Color::Red;

pub(crate) fn draw(frame: &mut Frame, app: &App) {
    draw_at(frame, app, unix_timestamp_now());
}

pub(crate) fn draw_at(frame: &mut Frame, app: &App, now: u64) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        app.set_detail_scroll_limit(0);
        frame.render_widget(
            Paragraph::new(format!(
                "Terminal too small: {}x{}.\n{} needs at least {MIN_WIDTH}x{MIN_HEIGHT}.\nResize, or press q to cancel.",
                area.width, area.height, app.title()
            ))
            .block(panel(app.title(), false))
            .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let layout = layout::picker_layout(area, app);
    let best = app.best_projection_index_at(now);
    draw_header(frame, layout.header, app);
    if let Some(list) = layout.list {
        draw_list(frame, list, app, now, best);
    }
    if let Some(details) = layout.details {
        details::draw_details(frame, details, app, now, best);
    }
    footer::draw_footer(frame, layout.footer, app);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let working = !app.inspection_finished;
    let failed = app.inspection_finished
        && (app.inspection_error.is_some() || app.completed < app.rows.len());
    let discovery_warning_count = app.discovery_warnings.len();
    let (status, status_style) = if let Some(exit_state) = app.exit_state {
        match exit_state {
            ExitState::Launching => (
                if app.configuration_title.is_some() {
                    "Launching selected home…".to_owned()
                } else {
                    "Launching selected Codex home…".to_owned()
                },
                Style::default().fg(ACCENT),
            ),
            ExitState::Cancelling => (
                if app.configuration_title.is_some() {
                    "Cancelling…".to_owned()
                } else {
                    "Cancelling and cleaning up inspections…".to_owned()
                },
                Style::default().fg(WARN),
            ),
        }
    } else if app.static_configuration {
        if discovery_warning_count == 0 {
            (
                format!("{} configurations", app.rows.len()),
                Style::default().fg(GOOD),
            )
        } else {
            (
                format!("⚠ {discovery_warning_count} discovery warnings"),
                Style::default().fg(WARN),
            )
        }
    } else if working {
        let discovery_status = match discovery_warning_count {
            0 => String::new(),
            1 => "  ⚠ 1 discovery warning".to_owned(),
            count => format!("  ⚠ {count} discovery warnings"),
        };
        (
            format!(
                "{} Inspecting accounts and usage  {}/{}{}",
                spinner[app.tick % spinner.len()],
                app.completed,
                app.rows.len(),
                discovery_status
            ),
            Style::default().fg(WARN),
        )
    } else if failed {
        (
            format!("⚠ Inspection stopped  {}/{}", app.completed, app.rows.len()),
            Style::default().fg(BAD),
        )
    } else if discovery_warning_count > 0 {
        (
            format!(
                "⚠ Inspection complete; discovery partial  {}/{}",
                app.completed,
                app.rows.len()
            ),
            Style::default().fg(WARN),
        )
    } else {
        (
            format!(
                "✓ Inspection complete  {}/{}",
                app.completed,
                app.rows.len()
            ),
            Style::default().fg(GOOD),
        )
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {} ", app.title()),
                Style::default().fg(Color::Black).bg(ACCENT).bold(),
            ),
            Span::raw("  "),
            Span::styled(status, status_style),
        ])),
        area,
    );
}

/// A bordered pane; the focused one gets an accent border and a bold title.
fn panel(title: &str, focused: bool) -> Block<'_> {
    let block = Block::default().title(title).borders(Borders::ALL);
    if focused {
        block
            .border_style(Style::default().fg(ACCENT))
            .title_style(Style::default().bold())
    } else {
        block
    }
}

fn projection_style(projection: Projection) -> Style {
    match projection {
        Projection::Remaining { percent, .. } if percent >= 25.0 => Style::default().fg(GOOD),
        Projection::Remaining { percent, .. } if percent >= 10.0 => Style::default().fg(WARN),
        Projection::Remaining { .. }
        | Projection::ExhaustsEarly { .. }
        | Projection::Exhausted { .. }
        | Projection::InspectionError
        | Projection::UsageError => Style::default().fg(BAD),
        Projection::SignedOut => Style::default().fg(WARN),
        Projection::Collecting {
            remaining_percent, ..
        } if remaining_percent < 10.0 => Style::default().fg(BAD),
        Projection::Collecting {
            remaining_percent, ..
        } if remaining_percent < 25.0 => Style::default().fg(WARN),
        Projection::Loading
        | Projection::InspectionUnavailable
        | Projection::Unavailable
        | Projection::Collecting { .. } => Style::default().fg(MUTED),
    }
}

fn stale_snapshot_label(label: String, stale: bool) -> String {
    if stale {
        format!("stale · {label}")
    } else {
        label
    }
}

fn stale_usage_style(stale: bool) -> Style {
    if stale {
        Style::default().fg(MUTED)
    } else {
        Style::default()
    }
}

fn stale_projection_style(projection: Projection, stale: bool) -> Style {
    if stale {
        Style::default().fg(MUTED)
    } else {
        projection_style(projection)
    }
}
