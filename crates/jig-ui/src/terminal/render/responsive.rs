use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::{Paragraph, Wrap},
};

use super::super::model::{App, Tab};

pub(crate) const MIN_WIDTH: u16 = 72;
pub(crate) const MIN_HEIGHT: u16 = 20;
pub(crate) const COMPACT_WIDTH: u16 = 40;
pub(crate) const COMPACT_HEIGHT: u16 = 12;
pub(crate) const WIDE_WIDTH: u16 = 108;
pub(crate) const WIDE_HEIGHT: u16 = 24;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LayoutTier {
    Micro,
    Compact,
    Standard,
    Wide,
}

pub(super) fn draw_if_responsive(frame: &mut Frame, area: Rect, app: &App) -> bool {
    if area.is_empty() {
        return true;
    }
    match layout_tier(area) {
        LayoutTier::Micro => {
            draw_micro(frame, area, app);
            true
        }
        LayoutTier::Compact => {
            draw_compact(frame, area, app);
            true
        }
        LayoutTier::Standard | LayoutTier::Wide => false,
    }
}

pub(crate) const fn layout_tier(area: Rect) -> LayoutTier {
    if area.width < COMPACT_WIDTH || area.height < COMPACT_HEIGHT {
        LayoutTier::Micro
    } else if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        LayoutTier::Compact
    } else if area.width >= WIDE_WIDTH && area.height >= WIDE_HEIGHT {
        LayoutTier::Wide
    } else {
        LayoutTier::Standard
    }
}

fn draw_micro(frame: &mut Frame, area: Rect, app: &App) {
    let domain_name = if app.detail_is_open() {
        "Detail"
    } else {
        app.tab
            .title()
            .split_once(' ')
            .map_or(app.tab.title(), |(_, name)| name)
    };
    let state = domain_state(app, app.tab);
    let mut lines = vec![Line::from(format!(
        "Jig {domain_name} {state} - {}x{}",
        area.width, area.height
    ))];
    if area.height > 2 {
        lines.push(Line::from(micro_summary(app)));
    }
    if area.height > 1 {
        lines.push(Line::from(if app.detail_is_open() {
            "q quit | Esc back | resize"
        } else {
            "q quit | resize for details"
        }));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

fn draw_compact(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);
    let state = domain_state(app, app.tab);
    let header = app.local_domain().error.map_or_else(
        || format!("Jig [{}] {state}", app.tab.title()),
        |error| format!("Jig [{}] {state} - {error}", app.tab.title()),
    );
    frame.render_widget(Paragraph::new(header), rows[0]);
    let tabs = Tab::ALL
        .into_iter()
        .map(|tab| {
            if tab == app.tab {
                format!("[{}]", tab.index() + 1)
            } else {
                (tab.index() + 1).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    frame.render_widget(Paragraph::new(tabs), rows[1]);
    draw_compact_content(frame, rows[2], app);
    frame.render_widget(Paragraph::new(compact_footer(app)), rows[3]);
}

fn draw_compact_content(frame: &mut Frame, area: Rect, app: &App) {
    if !app.domain_has_data(app.tab) {
        frame.render_widget(
            Paragraph::new(app.local_domain().error.unwrap_or("Loading local data..."))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    if app.detail_is_open() {
        super::local::draw_detail(frame, area, app);
        return;
    }
    match app.tab {
        Tab::Status => {
            frame.render_widget(
                Paragraph::new(compact_status_lines(app)).wrap(Wrap { trim: true }),
                area,
            );
        }
        Tab::Work => super::local::draw_compact_work(frame, area, app),
        Tab::Timeline => super::local::draw_compact_timeline(frame, area, app),
        Tab::Health => super::local::draw_compact_health(frame, area, app),
    }
}

fn compact_status_lines(app: &App) -> Vec<Line<'static>> {
    let Some(status) = &app.status else {
        return vec![Line::from("No status data.")];
    };
    vec![
        Line::from(format!("{} [{}]", status.repository.name, status.outcome)),
        Line::from(format!(
            "{} open plans · {} gate errors",
            status.work.open_plans, status.work.gate_errors
        )),
        Line::from(format!(
            "{} loop attempts · {} exhausted",
            status.loops.attempts, status.loops.exhausted_attempts
        )),
    ]
}

fn compact_footer(app: &App) -> String {
    if app.detail_is_open() {
        return super::local::detail_footer(app);
    }
    match app.tab {
        Tab::Status => "q quit | Tab views | r".to_string(),
        Tab::Work => "q quit | Tab views | j/k | Enter | r".to_string(),
        Tab::Timeline => "q quit | Tab views | j/k | Enter | f/F | +/- rows | r".to_string(),
        Tab::Health => "q quit | Tab views | j/k | Enter | r".to_string(),
    }
}

fn micro_summary(app: &App) -> String {
    if let Some(error) = app.local_domain().error {
        return format!("Error: {error}");
    }
    match app.tab {
        Tab::Status => app.status.as_ref().map_or_else(
            || "No status data".to_string(),
            |status| {
                format!(
                    "{}: {} open plans",
                    status.repository.name, status.work.open_plans
                )
            },
        ),
        Tab::Work => app.selected_work().map_or_else(
            || "No plan selected".to_string(),
            |plan| format!("Selected: {} {}", plan.display_plan_id, plan.title),
        ),
        Tab::Timeline => app.selected_timeline().map_or_else(
            || format!("No {} timeline rows", app.timeline_filter.label()),
            |row| format!("{}: {}", row.kind.label(), row.primary),
        ),
        Tab::Health => app.selected_health().map_or_else(
            || "No health item selected".to_string(),
            |row| format!("{}: {}", row.section, row.primary),
        ),
    }
}

fn domain_state(app: &App, tab: Tab) -> &'static str {
    let domain = app.local_domain();
    if domain.refreshing {
        "refreshing"
    } else if domain.error.is_some() && app.domain_has_data(tab) {
        "stale"
    } else if domain.error.is_some() {
        "error"
    } else {
        "ready"
    }
}
