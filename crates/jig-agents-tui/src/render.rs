use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::model::{App, ExitState, Projection, unix_timestamp_now};
use layout::{MIN_HEIGHT, MIN_WIDTH};
use list::draw_list;
pub(crate) use theme::Theme;

mod details;
mod footer;
mod layout;
mod list;
mod meter;
mod theme;
mod wrap;

#[cfg(test)]
mod tests;

/// Everything one frame renders from.
pub(super) struct View<'a> {
    pub(super) app: &'a App,
    pub(super) theme: Theme,
    pub(super) now: u64,
    pub(super) best: Option<usize>,
}

pub(crate) fn draw(frame: &mut Frame, app: &App) {
    draw_themed(frame, app, Theme::default());
}

pub(crate) fn draw_themed(frame: &mut Frame, app: &App, theme: Theme) {
    draw_with(frame, app, theme, unix_timestamp_now());
}

#[cfg(test)]
pub(crate) fn draw_at(frame: &mut Frame, app: &App, now: u64) {
    draw_with(frame, app, Theme::default(), now);
}

fn draw_with(frame: &mut Frame, app: &App, theme: Theme, now: u64) {
    let area = frame.area();
    let view = View {
        app,
        theme,
        now,
        best: app.best_projection_index_at(now),
    };
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        app.set_detail_scroll_limit(0);
        frame.render_widget(
            Paragraph::new(format!(
                "Terminal too small: {}x{}.\n{} needs at least {MIN_WIDTH}x{MIN_HEIGHT}.\nResize, or press q to cancel.",
                area.width, area.height, app.title()
            ))
            .block(panel(&view, app.title(), "", false))
            .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let layout = layout::picker_layout(area, app);
    draw_header(frame, layout.header, &view);
    if let Some(list) = layout.list {
        draw_list(frame, list, &view);
    }
    if let Some(details) = layout.details {
        details::draw_details(frame, details, &view);
    }
    footer::draw_footer(frame, layout.footer, &view);
}

fn draw_header(frame: &mut Frame, area: Rect, view: &View<'_>) {
    let app = view.app;
    let theme = view.theme;
    let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let progress = format!("{}/{}", app.completed, app.rows.len());
    let failed = app.inspection_finished
        && (app.inspection_error.is_some() || app.completed < app.rows.len());
    let warnings = match app.discovery_warnings.len() {
        0 => String::new(),
        1 => "1 discovery warning".to_owned(),
        count => format!("{count} discovery warnings"),
    };
    // A full status, a short one for narrow terminals, and its color.
    let (status, short, color) = if let Some(exit_state) = app.exit_state {
        match exit_state {
            ExitState::Launching => (
                "Launching selected home…".to_owned(),
                "Launching…".to_owned(),
                theme.accent(),
            ),
            ExitState::Cancelling if app.static_configuration => (
                "Cancelling…".to_owned(),
                "Cancelling…".to_owned(),
                theme.warn(),
            ),
            ExitState::Cancelling => (
                "Cancelling and cleaning up inspections…".to_owned(),
                "Cancelling…".to_owned(),
                theme.warn(),
            ),
        }
    } else if app.static_configuration {
        if warnings.is_empty() {
            let count = format!("{} configurations", app.rows.len());
            (count.clone(), count, theme.good())
        } else {
            (
                format!("▲ {warnings}"),
                format!("▲ {warnings}"),
                theme.warn(),
            )
        }
    } else if !app.inspection_finished {
        let frame = spinner[app.tick % spinner.len()];
        let suffix = if warnings.is_empty() {
            String::new()
        } else {
            format!("  ▲ {warnings}")
        };
        let activity = if app.refreshed {
            "Refreshing"
        } else {
            "Inspecting"
        };
        (
            format!("{frame} {activity} accounts and usage  {progress}{suffix}"),
            format!("{frame} {progress}"),
            theme.warn(),
        )
    } else if failed {
        (
            format!("▲ Inspection stopped  {progress}"),
            format!("▲ stopped {progress}"),
            theme.bad(),
        )
    } else if !warnings.is_empty() {
        (
            format!("▲ Inspection complete; discovery partial  {progress}"),
            format!("▲ partial {progress}"),
            theme.warn(),
        )
    } else {
        (
            format!("✓ Inspection complete  {progress}"),
            format!("✓ {progress}"),
            theme.good(),
        )
    };
    let title = format!(" {} ", app.title());
    let status = if title.width() + 2 + status.width() <= usize::from(area.width) {
        status
    } else {
        short
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(title, theme.badge()),
            Span::raw("  "),
            Span::styled(status, Style::default().fg(color)),
        ])),
        area,
    );
}

/// A rounded pane titled in its border. The focused pane gets an accent
/// border and title; `count` follows the title in muted text.
fn panel<'a>(view: &View<'_>, title: &str, count: &str, focused: bool) -> Block<'a> {
    let theme = view.theme;
    let (border, title_style) = if focused {
        (
            Style::default().fg(theme.accent()),
            Style::default()
                .fg(theme.accent())
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            Style::default().fg(theme.border()),
            Style::default().add_modifier(Modifier::BOLD),
        )
    };
    let mut spans = vec![Span::raw(" "), Span::styled(title.to_owned(), title_style)];
    if !count.is_empty() {
        spans.push(Span::styled(
            format!(" {count}"),
            Style::default().fg(theme.muted()),
        ));
    }
    spans.push(Span::raw(" "));
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border)
        .title(Line::from(spans))
        .padding(Padding::horizontal(1))
}

/// `text` padded with spaces to `width` terminal cells.
fn pad(text: &str, width: usize) -> String {
    let padding = width.saturating_sub(text.width());
    format!("{text}{}", " ".repeat(padding))
}

/// `text` cut to `width` terminal cells, ending in `…` when cut.
fn truncate(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let mut kept = String::new();
    let mut used = 0;
    for character in text.chars() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width + 1 > width {
            break;
        }
        used += character_width;
        kept.push(character);
    }
    if width > 0 {
        kept.push('…');
    }
    kept
}

fn projection_style(theme: Theme, projection: Projection) -> Style {
    match projection {
        Projection::Remaining { percent, .. } if percent >= 25.0 => {
            Style::default().fg(theme.good())
        }
        Projection::Remaining { percent, .. } if percent >= 10.0 => {
            Style::default().fg(theme.warn())
        }
        Projection::Remaining { .. }
        | Projection::ExhaustsEarly { .. }
        | Projection::Exhausted { .. }
        | Projection::InspectionError
        | Projection::UsageError => Style::default().fg(theme.bad()),
        Projection::SignedOut => Style::default().fg(theme.warn()),
        Projection::Collecting {
            remaining_percent, ..
        } if remaining_percent < 10.0 => Style::default().fg(theme.bad()),
        Projection::Collecting {
            remaining_percent, ..
        } if remaining_percent < 25.0 => Style::default().fg(theme.warn()),
        Projection::Loading
        | Projection::InspectionUnavailable
        | Projection::Unavailable
        | Projection::Collecting { .. } => Style::default().fg(theme.muted()),
    }
}

fn stale_snapshot_label(label: String, stale: bool) -> String {
    if stale {
        format!("stale · {label}")
    } else {
        label
    }
}

fn stale_usage_style(theme: Theme, stale: bool) -> Style {
    if stale {
        Style::default().fg(theme.muted())
    } else {
        Style::default()
    }
}

fn stale_projection_style(theme: Theme, projection: Projection, stale: bool) -> Style {
    if stale {
        Style::default().fg(theme.muted())
    } else {
        projection_style(theme, projection)
    }
}
