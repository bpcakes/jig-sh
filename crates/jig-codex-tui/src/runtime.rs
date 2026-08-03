use std::{
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use jig_tui::{CooperativeWorker, TerminalSession, is_actionable_key, require_terminal};

use crate::{
    Home, HomeUpdate, InspectionSource,
    model::{App, ExitState, Focus},
    render,
};

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const SPINNER_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) fn run(
    homes: Vec<Home>,
    source: impl InspectionSource + 'static,
    cancelled: impl Fn() -> bool + Send + Sync + 'static,
) -> Result<Option<PathBuf>> {
    require_terminal(
        "jig codex launch",
        "pass a home explicitly for non-interactive use",
    )?;
    let mut terminal = TerminalSession::enter("Codex home picker")?;
    let mut app = App::new(homes, source.discovery_warnings());
    let external_cancellation: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(cancelled);
    let mut worker = InspectionWorker::spawn(Arc::new(source), Arc::clone(&external_cancellation))?;
    let mut dirty = true;
    let mut next_spinner = Instant::now() + SPINNER_INTERVAL;

    loop {
        if external_cancellation() {
            return finish_run(
                &mut terminal,
                &mut app,
                &mut worker,
                ExitState::Cancelling,
                None,
            );
        }

        prepare_for_input(
            &mut app,
            &mut worker,
            &mut dirty,
            &mut next_spinner,
            Instant::now(),
            |app| {
                terminal
                    .draw(|frame| render::draw(frame, app))
                    .context("failed to draw the Codex home picker")?;
                Ok(())
            },
        )?;

        if external_cancellation() {
            return finish_run(
                &mut terminal,
                &mut app,
                &mut worker,
                ExitState::Cancelling,
                None,
            );
        }

        if event::poll(EVENT_POLL_INTERVAL).context("failed to poll Codex picker input")? {
            match event::read().context("failed to read Codex picker input")? {
                Event::Key(key) if is_actionable_key(key) => match handle_key(&mut app, key) {
                    Action::Ignore => {}
                    Action::Redraw => dirty = true,
                    Action::Cancel => {
                        return finish_run(
                            &mut terminal,
                            &mut app,
                            &mut worker,
                            ExitState::Cancelling,
                            None,
                        );
                    }
                    Action::Select => {
                        let selected = app.selected_path();
                        if selected.is_some() {
                            return finish_run(
                                &mut terminal,
                                &mut app,
                                &mut worker,
                                ExitState::Launching,
                                selected,
                            );
                        }
                    }
                },
                Event::Resize(_, _) => dirty = true,
                _ => {}
            }
        }
    }
}

fn finish_run(
    terminal: &mut TerminalSession,
    app: &mut App,
    worker: &mut InspectionWorker,
    exit_state: ExitState,
    selected: Option<PathBuf>,
) -> Result<Option<PathBuf>> {
    app.begin_exit(exit_state);
    let draw_result = terminal
        .draw(|frame| render::draw(frame, app))
        .context("failed to draw the Codex home picker exit state");
    worker.cancel_and_join();
    draw_result?;
    Ok(selected)
}

