use super::*;

pub(super) fn draw_list(frame: &mut Frame, area: Rect, app: &App, visible: &[usize]) {
    let rows = visible.iter().map(|index| {
        let row = &app.rows[*index];
        Row::new([
            Cell::from(row_marker(*index, row.is_current(), None)),
            Cell::from(vec![
                Line::from(row.display_name().to_owned()),
                Line::from(Span::styled(
                    row.display_path().to_owned(),
                    Style::default().fg(MUTED),
                )),
            ]),
        ])
        .height(STACKED_ROW_HEIGHT)
    });
    let table = Table::new(rows, [Constraint::Length(2), Constraint::Min(1)])
        .header(Row::new(["", "Home / Path"]).style(Style::default().fg(ACCENT).bold()))
        .block(panel("Homes  (* current)"));
    draw_home_table(frame, area, app, visible, table);
}
