use std::{
    io::{self, IsTerminal, Stdout},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use serde_json::Value;

use crate::{
    SnapshotSource,
    model::{App, Tab},
    render,
};

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) fn run(source: impl SnapshotSource + 'static, refresh_interval: Duration) -> Result<()> {
    require_terminal(io::stdin().is_terminal(), io::stdout().is_terminal())?;
    if refresh_interval.is_zero() {
        bail!("the status TUI refresh interval must be at least one second");
    }

    let source: Arc<dyn SnapshotSource> = Arc::new(source);
    let mut terminal = TerminalSession::enter()?;
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
            match event::read().context("failed to read terminal input")? {
                Event::Key(key) if is_actionable_key(key) => match handle_key(&mut app, key) {
                    RuntimeAction::Continue => dirty = true,
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
                },
                _ => {}
            }
        }
    }
}

fn require_terminal(stdin_is_terminal: bool, stdout_is_terminal: bool) -> Result<()> {
    match (stdin_is_terminal, stdout_is_terminal) {
        (true, true) => Ok(()),
        (false, false) => bail!(
            "`jig status --tui` requires terminal input and output; use `jig status` or `jig status --json` when redirecting"
        ),
        (false, true) => bail!(
            "`jig status --tui` requires terminal input; use `jig status --json` for pipelines"
        ),
        (true, false) => bail!(
            "`jig status --tui` requires terminal output; use `jig status --json` for redirected output"
        ),
    }
}

fn is_actionable_key(key: KeyEvent) -> bool {
    matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeAction {
    Continue,
    Refresh,
    Quit,
}

fn handle_key(app: &mut App, key: KeyEvent) -> RuntimeAction {
    if key.code == KeyCode::Esc
        || key.code == KeyCode::Char('q')
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        return RuntimeAction::Quit;
    }
    match key.code {
        KeyCode::Char('r') => RuntimeAction::Refresh,
        KeyCode::Tab => {
            app.cycle_tab(false);
            RuntimeAction::Continue
        }
        KeyCode::BackTab => {
            app.cycle_tab(true);
            RuntimeAction::Continue
        }
        KeyCode::Char('1') => {
            app.select_tab(Tab::Overview);
            RuntimeAction::Continue
        }
        KeyCode::Char('2') => {
            app.select_tab(Tab::Packages);
            RuntimeAction::Continue
        }
        KeyCode::Char('3') => {
            app.select_tab(Tab::Blockers);
            RuntimeAction::Continue
        }
        KeyCode::Char('[') => {
            app.switch_provider(true);
            RuntimeAction::Continue
        }
        KeyCode::Char(']') => {
            app.switch_provider(false);
            RuntimeAction::Continue
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.move_selection(-1);
            RuntimeAction::Continue
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.move_selection(1);
            RuntimeAction::Continue
        }
        KeyCode::PageUp => {
            app.move_selection(-10);
            RuntimeAction::Continue
        }
        KeyCode::PageDown => {
            app.move_selection(10);
            RuntimeAction::Continue
        }
        KeyCode::Home => {
            app.move_to_edge(false);
            RuntimeAction::Continue
        }
        KeyCode::End => {
            app.move_to_edge(true);
            RuntimeAction::Continue
        }
        KeyCode::Char('b') => {
            app.toggle_blocked_only();
            RuntimeAction::Continue
        }
        _ => RuntimeAction::Continue,
    }
}

struct RefreshWorker {
    cancelled: Arc<AtomicBool>,
    receiver: Receiver<Result<Value, String>>,
    handle: Option<JoinHandle<()>>,
}

impl RefreshWorker {
    fn spawn(source: Arc<dyn SnapshotSource>) -> Result<Self> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let thread_cancelled = Arc::clone(&cancelled);
        let (sender, receiver) = mpsc::channel();
        let handle = thread::Builder::new()
            .name("jig-status-refresh".to_owned())
            .spawn(move || {
                let is_cancelled = || thread_cancelled.load(Ordering::SeqCst);
                let result = source.snapshot(&is_cancelled);
                let _ = sender.send(result);
            })
            .context("failed to start the status refresh worker")?;
        Ok(Self {
            cancelled,
            receiver,
            handle: Some(handle),
        })
    }

    fn try_finish(&mut self) -> Option<Result<Value, String>> {
        let received = match self.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                Err("status refresh worker ended without returning a snapshot".to_owned())
            }
        };
        let joined = self
            .handle
            .take()
            .is_none_or(|handle| handle.join().is_ok());
        if joined {
            Some(received)
        } else {
            Some(Err("status refresh worker panicked".to_owned()))
        }
    }

    fn cancel_and_join(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for RefreshWorker {
    fn drop(&mut self) {
        self.cancel_and_join();
    }
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    fn enter() -> Result<Self> {
        enable_raw_mode().context("failed to enable terminal raw mode")?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, Hide) {
            let _ = execute!(stdout, Show, LeaveAlternateScreen);
            let _ = disable_raw_mode();
            return Err(error).context("failed to enter the terminal alternate screen");
        }
        let backend = CrosstermBackend::new(stdout);
        match Terminal::new(backend) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                let mut stdout = io::stdout();
                let _ = execute!(stdout, Show, LeaveAlternateScreen);
                let _ = disable_raw_mode();
                Err(error).context("failed to initialize the status terminal")
            }
        }
    }

    fn draw(&mut self, draw: impl FnOnce(&mut ratatui::Frame)) -> io::Result<()> {
        self.terminal.draw(draw).map(|_| ())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.terminal.show_cursor();
        let _ = execute!(self.terminal.backend_mut(), Show, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_requirement_explains_each_redirect_case() {
        assert!(require_terminal(true, true).is_ok());
        assert!(
            require_terminal(false, true)
                .unwrap_err()
                .to_string()
                .contains("terminal input")
        );
        assert!(
            require_terminal(true, false)
                .unwrap_err()
                .to_string()
                .contains("terminal output")
        );
        assert!(
            require_terminal(false, false)
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
            RuntimeAction::Continue
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
}
