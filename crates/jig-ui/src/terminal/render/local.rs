use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{List, ListItem, ListState, Paragraph, Wrap},
};

use super::{ACCENT, BAD, panel};
use crate::terminal::model::{App, DetailDocument};

pub(super) fn draw_timeline(frame: &mut Frame, area: Rect, app: &App) {
    let chunks =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).split(area);
    let rows = app.timeline_rows();
    let items = rows
        .iter()
        .map(|row| {
            ListItem::new(format!(
                "{} {} {}  {}",
                row.timestamp, row.display_identity, row.primary, row.secondary
            ))
        })
        .collect::<Vec<_>>();
    let title = app.recorder.data.as_ref().map_or_else(
        || "Timeline".to_string(),
        |local| {
            format!(
                "Timeline [{}] · {} · {}",
                app.timeline_filter.label(),
                local.timeline_limit,
                local.limits.timeline.label("rows")
            )
        },
    );
    draw_list(frame, chunks[0], title, items, app.timeline_index);
    let mut lines = app.selected_timeline().map_or_else(
        || vec![Line::from("No timeline row matches this filter.")],
        |row| document_lines(&row.detail),
    );
    append_local_notices(&mut lines, app);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel(
                "Timeline preview · f/F filter · +/- rows · Enter detail",
            ))
            .wrap(Wrap { trim: true }),
        chunks[1],
    );
}

pub(super) fn draw_compact_timeline(frame: &mut Frame, area: Rect, app: &App) {
    let items = app
        .timeline_rows()
        .iter()
        .map(|row| ListItem::new(format!("{}  {}", row.primary, row.secondary)))
        .collect();
    draw_list(
        frame,
        area,
        format!("Timeline [{}] · Enter opens", app.timeline_filter.label()),
        items,
        app.timeline_index,
    );
}

pub(super) fn draw_health(frame: &mut Frame, area: Rect, app: &App) {
    let chunks =
        Layout::horizontal([Constraint::Percentage(48), Constraint::Percentage(52)]).split(area);
    let Some(local) = app.recorder.data.as_ref() else {
        return;
    };
    let mut previous = None;
    let items = local
        .health
        .iter()
        .map(|row| {
            let section = if previous == Some(row.section) {
                String::new()
            } else {
                previous = Some(row.section);
                format!("{} · ", row.section)
            };
            ListItem::new(format!("{section}{}  {}", row.primary, row.secondary))
        })
        .collect::<Vec<_>>();
    draw_list(
        frame,
        chunks[0],
        format!(
            "Health · {} failures / {} targets",
            local.failures.len(),
            local.targets.len()
        ),
        items,
        app.health_index,
    );
    let mut lines = app.selected_health().map_or_else(
        || vec![Line::from("No health observations were reported.")],
        |row| document_lines(&row.detail),
    );
    lines.push(Line::from(format!(
        "Limits: {} · {}",
        local.limits.failures.label("failures"),
        local.limits.targets.label("targets")
    )));
    append_local_notices(&mut lines, app);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("Health detail · Enter opens"))
            .wrap(Wrap { trim: true }),
        chunks[1],
    );
}

pub(super) fn draw_compact_health(frame: &mut Frame, area: Rect, app: &App) {
    let items = app
        .recorder
        .data
        .iter()
        .flat_map(|local| &local.health)
        .map(|row| {
            ListItem::new(format!(
                "{} · {}  {}",
                row.section, row.primary, row.secondary
            ))
        })
        .collect();
    draw_list(
        frame,
        area,
        "Health · Enter opens".to_string(),
        items,
        app.health_index,
    );
}

pub(super) fn detail_footer() -> String {
    "q quit | Esc back | j/k vertical | h/l horizontal | r refresh".to_string()
}

pub(super) fn draw_detail(frame: &mut Frame, area: Rect, app: &App) {
    let Some(document) = &app.detail.document else {
        return;
    };
    let observed = app.detail.item_generated_at_ms.map_or_else(
        || "unknown time".to_string(),
        |timestamp| crate::terminal::model::format_timestamp(Some(timestamp)),
    );
    let stale = app.detail.item_epoch.is_some_and(|epoch| {
        app.recorder
            .data
            .as_ref()
            .is_some_and(|local| local.epoch_id != epoch)
    });
    let title = format!(
        "Detail · epoch {} · observed {observed}{} · Esc closes",
        app.detail
            .item_epoch
            .map_or(0, crate::dashboard::RecorderEpochId::get),
        if stale { " · stale" } else { "" }
    );
    draw_document(
        frame,
        area,
        document,
        app.detail.scroll,
        app.detail.horizontal_scroll,
        &title,
    );
}

fn append_local_notices(lines: &mut Vec<Line<'static>>, app: &App) {
    if let Some(local) = &app.recorder.data {
        lines.extend(local.errors.iter().map(|error| {
            Line::from(format!(
                "{}:{}{} — {}",
                error.scope,
                error.code,
                error
                    .subject
                    .as_deref()
                    .map(|subject| format!(" ({subject})"))
                    .unwrap_or_default(),
                error.message
            ))
            .style(Style::default().fg(BAD))
        }));
    }
}

fn document_lines(document: &DetailDocument) -> Vec<Line<'static>> {
    std::iter::once(
        Line::from(document.title.clone()).style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .chain(document.lines.iter().cloned().map(Line::from))
    .collect()
}

fn draw_document(
    frame: &mut Frame,
    area: Rect,
    document: &DetailDocument,
    scroll: u16,
    horizontal_scroll: u16,
    title: &str,
) {
    let visible = usize::from(area.height.saturating_sub(2).max(1));
    let lines = std::iter::once(document.title.as_str())
        .chain(document.lines.iter().map(String::as_str))
        .skip(usize::from(scroll))
        .take(visible)
        .map(|line| Line::from(line.to_string()))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel(title))
            .scroll((0, horizontal_scroll)),
        area,
    );
}

fn draw_list(
    frame: &mut Frame,
    area: Rect,
    title: String,
    items: Vec<ListItem<'static>>,
    selected: usize,
) {
    let selected = (!items.is_empty()).then(|| selected.min(items.len().saturating_sub(1)));
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(
        List::new(items)
            .block(panel(&title))
            .highlight_symbol("▶ ")
            .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        area,
        &mut state,
    );
}
