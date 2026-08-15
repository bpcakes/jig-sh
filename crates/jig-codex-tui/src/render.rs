use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Wrap},
};

use crate::model::{App, ExitState, Focus, Inspection};

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;
const GOOD: Color = Color::Green;
const WARN: Color = Color::Yellow;
const BAD: Color = Color::Red;
const MIN_WIDTH: u16 = 46;
const MIN_HEIGHT: u16 = 12;

pub(crate) fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        app.set_detail_scroll_limit(0);
        frame.render_widget(
            Paragraph::new(format!(
                "Terminal too small: {}x{}.\nCodex Home Picker needs at least {MIN_WIDTH}x{MIN_HEIGHT}.\nResize, or press q to cancel.",
                area.width, area.height
            ))
            .block(panel("Codex Home Picker"))
            .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(if app.searching || !app.filter.is_empty() {
                3
            } else {
                2
            }),
        ])
        .split(area);
    draw_header(frame, outer[0], app);
    if area.width >= 96 {
        let content = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(outer[1]);
        draw_list(frame, content[0], app);
        draw_details(frame, content[1], app);
    } else {
        let content = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(outer[1]);
        draw_list(frame, content[0], app);
        draw_details(frame, content[1], app);
    }
    draw_footer(frame, outer[2], app);
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
                "Launching selected Codex home…".to_owned(),
                Style::default().fg(ACCENT),
            ),
            ExitState::Cancelling => (
                "Cancelling and cleaning up inspections…".to_owned(),
                Style::default().fg(WARN),
            ),
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
                " Codex Home Picker ",
                Style::default().fg(Color::Black).bg(ACCENT).bold(),
            ),
            Span::raw("  "),
            Span::styled(status, status_style),
        ])),
        area,
    );
}

fn draw_list(frame: &mut Frame, area: Rect, app: &App) {
    let visible = app.visible_indices();
    if visible.is_empty() {
        frame.render_widget(
            Paragraph::new("No homes match the current search.")
                .block(panel("Homes"))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    if area.width < 60 {
        draw_compact_list(frame, area, app, &visible);
        return;
    }
    let rows = visible
        .iter()
        .map(|index| {
            let row = &app.rows[*index];
            let marker = if row.is_current() { "*" } else { " " };
            Row::new([
                Cell::from(marker),
                Cell::from(row.display_name().to_owned()),
                Cell::from(row.account()),
                Cell::from(row.plan().to_owned()),
                Cell::from(row.usage()),
                Cell::from(row.state().to_owned()),
            ])
            .style(match row.inspection() {
                Inspection::Ready(details) if details.inspection_error.is_some() => {
                    Style::default().fg(BAD)
                }
                Inspection::Loading => Style::default().fg(MUTED),
                Inspection::Ready(_) | Inspection::Unavailable => Style::default(),
            })
        })
        .collect::<Vec<_>>();
    let widths = [
        Constraint::Length(1),
        Constraint::Percentage(19),
        Constraint::Percentage(28),
        Constraint::Percentage(12),
        Constraint::Percentage(25),
        Constraint::Percentage(16),
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new(["", "Home", "Account", "Plan", "Usage", "State"])
                .style(Style::default().fg(ACCENT).bold()),
        )
        .block(panel("Homes  (* current)"))
        .row_highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("›");
    let mut state = TableState::default()
        .with_offset(app.list_offset_for_viewport(area.height))
        .with_selected(
            app.selected
                .and_then(|selected| visible.iter().position(|index| *index == selected)),
        );
    frame.render_stateful_widget(table, area, &mut state);
    app.set_list_offset(state.offset());
}

fn draw_compact_list(frame: &mut Frame, area: Rect, app: &App, visible: &[usize]) {
    let rows = visible
        .iter()
        .map(|index| {
            let row = &app.rows[*index];
            Row::new([
                Cell::from(if row.is_current() { "*" } else { " " }),
                Cell::from(row.display_name().to_owned()),
                Cell::from(row.usage()),
                Cell::from(row.state().to_owned()),
            ])
            .style(match row.inspection() {
                Inspection::Ready(details) if details.inspection_error.is_some() => {
                    Style::default().fg(BAD)
                }
                Inspection::Loading => Style::default().fg(MUTED),
                Inspection::Ready(_) | Inspection::Unavailable => Style::default(),
            })
        })
        .collect::<Vec<_>>();
    let table = Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Percentage(35),
            Constraint::Percentage(40),
            Constraint::Percentage(25),
        ],
    )
    .header(Row::new(["", "Home", "Usage", "State"]).style(Style::default().fg(ACCENT).bold()))
    .block(panel("Homes  (* current)"))
    .row_highlight_style(
        Style::default()
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("›");
    let mut state = TableState::default()
        .with_offset(app.list_offset_for_viewport(area.height))
        .with_selected(
            app.selected
                .and_then(|selected| visible.iter().position(|index| *index == selected)),
        );
    frame.render_stateful_widget(table, area, &mut state);
    app.set_list_offset(state.offset());
}

fn draw_details(frame: &mut Frame, area: Rect, app: &App) {
    if app.selected_row().is_none() {
        app.set_detail_scroll_limit(0);
        frame.render_widget(
            Paragraph::new("No home selected.").block(panel(detail_title(app))),
            area,
        );
        return;
    }
    let lines = detail_lines(app);
    let sizing_paragraph = Paragraph::new(lines.clone())
        .block(panel(detail_title(app)))
        .wrap(Wrap { trim: false });
    let max_scroll = sizing_paragraph
        .line_count(area.width.saturating_sub(2))
        .saturating_sub(usize::from(area.height));
    let max_scroll = u16::try_from(max_scroll).unwrap_or(u16::MAX);
    app.set_detail_scroll_limit(max_scroll);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel(detail_title(app)))
            .wrap(Wrap { trim: false })
            .scroll((app.detail_scroll.min(max_scroll), 0)),
        area,
    );
}

