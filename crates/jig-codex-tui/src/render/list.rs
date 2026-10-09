use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Text},
    widgets::{Block, Cell, Paragraph, Row, Table, TableState, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::{
    ACCENT, BAD, MUTED, configuration,
    layout::{ListStyle, STACKED_ROW_HEIGHT},
    panel, stale_projection_style, stale_snapshot_label, stale_usage_style,
};
use crate::model::{App, Focus, Inspection};

const MARKER_WIDTH: u16 = 2;
const HIGHLIGHT_SYMBOL: &str = "›";
const COLUMN_SPACING: u16 = 1;

/// One home's list text, shared by every list style.
struct ListRow {
    marker: &'static str,
    name: String,
    account: String,
    usage: String,
    usage_style: Style,
    projection: String,
    projection_style: Style,
    style: Style,
}

impl ListRow {
    fn at(app: &App, index: usize, now: u64, best: Option<usize>) -> Self {
        let row = &app.rows[index];
        let assessment = row.usage_snapshot_assessment_at(now);
        let projection = assessment.projection();
        Self {
            marker: row_marker(index, row.is_current(), best),
            name: row.display_name().to_owned(),
            account: row.account(),
            usage: stale_snapshot_label(row.usage(), assessment.quota_is_stale()),
            usage_style: stale_usage_style(assessment.quota_is_stale()),
            projection: stale_snapshot_label(projection.label(), assessment.projection_is_stale()),
            projection_style: stale_projection_style(projection, assessment.projection_is_stale()),
            style: inspection_row_style(row.inspection()),
        }
    }
}

pub(super) fn draw_list(frame: &mut Frame, area: Rect, app: &App, now: u64, best: Option<usize>) {
    let visible = app.visible_indices();
    if visible.is_empty() {
        frame.render_widget(
            Paragraph::new("No homes match the current search.")
                .block(list_panel(app, "Homes"))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    if app.static_configuration {
        configuration::draw_list(frame, area, app, &visible);
        return;
    }
    let rows = visible
        .iter()
        .map(|index| ListRow::at(app, *index, now, best))
        .collect::<Vec<_>>();
    let style = ListStyle::for_width(area.width);
    let table = match style {
        ListStyle::Compact => compact_table(&rows),
        ListStyle::TwoLine => two_line_table(&rows, area.width),
        ListStyle::Full => full_table(&rows, area.width),
    };
    let title = homes_panel_title(best, style == ListStyle::Compact);
    draw_home_table(
        frame,
        area,
        app,
        &visible,
        table.block(list_panel(app, title)),
    );
}

/// Home and account over the projection, in one column.
fn compact_table(rows: &[ListRow]) -> Table<'static> {
    let rows = rows.iter().map(|row| {
        Row::new([
            Cell::from(row.marker),
            Cell::from(Text::from(vec![
                Line::from(format!("{} · {}", row.name, row.account)),
                Line::styled(row.projection.clone(), row.projection_style),
            ])),
        ])
        .height(STACKED_ROW_HEIGHT)
        .style(row.style)
    });
    Table::new(
        rows,
        [Constraint::Length(MARKER_WIDTH), Constraint::Fill(1)],
    )
    .header(header_row(["", "Home · Account / Projection"]))
}

/// Home over account beside remaining quota over projection.
fn two_line_table(rows: &[ListRow], width: u16) -> Table<'static> {
    const HEADERS: [&str; 3] = ["", "Home / Account", "Remaining / Projection"];
    let identity = column_width(HEADERS[1], rows, |row| [&row.name, &row.account]);
    let quota = column_width(HEADERS[2], rows, |row| [&row.usage, &row.projection]);
    let [identity, _] = fit_widths(content_width(width, 2), [(identity, 14), (quota, 16)]);
    let table_rows = rows.iter().map(|row| {
        Row::new([
            Cell::from(row.marker),
            Cell::from(Text::from(vec![
                Line::from(row.name.clone()),
                Line::from(row.account.clone()),
            ])),
            Cell::from(Text::from(vec![
                Line::styled(row.usage.clone(), row.usage_style),
                Line::styled(row.projection.clone(), row.projection_style),
            ])),
        ])
        .height(STACKED_ROW_HEIGHT)
        .style(row.style)
    });
    Table::new(
        table_rows,
        [
            Constraint::Length(MARKER_WIDTH),
            Constraint::Length(identity),
            Constraint::Fill(1),
        ],
    )
    .header(header_row(HEADERS))
}

/// One line per home with its own Account column.
fn full_table(rows: &[ListRow], width: u16) -> Table<'static> {
    const HEADERS: [&str; 5] = ["", "Home", "Account", "Remaining now", "Projection"];
    let name = column_width(HEADERS[1], rows, |row| [&row.name]);
    let account = column_width(HEADERS[2], rows, |row| [&row.account]);
    let usage = column_width(HEADERS[3], rows, |row| [&row.usage]);
    let projection = column_width(HEADERS[4], rows, |row| [&row.projection]);
    let [name, account, usage, _] = fit_widths(
        content_width(width, 4),
        [(name, 8), (account, 10), (usage, 10), (projection, 14)],
    );
    let table_rows = rows.iter().map(|row| {
        Row::new([
            Cell::from(row.marker),
            Cell::from(row.name.clone()),
            Cell::from(row.account.clone()),
            Cell::from(row.usage.clone()).style(row.usage_style),
            Cell::from(row.projection.clone()).style(row.projection_style),
        ])
        .style(row.style)
    });
    Table::new(
        table_rows,
        [
            Constraint::Length(MARKER_WIDTH),
            Constraint::Length(name),
            Constraint::Length(account),
            Constraint::Length(usage),
            Constraint::Fill(1),
        ],
    )
    .header(header_row(HEADERS))
}

