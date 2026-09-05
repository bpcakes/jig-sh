use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{List, ListItem, ListState, Paragraph, Wrap},
};

use super::super::model::{App, Tab, sanitize_text};

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
    let domain_name = if app.package_detail_is_open() {
        "Package detail"
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
        lines.push(Line::from(if app.package_detail_is_open() {
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
    let header = app.domain(app.tab).error.map_or_else(
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
    if matches!(app.tab, Tab::Status | Tab::Packages | Tab::Blockers) && app.status.data.is_none() {
        frame.render_widget(
            Paragraph::new(
                app.status
                    .error
                    .as_deref()
                    .unwrap_or("Loading status data..."),
            )
            .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    if matches!(app.tab, Tab::Work | Tab::Timeline | Tab::Health) && app.recorder.data.is_none() {
        frame.render_widget(
            Paragraph::new(
                app.recorder
                    .error
                    .as_deref()
                    .unwrap_or("Loading local data..."),
            )
            .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    if app.package_detail_is_open() {
        super::package_detail::draw(frame, area, app);
        return;
    }
    match app.tab {
        Tab::Status => draw_compact_lines(frame, area, compact_status_lines(app)),
        Tab::Packages => {
            let items = app
                .package_rows()
                .into_iter()
                .map(|package| ListItem::new(format!("{} {}", package.display_id, package.title)))
                .collect::<Vec<_>>();
            draw_compact_list(frame, area, items, app.package_index);
        }
        Tab::Blockers => {
            let items = app
                .current_provider()
                .into_iter()
                .flat_map(|provider| &provider.blockers)
                .map(|item| {
                    ListItem::new(format!(
                        "{} [{}] {}",
                        item.display_package_id, item.blocker.display_code, item.blocker.message
                    ))
                })
                .collect::<Vec<_>>();
            draw_compact_list(frame, area, items, app.blocker_index);
        }
        Tab::Work | Tab::Timeline | Tab::Health => {
            draw_compact_lines(frame, area, compact_recorder_lines(app));
        }
    }
}

fn draw_compact_lines(frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>) {
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

fn draw_compact_list(
    frame: &mut Frame,
    area: Rect,
    items: Vec<ListItem<'static>>,
    selected: usize,
) {
    let selected = (!items.is_empty()).then_some(selected.min(items.len().saturating_sub(1)));
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(
        List::new(items).highlight_symbol("▶ ").highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        area,
        &mut state,
    );
}

fn compact_status_lines(app: &App) -> Vec<Line<'static>> {
    let Some(dashboard) = &app.status.data else {
        return vec![Line::from("No status data.")];
    };
    let mut lines = vec![Line::from(format!(
        "{} [{}]",
        dashboard.repository.name, dashboard.outcome
    ))];
    if let Some(provider) = app.current_provider() {
        lines.push(Line::from(format!(
            "{}: {} packages, {} blockers",
            provider
                .display_name
                .as_deref()
                .unwrap_or(&provider.display_id),
            provider.summary.work_packages,
            provider.summary.blockers
        )));
    }
    lines
}

fn compact_recorder_lines(app: &App) -> Vec<Line<'static>> {
    let Some(recorder) = &app.recorder.data else {
        return vec![Line::from("No local data.")];
    };
    vec![
        Line::from(format!(
            "{} @ epoch {}",
            sanitize_text(&recorder.repo.name),
            recorder.epoch_id.get()
        )),
        Line::from(format!(
            "{} open plans | {} timeline rows",
            recorder.counts.open_plans,
            recorder.timeline.len()
        )),
    ]
}

fn compact_footer(app: &App) -> String {
    if app.package_detail_is_open() {
        return "q quit | Esc/Enter back | j/k scroll | r".to_string();
    }
    match app.tab {
        Tab::Packages => "q quit | Tab | j/k | Enter | b | [/]".to_string(),
        Tab::Blockers => "q quit | Tab views | j/k | [/]".to_string(),
        Tab::Status => "q quit | Tab views | r | [/]".to_string(),
        Tab::Work | Tab::Timeline | Tab::Health => "q quit | Tab views | r".to_string(),
    }
}

fn micro_summary(app: &App) -> String {
    if let Some(error) = app.domain(app.tab).error {
        return format!("Error: {error}");
    }
    match app.tab {
        Tab::Status => app.current_provider().map_or_else(
            || "No provider selected".to_string(),
            |provider| {
                format!(
                    "Selected: {}",
                    provider
                        .display_name
                        .as_deref()
                        .unwrap_or(&provider.display_id)
                )
            },
        ),
        Tab::Packages => app.selected_package().map_or_else(
            || "No package selected".to_string(),
            |package| format!("Selected: {} {}", package.display_id, package.title),
        ),
        Tab::Blockers => app.selected_blocker().map_or_else(
            || "No blocker selected".to_string(),
            |item| {
                format!(
                    "Selected: {} [{}] {}",
                    item.display_package_id, item.blocker.display_code, item.blocker.message
                )
            },
        ),
        Tab::Work | Tab::Timeline | Tab::Health => app.recorder.data.as_ref().map_or_else(
            || "No local snapshot".to_string(),
            |recorder| format!("Local snapshot epoch {}", recorder.epoch_id.get()),
        ),
    }
}

fn domain_state(app: &App, tab: Tab) -> &'static str {
    let domain = app.domain(tab);
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
