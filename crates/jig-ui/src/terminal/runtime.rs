use std::time::Duration;

use super::{
    DashboardOptions, InitialTab,
    model::{App, Tab},
    render,
};
use crate::dashboard::DashboardSource;
use anyhow::{Result, bail};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use jig_tui::{TerminalSession, is_actionable_key, require_terminal};

mod event_loop;
mod scheduler;
mod worker;

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) fn run(source: impl DashboardSource + 'static, options: DashboardOptions) -> Result<()> {
    run_with_cancellation(source, options, || false)
}

pub(crate) fn run_with_cancellation(
    source: impl DashboardSource + 'static,
    options: DashboardOptions,
    externally_cancelled: impl Fn() -> bool,
) -> Result<()> {
    validate_refresh_interval(options.refresh_interval)?;
    require_terminal(
        "Jig dashboard",
        "use `jig ui --json` or `jig status --json` when redirecting",
    )?;
    let mut terminal = TerminalSession::enter("Jig dashboard")?;
    let mut app = App::new(match options.initial_tab {
        InitialTab::Status => Tab::Status,
        InitialTab::Work => Tab::Work,
    });
    if let Some(plan_id) = options.initial_plan.clone() {
        app.request_initial_plan(plan_id);
    }
    event_loop::run(&mut terminal, source, app, options, externally_cancelled)
}

fn validate_refresh_interval(interval: Duration) -> Result<()> {
    if interval < Duration::from_secs(1) {
        bail!("the dashboard refresh interval must be at least one second");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeAction {
    Ignore,
    Redraw,
    TabChanged,
    Refresh,
    RefreshDetail,
    DetailRequested,
    GrowTimeline,
    ShrinkTimeline,
    Quit,
}

fn handle_event(app: &mut App, event: Event) -> RuntimeAction {
    match event {
        Event::Key(key) if is_actionable_key(key) => handle_key(app, key),
        Event::Resize(_, _) => RuntimeAction::Redraw,
        _ => RuntimeAction::Ignore,
    }
}

fn handle_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    if key.code == KeyCode::Char('q')
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        return RuntimeAction::Quit;
    }
    if app.detail_is_open() {
        return match key.code {
            KeyCode::Esc | KeyCode::Backspace => {
                app.close_detail();
                RuntimeAction::Redraw
            }
            KeyCode::Enter => {
                app.open_detail_leaf_or_close();
                RuntimeAction::Redraw
            }
            KeyCode::Tab => {
                app.cycle_detail_section(false);
                RuntimeAction::Redraw
            }
            KeyCode::BackTab => {
                app.cycle_detail_section(true);
                RuntimeAction::Redraw
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.move_detail_selection(-1);
                RuntimeAction::Redraw
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.move_detail_selection(1);
                RuntimeAction::Redraw
            }
            KeyCode::Left | KeyCode::Char('h') => {
                app.scroll_detail_horizontal(-4);
                RuntimeAction::Redraw
            }
            KeyCode::Right | KeyCode::Char('l') => {
                app.scroll_detail_horizontal(4);
                RuntimeAction::Redraw
            }
            KeyCode::PageUp => {
                app.move_detail_selection(-8);
                RuntimeAction::Redraw
            }
            KeyCode::PageDown => {
                app.move_detail_selection(8);
                RuntimeAction::Redraw
            }
            KeyCode::Home => {
                app.move_detail_to_edge(false);
                RuntimeAction::Redraw
            }
            KeyCode::End => {
                app.move_detail_to_edge(true);
                RuntimeAction::Redraw
            }
            KeyCode::Char('r' | 'R') => {
                if app.detail.target_plan_id.is_some() {
                    RuntimeAction::RefreshDetail
                } else {
                    RuntimeAction::Refresh
                }
            }
            _ => RuntimeAction::Ignore,
        };
    }
    if key.code == KeyCode::Esc {
        return RuntimeAction::Quit;
    }
    match key.code {
        KeyCode::Char('r' | 'R') => RuntimeAction::Refresh,
        KeyCode::Enter => {
            if app.open_selected_detail() {
                RuntimeAction::DetailRequested
            } else {
                RuntimeAction::Ignore
            }
        }
        KeyCode::Tab | KeyCode::Right => {
            app.cycle_tab(false);
            RuntimeAction::TabChanged
        }
        KeyCode::BackTab | KeyCode::Left => {
            app.cycle_tab(true);
            RuntimeAction::TabChanged
        }
        KeyCode::Char('1') => {
            app.select_tab(Tab::Status);
            RuntimeAction::TabChanged
        }
        KeyCode::Char('2') => {
            app.select_tab(Tab::Work);
            RuntimeAction::TabChanged
        }
        KeyCode::Char('3') => {
            app.select_tab(Tab::Timeline);
            RuntimeAction::TabChanged
        }
        KeyCode::Char('4') => {
            app.select_tab(Tab::Health);
            RuntimeAction::TabChanged
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.move_selection(-1);
            RuntimeAction::Redraw
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.move_selection(1);
            RuntimeAction::Redraw
        }
        KeyCode::PageUp => {
            app.move_selection(-10);
            RuntimeAction::Redraw
        }
        KeyCode::PageDown => {
            app.move_selection(10);
            RuntimeAction::Redraw
        }
        KeyCode::Home => {
            app.move_to_edge(false);
            RuntimeAction::Redraw
        }
        KeyCode::End => {
            app.move_to_edge(true);
            RuntimeAction::Redraw
        }
        KeyCode::Char('f') if app.tab == Tab::Timeline => {
            app.cycle_timeline_filter(false);
            RuntimeAction::Redraw
        }
        KeyCode::Char('F') if app.tab == Tab::Timeline => {
            app.cycle_timeline_filter(true);
            RuntimeAction::Redraw
        }
        KeyCode::Char('+') if app.tab == Tab::Timeline => RuntimeAction::GrowTimeline,
        KeyCode::Char('-') if app.tab == Tab::Timeline => RuntimeAction::ShrinkTimeline,
        _ => RuntimeAction::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn number_keys_select_the_four_local_views() {
        let mut app = App::default();
        for (key, tab) in [
            ('1', Tab::Status),
            ('2', Tab::Work),
            ('3', Tab::Timeline),
            ('4', Tab::Health),
        ] {
            assert_eq!(
                handle_key(
                    &mut app,
                    KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE)
                ),
                RuntimeAction::TabChanged
            );
            assert_eq!(app.tab, tab);
        }
        for removed in ['5', '6', '[', ']', 'b'] {
            assert_eq!(
                handle_key(
                    &mut app,
                    KeyEvent::new(KeyCode::Char(removed), KeyModifiers::NONE)
                ),
                RuntimeAction::Ignore
            );
        }
    }

    #[test]
    fn refresh_interval_is_bounded() {
        assert!(validate_refresh_interval(Duration::from_secs(1)).is_ok());
        assert!(validate_refresh_interval(Duration::from_millis(999)).is_err());
    }
}