fn prepare_for_input(
    app: &mut App,
    worker: &mut InspectionWorker,
    dirty: &mut bool,
    next_spinner: &mut Instant,
    now: Instant,
    mut draw: impl FnMut(&App) -> Result<()>,
) -> Result<()> {
    let finished = worker.try_finish();
    while let Some(update) = worker.try_update() {
        app.apply_update(update);
        *dirty = true;
    }
    if let Some(result) = finished {
        app.finish_inspection(result.err());
        *dirty = true;
    }

    if !app.inspection_finished && now >= *next_spinner {
        app.tick = app.tick.wrapping_add(1);
        *next_spinner = now + SPINNER_INTERVAL;
        *dirty = true;
    }

    if *dirty {
        draw(app)?;
        *dirty = false;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    Ignore,
    Redraw,
    Cancel,
    Select,
}

pub(crate) fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Action::Cancel;
    }
    if app.searching {
        return match key.code {
            KeyCode::Esc => {
                app.searching = false;
                Action::Redraw
            }
            KeyCode::Enter => Action::Select,
            KeyCode::Backspace => {
                app.pop_filter();
                Action::Redraw
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.clear_filter();
                Action::Redraw
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                app.push_filter(character);
                Action::Redraw
            }
            KeyCode::Up => {
                app.move_selection(-1);
                Action::Redraw
            }
            KeyCode::Down => {
                app.move_selection(1);
                Action::Redraw
            }
            KeyCode::PageUp => {
                app.move_selection(-10);
                Action::Redraw
            }
            KeyCode::PageDown => {
                app.move_selection(10);
                Action::Redraw
            }
            KeyCode::Home => {
                app.move_to_edge(false);
                Action::Redraw
            }
            KeyCode::End => {
                app.move_to_edge(true);
                Action::Redraw
            }
            _ => Action::Ignore,
        };
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => Action::Cancel,
        KeyCode::Enter => Action::Select,
        KeyCode::Tab | KeyCode::BackTab => {
            app.toggle_focus();
            Action::Redraw
        }
        KeyCode::Char('/') => {
            app.focus = Focus::Homes;
            app.searching = true;
            Action::Redraw
        }
        KeyCode::Backspace if !app.filter.is_empty() => {
            app.pop_filter();
            Action::Redraw
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.clear_filter();
            Action::Redraw
        }
        KeyCode::Up | KeyCode::Char('k') => {
            match app.focus {
                Focus::Homes => app.move_selection(-1),
                Focus::Details => app.scroll_details(-1),
            }
            Action::Redraw
        }
        KeyCode::Down | KeyCode::Char('j') => {
            match app.focus {
                Focus::Homes => app.move_selection(1),
                Focus::Details => app.scroll_details(1),
            }
            Action::Redraw
        }
        KeyCode::PageUp => {
            match app.focus {
                Focus::Homes => app.move_selection(-10),
                Focus::Details => app.scroll_details(-8),
            }
            Action::Redraw
        }
        KeyCode::PageDown => {
            match app.focus {
                Focus::Homes => app.move_selection(10),
                Focus::Details => app.scroll_details(8),
            }
            Action::Redraw
        }
        KeyCode::Home => {
            match app.focus {
                Focus::Homes => app.move_to_edge(false),
                Focus::Details => app.move_details_to_edge(false),
            }
            Action::Redraw
        }
        KeyCode::End => {
            match app.focus {
                Focus::Homes => app.move_to_edge(true),
                Focus::Details => app.move_details_to_edge(true),
            }
            Action::Redraw
        }
        _ => Action::Ignore,
    }
}

struct InspectionWorker {
    updates: Receiver<HomeUpdate>,
    worker: Option<CooperativeWorker<Result<(), String>>>,
}

impl InspectionWorker {
    fn spawn(
        source: Arc<dyn InspectionSource>,
        external_cancellation: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<Self> {
        let (sender, updates) = mpsc::channel();
        let worker = CooperativeWorker::spawn("jig-codex-inspection", move |cancelled| {
            let is_cancelled = || cancelled.is_cancelled() || external_cancellation();
            let mut emit = |update| {
                sender
                    .send(update)
                    .map_err(|_| "Codex picker stopped accepting inspection updates".to_owned())
            };
            source.inspect(&mut emit, &is_cancelled)
        })?;
        Ok(Self {
            updates,
            worker: Some(worker),
        })
    }

    fn try_update(&self) -> Option<HomeUpdate> {
        self.updates.try_recv().ok()
    }

    fn try_finish(&mut self) -> Option<Result<(), String>> {
        let result = self
            .worker
            .as_mut()?
            .try_finish()
            .map(|result| result.and_then(|value| value));
        if result.is_some() {
            self.worker = None;
        }
        result
    }

    fn cancel_and_join(&mut self) {
        if let Some(mut worker) = self.worker.take() {
            worker.cancel_and_join();
        }
    }
}

impl Drop for InspectionWorker {
    fn drop(&mut self) {
        self.cancel_and_join();
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn inspection_update_is_drawn_before_a_queued_enter_can_select_it() {
        let homes = vec![Home {
            path: PathBuf::from("/tmp/.codex-work"),
            name: "codex-work".into(),
            current: true,
        }];
        let mut app = App::new(homes, Vec::new());
        for character in "person@example.com".chars() {
            app.push_filter(character);
        }
        assert_eq!(app.selected_path(), None);

        let (sender, updates) = mpsc::channel();
        sender
            .send(HomeUpdate {
                index: 0,
                details: json!({
                    "account": {
                        "type": "chatgpt",
                        "email": "person@example.com",
                        "plan_type": "pro"
                    },
                    "status": "authenticated",
                    "rate_limits": [],
                    "inspection_error": null,
                    "usage_error": null
                }),
            })
            .unwrap();
        let mut worker = InspectionWorker {
            updates,
            worker: None,
        };
        let mut dirty = false;
        let now = Instant::now();
        let mut next_spinner = now + Duration::from_secs(1);
        let mut rendered_selection = None;

        prepare_for_input(
            &mut app,
            &mut worker,
            &mut dirty,
            &mut next_spinner,
            now,
            |app| {
                rendered_selection = app.selected_path();
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(rendered_selection, Some(PathBuf::from("/tmp/.codex-work")));
        assert!(!dirty);
        assert_eq!(
            handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Action::Select
        );
        assert_eq!(app.selected_path(), rendered_selection);
    }
}
