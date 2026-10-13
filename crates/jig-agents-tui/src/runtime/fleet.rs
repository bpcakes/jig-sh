//! Input for the global fleet explanation takes precedence over hidden home controls.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::Action;
use crate::model::App;

pub(super) fn handle_key(app: &mut App, key: KeyEvent) -> Option<Action> {
    if !app.fleet_forecast_enabled() {
        return None;
    }
    if key.code == KeyCode::F(1)
        || (key.code == KeyCode::Char('?') && (!app.searching || app.fleet_details_open))
    {
        app.toggle_fleet_details();
        return Some(Action::Redraw);
    }
    if !app.fleet_details_open {
        return None;
    }
    Some(match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('q') => {
            app.toggle_fleet_details();
            Action::Redraw
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.scroll_fleet_details(-1);
            Action::Redraw
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.scroll_fleet_details(1);
            Action::Redraw
        }
        KeyCode::PageUp => {
            app.scroll_fleet_details(-8);
            Action::Redraw
        }
        KeyCode::PageDown => {
            app.scroll_fleet_details(8);
            Action::Redraw
        }
        KeyCode::Home => {
            app.move_fleet_details_to_edge(false);
            Action::Redraw
        }
        KeyCode::End => {
            app.move_fleet_details_to_edge(true);
            Action::Redraw
        }
        KeyCode::Char('r')
            if !key.modifiers.contains(KeyModifiers::CONTROL) && app.can_refresh() =>
        {
            Action::Refresh
        }
        _ => Action::Ignore,
    })
}