fn detail_lines(app: &App) -> Vec<Line<'static>> {
    let Some(row) = app.selected_row() else {
        return Vec::new();
    };
    let mut lines = vec![
        key_value("Name", row.display_name()),
        key_value("Path", row.display_path()),
        key_value("Current", if row.is_current() { "yes" } else { "no" }),
    ];
    match row.inspection() {
        Inspection::Loading => {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Loading account and usage… You can launch now.",
                Style::default().fg(WARN),
            )));
        }
        Inspection::Unavailable => {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Inspection stopped before this home completed.",
                Style::default().fg(BAD),
            )));
        }
        Inspection::Ready(details) => {
            lines.extend([
                Line::from(""),
                key_value("Account", details.account_label()),
                key_value("Type", &details.account_type),
                key_value("Plan", &details.plan),
                key_value("Status", &details.status),
            ]);
            for bucket in &details.buckets {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("{} usage", bucket.label()),
                    Style::default().fg(ACCENT).bold(),
                )));
                if bucket.plan != "-" {
                    lines.push(key_value("Plan", &bucket.plan));
                }
                if bucket.reached != "-" {
                    lines.push(key_value("Reached", &bucket.reached));
                }
                for (index, window) in bucket.windows.iter().enumerate() {
                    let role = bucket.window_role(index);
                    lines.push(Line::from(format!(
                        "  {role}: {}  {}",
                        window.compact(),
                        window.reset_label()
                    )));
                }
            }
            if let Some(error) = &details.inspection_error {
                lines.push(error_line("Inspection", error));
            }
            if let Some(error) = &details.usage_error {
                lines.push(error_line("Usage", error));
            }
        }
    }
    if let Some(error) = &app.inspection_error {
        lines.push(error_line("Worker", error));
    }
    for warning in &app.discovery_warnings {
        lines.push(warning_line("Discovery", warning));
    }
    lines
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let controls = if app.exit_state.is_some() {
        "Please wait while background inspection is stopped safely."
    } else if app.searching {
        "Type to filter  Backspace edit  Ctrl-U clear  Enter launch  Esc finish search"
    } else if app.focus == Focus::Details {
        "DETAILS  ↑/↓ or j/k scroll  PgUp/PgDn jump  Tab homes  Enter launch  q cancel"
    } else {
        "HOMES  ↑/↓ or j/k move  / search  Tab details  Enter launch  Esc/q cancel"
    };
    let mut lines = vec![Line::from(Span::styled(
        controls,
        Style::default().fg(MUTED),
    ))];
    if app.searching || !app.filter.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("Search: ", Style::default().fg(ACCENT).bold()),
            Span::raw(app.filter.clone()),
            Span::styled(
                if app.searching { "▌" } else { "" },
                Style::default().fg(ACCENT),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), area);
    if app.searching && area.height > 1 {
        let prompt_width = 8_u16;
        let filter_width =
            u16::try_from(Line::from(app.filter.as_str()).width()).unwrap_or(u16::MAX);
        let cursor_x = area
            .x
            .saturating_add(prompt_width)
            .saturating_add(filter_width)
            .min(area.right().saturating_sub(1));
        frame.set_cursor_position((cursor_x, area.y.saturating_add(1)));
    }
}

fn detail_title(app: &App) -> &'static str {
    if app.focus == Focus::Details {
        "Selected home  [focused]"
    } else {
        "Selected home"
    }
}

fn panel(title: &str) -> Block<'_> {
    Block::default().title(title).borders(Borders::ALL)
}

fn key_value(label: &'static str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(MUTED)),
        Span::raw(value.to_owned()),
    ])
}

fn error_line(label: &str, error: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("{label} error: {error}"),
        Style::default().fg(BAD),
    ))
}

fn warning_line(label: &str, warning: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("{label} warning: {warning}"),
        Style::default().fg(WARN),
    ))
}
