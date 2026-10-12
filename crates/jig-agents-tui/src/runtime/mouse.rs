//! Mouse input: a click selects a home or focuses the details, a second click
//! on the same home within the double-click window launches it, and the wheel
//! moves the selection or scrolls the details under the pointer.

use std::time::{Duration, Instant};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use super::Action;
use crate::model::{App, Focus};

pub(crate) const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const DETAIL_WHEEL_LINES: i16 = 3;

pub(crate) fn handle_mouse(app: &mut App, mouse: MouseEvent, now: Instant) -> Action {
    if app.exit_state.is_some() {
        return Action::Ignore;
    }
    let position = Position::new(mouse.column, mouse.row);
    let areas = app.hit_areas();
    let inside = |area: Option<Rect>| area.is_some_and(|area| area.contains(position));
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let Some(index) = home_at(app, position) else {
                app.last_click = None;
                return if inside(areas.details) {
                    app.focus = Focus::Details;
                    Action::Redraw
                } else if inside(areas.list) {
                    app.focus = Focus::Homes;
                    Action::Redraw
                } else {
                    Action::Ignore
                };
            };
            app.focus = Focus::Homes;
            let repeated = app.last_click.is_some_and(|(clicked, at)| {
                clicked == index && now.saturating_duration_since(at) <= DOUBLE_CLICK
            });
            if repeated && app.selected == Some(index) {
                app.last_click = None;
                return Action::Select;
            }
            app.select(index);
            app.last_click = Some((index, now));
            Action::Redraw
        }
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
            let step = if mouse.kind == MouseEventKind::ScrollDown {
                1
            } else {
                -1
            };
            if inside(areas.details) {
                app.scroll_details(step * DETAIL_WHEEL_LINES);
                Action::Redraw
            } else if inside(areas.list) {
                app.move_selection(isize::from(step));
                Action::Redraw
            } else {
                Action::Ignore
            }
        }
        _ => Action::Ignore,
    }
}

/// The home drawn at `position` in the last frame, if any.
fn home_at(app: &App, position: Position) -> Option<usize> {
    let rows = app.hit_areas().rows?;
    if !rows.area.contains(position) {
        return None;
    }
    let row = usize::from((position.y - rows.area.y) / rows.row_height.max(1));
    app.visible_indices().get(rows.offset + row).copied()
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::{Home, render};

    fn app(count: usize) -> App {
        App::codex(
            (0..count)
                .map(|index| Home {
                    path: format!("/tmp/ExampleHome-{index:02}").into(),
                    name: format!("example-{index:02}"),
                    current: index == 0,
                })
                .collect(),
            Vec::new(),
        )
    }

    fn draw(app: &App, width: u16, height: u16) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| render::draw(frame, app)).unwrap();
    }

    fn event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn click(app: &mut App, column: u16, row: u16, at: Instant) -> Action {
        handle_mouse(
            app,
            event(MouseEventKind::Down(MouseButton::Left), column, row),
            at,
        )
    }

    /// The screen row of the visible home at `position` in the last frame.
    fn row_of(app: &App, position: usize) -> u16 {
        let rows = app.hit_areas().rows.unwrap();
        rows.area.y + u16::try_from(position - rows.offset).unwrap() * rows.row_height
    }

    #[test]
    fn a_click_selects_and_a_quick_second_click_launches() {
        let mut app = app(4);
        draw(&app, 120, 30);
        let start = Instant::now();
        let third = row_of(&app, 2);

        assert_eq!(click(&mut app, 10, third, start), Action::Redraw);
        assert_eq!(app.selected, Some(2));
        let quick = start + DOUBLE_CLICK / 2;
        assert_eq!(click(&mut app, 30, third, quick), Action::Select);
        assert_eq!(app.selected, Some(2));

        // Too slow, or on another home, is just another selection.
        let late = quick + DOUBLE_CLICK * 2;
        assert_eq!(click(&mut app, 10, third, late), Action::Redraw);
        assert_eq!(
            click(&mut app, 10, third, late + DOUBLE_CLICK * 2),
            Action::Redraw
        );
        let second = row_of(&app, 1);
        assert_eq!(
            click(&mut app, 10, second, late + DOUBLE_CLICK * 2),
            Action::Redraw
        );
        assert_eq!(app.selected, Some(1));
    }

    #[test]
    fn either_line_of_a_two_line_row_selects_its_home() {
        let mut app = app(4);
        draw(&app, 80, 30);
        assert_eq!(app.hit_areas().rows.unwrap().row_height, 2);
        let second_line = row_of(&app, 3) + 1;
        click(&mut app, 10, second_line, Instant::now());
        assert_eq!(app.selected, Some(3));
    }

    #[test]
    fn clicks_map_through_the_scrolled_list() {
        let mut app = app(20);
        app.move_to_edge(true);
        draw(&app, 120, 22);
        let rows = app.hit_areas().rows.unwrap();
        assert!(rows.offset > 0);
        click(&mut app, 10, rows.area.y, Instant::now());
        assert_eq!(app.selected, Some(rows.offset));
    }

    #[test]
    fn clicks_outside_the_rows_focus_a_pane_without_selecting() {
        let mut app = app(4);
        draw(&app, 120, 30);
        let areas = app.hit_areas();
        let details = areas.details.unwrap();
        let now = Instant::now();

        assert_eq!(
            click(&mut app, details.x + 2, details.y + 2, now),
            Action::Redraw
        );
        assert_eq!(app.focus, Focus::Details);
        assert_eq!(app.selected, Some(0));

        // The column header belongs to the list but is not a home.
        let header = areas.rows.unwrap().area.y - 1;
        assert_eq!(click(&mut app, 10, header, now), Action::Redraw);
        assert_eq!(app.focus, Focus::Homes);
        assert_eq!(app.selected, Some(0));

        let footer = 29;
        assert_eq!(click(&mut app, 10, footer, now), Action::Ignore);
    }

    #[test]
    fn the_wheel_moves_the_selection_or_scrolls_the_details_under_it() {
        let mut app = app(4);
        draw(&app, 120, 30);
        let now = Instant::now();
        let list = app.hit_areas().list.unwrap();
        let down = event(MouseEventKind::ScrollDown, list.x + 5, list.y + 3);
        assert_eq!(handle_mouse(&mut app, down, now), Action::Redraw);
        assert_eq!(app.selected, Some(1));
        let up = event(MouseEventKind::ScrollUp, list.x + 5, list.y + 3);
        handle_mouse(&mut app, up, now);
        assert_eq!(app.selected, Some(0));

        app.set_detail_scroll_limit(10);
        let details = app.hit_areas().details.unwrap();
        let down = event(MouseEventKind::ScrollDown, details.x + 5, details.y + 3);
        handle_mouse(&mut app, down, now);
        assert_eq!(app.detail_scroll, 3);
        assert_eq!(app.selected, Some(0));
    }

    #[test]
    fn mouse_input_is_ignored_before_the_first_frame_and_while_exiting() {
        let mut app = app(2);
        assert_eq!(click(&mut app, 10, 3, Instant::now()), Action::Ignore);
        draw(&app, 120, 30);
        app.begin_exit(crate::model::ExitState::Launching);
        let row = row_of(&app, 1);
        assert_eq!(click(&mut app, 10, row, Instant::now()), Action::Ignore);
        assert_eq!(app.selected, Some(0));
    }
}