fn header_row<const N: usize>(labels: [&'static str; N]) -> Row<'static> {
    Row::new(labels).style(Style::default().fg(ACCENT).bold())
}

/// Width left for the text columns after borders, the highlight symbol, the
/// marker column, and the spacing before each of `columns` text columns.
fn content_width(list_width: u16, columns: u16) -> u16 {
    let highlight = u16::try_from(HIGHLIGHT_SYMBOL.width()).unwrap_or(1);
    list_width
        .saturating_sub(2)
        .saturating_sub(highlight)
        .saturating_sub(MARKER_WIDTH)
        .saturating_sub(COLUMN_SPACING.saturating_mul(columns))
}

fn column_width<'a, const N: usize>(
    header: &str,
    rows: &'a [ListRow],
    texts: impl Fn(&'a ListRow) -> [&'a String; N],
) -> u16 {
    let widest = rows
        .iter()
        .flat_map(texts)
        .map(|text| text.width())
        .chain([header.width()])
        .max()
        .unwrap_or(0);
    u16::try_from(widest).unwrap_or(u16::MAX)
}

/// Gives each column its content width. When they do not all fit, the widest
/// column above its minimum gives up a cell until they do or none can shrink.
pub(super) fn fit_widths<const N: usize>(available: u16, columns: [(u16, u16); N]) -> [u16; N] {
    let mut widths = columns.map(|(desired, minimum)| desired.max(minimum));
    let minimums = columns.map(|(_, minimum)| minimum);
    let mut total = widths.iter().map(|width| u32::from(*width)).sum::<u32>();
    while total > u32::from(available) {
        let Some(widest) = (0..N)
            .filter(|index| widths[*index] > minimums[*index])
            .max_by_key(|index| (widths[*index], N - index))
        else {
            break;
        };
        widths[widest] -= 1;
        total -= 1;
    }
    widths
}

pub(super) fn list_panel<'a>(app: &App, title: &'a str) -> Block<'a> {
    panel(title, app.focus == Focus::Homes)
}

pub(super) fn draw_home_table(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    visible: &[usize],
    table: Table<'_>,
) {
    // The selection stays visible but recedes while the details pane has focus.
    let highlight = if app.focus == Focus::Homes {
        Style::default().bg(Color::Blue).fg(Color::White)
    } else {
        Style::default().bg(Color::DarkGray).fg(Color::White)
    };
    let table = table
        .column_spacing(COLUMN_SPACING)
        .row_highlight_style(highlight.add_modifier(Modifier::BOLD))
        .highlight_symbol(HIGHLIGHT_SYMBOL);
    let mut state = TableState::default()
        .with_offset(app.list_offset_for_viewport(area.height))
        .with_selected(
            app.selected
                .and_then(|selected| visible.iter().position(|index| *index == selected)),
        );
    frame.render_stateful_widget(table, area, &mut state);
    app.set_list_offset(state.offset());
}

fn inspection_row_style(inspection: &Inspection) -> Style {
    match inspection {
        Inspection::Ready(details) if details.inspection_error.is_some() => {
            Style::default().fg(BAD)
        }
        Inspection::Loading => Style::default().fg(MUTED),
        Inspection::Ready(_) | Inspection::Unavailable => Style::default(),
    }
}

pub(super) fn row_marker(index: usize, current: bool, best: Option<usize>) -> &'static str {
    match (best == Some(index), current) {
        (true, true) => "+*",
        (true, false) => "+ ",
        (false, true) => " *",
        (false, false) => "  ",
    }
}

fn homes_panel_title(best: Option<usize>, compact: bool) -> &'static str {
    match (best.is_some(), compact) {
        (true, false) => "Homes  (+ best at current pace, * current)",
        (true, true) => "Homes  (+ best pace, * current)",
        (false, _) => "Homes  (* current; no rankable projection)",
    }
}
