use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use jig_tui::{CooperativeWorker, TerminalSession, is_actionable_key, require_terminal};
use serde_json::Value;

use crate::{
    SnapshotSource,
    model::{App, Tab},
    render,
};

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) fn run(source: impl SnapshotSource + 'static, refresh_interval: Duration) -> Result<()> {
    require_terminal(
        "jig status --tui",
        "use `jig status` or `jig status --json` when redirecting",
    )?;
    if refresh_interval.is_zero() {
        bail!("the status TUI refresh interval must be at least one second");
    }

    let source: Arc<dyn SnapshotSource> = Arc::new(source);
    let mut terminal = TerminalSession::enter("status TUI")?;
    let mut app = App::default();
    let mut worker = Some(RefreshWorker::spawn(Arc::clone(&source))?);
    app.refreshing = true;
    let mut next_refresh = Instant::now() + refresh_interval;
    let mut dirty = true;

    loop {
        if dirty {
            terminal
                .draw(|frame| render::draw(frame, &app))
                .context("failed to draw the status TUI")?;
            dirty = false;
        }

        if let Some(result) = worker.as_mut().and_then(RefreshWorker::try_finish) {
            worker = None;
            app.refreshing = false;
            match result {
                Ok(value) => app.accept_snapshot(value),
                Err(error) => app.accept_error(error),
            }
            next_refresh = Instant::now() + refresh_interval;
            dirty = true;
        }

        let refresh_due =
            worker.is_none() && (app.refresh_queued || Instant::now() >= next_refresh);
        if refresh_due {
            app.refresh_queued = false;
            app.refreshing = true;
            worker = Some(RefreshWorker::spawn(Arc::clone(&source))?);
            dirty = true;
        }

        if event::poll(EVENT_POLL_INTERVAL).context("failed to poll terminal input")? {
            match handle_event(
                &mut app,
                event::read().context("failed to read terminal input")?,
            ) {
                RuntimeAction::Ignore => {}
                RuntimeAction::Redraw => dirty = true,
                RuntimeAction::Refresh => {
                    if worker.is_some() {
                        app.refresh_queued = true;
                    } else {
                        app.refreshing = true;
                        worker = Some(RefreshWorker::spawn(Arc::clone(&source))?);
                    }
                    dirty = true;
                }
                RuntimeAction::Quit => {
                    if let Some(mut active) = worker.take() {
                        active.cancel_and_join();
                    }
                    return Ok(());
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeAction {
    Ignore,
    Redraw,
    Refresh,
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
    if app.package_detail_is_open() {
        return match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Backspace => {
                app.close_package_detail();
                RuntimeAction::Redraw
            }
            KeyCode::Char('r') => RuntimeAction::Refresh,
            KeyCode::Up | KeyCode::Char('k') => {
                app.scroll_package_detail(-1);
                RuntimeAction::Redraw
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.scroll_package_detail(1);
                RuntimeAction::Redraw
            }
            KeyCode::PageUp => {
                app.scroll_package_detail(-8);
                RuntimeAction::Redraw
            }
            KeyCode::PageDown => {
                app.scroll_package_detail(8);
                RuntimeAction::Redraw
            }
            KeyCode::Home => {
                app.move_package_detail_to_edge(false);
                RuntimeAction::Redraw
            }
            KeyCode::End => {
                app.move_package_detail_to_edge(true);
                RuntimeAction::Redraw
            }
            _ => RuntimeAction::Ignore,
        };
    }
    if key.code == KeyCode::Esc {
        return RuntimeAction::Quit;
    }
    match key.code {
        KeyCode::Char('r') => RuntimeAction::Refresh,
        KeyCode::Enter => {
            if app.open_package_detail() {
                RuntimeAction::Redraw
            } else {
                RuntimeAction::Ignore
            }
        }
        KeyCode::Tab => {
            app.cycle_tab(false);
            RuntimeAction::Redraw
        }
        KeyCode::BackTab => {
            app.cycle_tab(true);
            RuntimeAction::Redraw
        }
        KeyCode::Char('1') => {
            app.select_tab(Tab::Overview);
            RuntimeAction::Redraw
        }
        KeyCode::Char('2') => {
            app.select_tab(Tab::Packages);
            RuntimeAction::Redraw
        }
        KeyCode::Char('3') => {
            app.select_tab(Tab::Blockers);
            RuntimeAction::Redraw
        }
        KeyCode::Char('[') => {
            app.switch_provider(true);
            RuntimeAction::Redraw
        }
        KeyCode::Char(']') => {
            app.switch_provider(false);
            RuntimeAction::Redraw
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
        KeyCode::Char('b') => {
            app.toggle_blocked_only();
            RuntimeAction::Redraw
        }
        _ => RuntimeAction::Redraw,
    }
}

struct RefreshWorker(CooperativeWorker<Result<Value, String>>);

impl RefreshWorker {
    fn spawn(source: Arc<dyn SnapshotSource>) -> Result<Self> {
        CooperativeWorker::spawn("jig-status-refresh", move |cancelled| {
            let is_cancelled = || cancelled.is_cancelled();
            source.snapshot(&is_cancelled)
        })
        .map(Self)
    }

    fn try_finish(&mut self) -> Option<Result<Value, String>> {
        self.0
            .try_finish()
            .map(|result| result.and_then(|value| value))
    }

    fn cancel_and_join(&mut self) {
        self.0.cancel_and_join();
    }
}

#[cfg(test)]
mod tests {
    use jig_tui::require_terminal_with_state;

    use super::*;

    #[test]
    fn terminal_requirement_explains_each_redirect_case() {
        let require = |stdin, stdout| {
            require_terminal_with_state(
                "jig status --tui",
                "use `jig status` or `jig status --json` when redirecting",
                stdin,
                stdout,
            )
        };
        assert!(require(true, true).is_ok());
        assert!(
            require(false, true)
                .unwrap_err()
                .to_string()
                .contains("terminal input")
        );
        assert!(
            require(true, false)
                .unwrap_err()
                .to_string()
                .contains("terminal output")
        );
        assert!(
            require(false, false)
                .unwrap_err()
                .to_string()
                .contains("terminal input and output")
        );
    }

    #[test]
    fn keys_cover_tabs_navigation_refresh_and_quit() {
        let mut app = App::default();
        assert_eq!(
            handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            RuntimeAction::Redraw
        );
        assert_eq!(app.tab, Tab::Packages);
        assert_eq!(
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)
            ),
            RuntimeAction::Refresh
        );
        assert_eq!(
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
            ),
            RuntimeAction::Quit
        );
    }

    #[test]
    fn resize_events_request_an_immediate_redraw() {
        let mut app = App::default();

        assert_eq!(
            handle_event(&mut app, Event::Resize(120, 40)),
            RuntimeAction::Redraw
        );
    }

    #[test]
    fn enter_opens_package_detail_and_escape_returns_before_quitting() {
        let mut app = App::default();
        app.accept_snapshot(serde_json::json!({
            "schema_version": 1,
            "repository": {},
            "providers": [{
                "id": "provider",
                "report": {
                    "work_packages": [{
                        "id": "WP-001",
                        "specification": {},
                        "implementation": {},
                        "verification": {}
                    }]
                }
            }]
        }));
        app.select_tab(Tab::Packages);

        assert_eq!(
            handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            RuntimeAction::Redraw
        );
        assert!(app.package_detail_is_open());
        assert_eq!(
            handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            RuntimeAction::Redraw
        );
        assert!(!app.package_detail_is_open());
        assert_eq!(
            handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            RuntimeAction::Quit
        );
    }
}
