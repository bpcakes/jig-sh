//! Shared terminal lifecycle and cooperative worker foundations for Jig TUIs.

use std::{
    io::{self, IsTerminal, Stdout, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread::{self, JoinHandle},
};

use anyhow::{Context, Result, bail};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{DisableBracketedPaste, EnableBracketedPaste, KeyEvent, KeyEventKind},
    execute,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};
use ratatui::{Terminal, backend::CrosstermBackend};

/// Requires both terminal input and output for a full-screen interface.
pub fn require_terminal(command: &str, fallback: &str) -> Result<()> {
    require_terminal_with_state(
        command,
        fallback,
        io::stdin().is_terminal(),
        io::stdout().is_terminal(),
    )
}

/// Testable terminal requirement check with explicit input/output state.
pub fn require_terminal_with_state(
    command: &str,
    fallback: &str,
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
) -> Result<()> {
    match (stdin_is_terminal, stdout_is_terminal) {
        (true, true) => Ok(()),
        (false, false) => {
            bail!("`{command}` requires terminal input and output; {fallback}")
        }
        (false, true) => bail!("`{command}` requires terminal input; {fallback}"),
        (true, false) => bail!("`{command}` requires terminal output; {fallback}"),
    }
}

/// Returns true for key presses and repeats, excluding release-only events.
pub const fn is_actionable_key(key: KeyEvent) -> bool {
    matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

/// Replaces terminal control and unsafe directional-format characters in display text.
///
/// Exact machine-readable values should remain unchanged; use this only at a human-facing
/// terminal boundary.
pub fn sanitize_text(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() || is_unsafe_format_character(character) {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect()
}

fn is_unsafe_format_character(character: char) -> bool {
    matches!(
        character,
        '\u{00ad}'
            | '\u{061c}'
            | '\u{180e}'
            | '\u{200b}'
            | '\u{200e}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}'
            | '\u{e0001}'
            | '\u{e0020}'..='\u{e007f}'
    )
}

/// Owns raw mode, alternate-screen state, cursor visibility, and restoration.
pub struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    bracketed_paste: bool,
}

impl TerminalSession {
    /// Enters raw mode and the alternate screen.
    pub fn enter(label: &str) -> Result<Self> {
        Self::enter_with_options(label, false)
    }

    /// Enters raw mode and the alternate screen with bracketed paste events.
    /// Paste mode is paired with restoration on every return and unwind path.
    pub fn enter_with_bracketed_paste(label: &str) -> Result<Self> {
        Self::enter_with_options(label, true)
    }

    fn enter_with_options(label: &str, bracketed_paste: bool) -> Result<Self> {
        enable_raw_mode().with_context(|| format!("failed to enable {label} terminal raw mode"))?;
        let mut stdout = io::stdout();
        let enter_result = if bracketed_paste {
            execute!(stdout, EnterAlternateScreen, Hide, EnableBracketedPaste)
        } else {
            execute!(stdout, EnterAlternateScreen, Hide)
        };
        if let Err(error) = enter_result {
            let _ = execute!(stdout, DisableBracketedPaste, Show, LeaveAlternateScreen);
            let _ = disable_raw_mode();
            return Err(error).with_context(|| format!("failed to enter the {label} terminal"));
        }
        let backend = CrosstermBackend::new(stdout);
        match Terminal::new(backend) {
            Ok(terminal) => Ok(Self {
                terminal,
                bracketed_paste,
            }),
            Err(error) => {
                let mut stdout = io::stdout();
                if bracketed_paste {
                    let _ = execute!(stdout, DisableBracketedPaste, Show, LeaveAlternateScreen);
                } else {
                    let _ = execute!(stdout, Show, LeaveAlternateScreen);
                }
                let _ = disable_raw_mode();
                Err(error).with_context(|| format!("failed to initialize the {label} terminal"))
            }
        }
    }

    /// Draws one frame.
    pub fn draw(&mut self, draw: impl FnOnce(&mut ratatui::Frame)) -> io::Result<()> {
        self.terminal.draw(draw).map(|_| ())
    }

    /// Clears Ratatui's frame and lends the alternate-screen writer to one
    /// immediate output operation. The caller must not retain the writer.
    pub fn with_direct_output<T>(
        &mut self,
        operation: impl FnOnce(&mut dyn io::Write) -> io::Result<T>,
    ) -> io::Result<T> {
        self.terminal.clear()?;
        execute!(self.terminal.backend_mut(), MoveTo(0, 0), Show)?;
        let result = operation(self.terminal.backend_mut())?;
        self.terminal.backend_mut().flush()?;
        Ok(result)
    }

    /// Erases direct alternate-screen output and invalidates Ratatui's back
    /// buffer so the next frame is redrawn in full.
    pub fn clear_direct_output(&mut self) -> io::Result<()> {
        execute!(
            self.terminal.backend_mut(),
            Clear(ClearType::All),
            MoveTo(0, 0),
            Hide
        )?;
        self.terminal.clear()?;
        self.terminal.backend_mut().flush()
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.terminal.show_cursor();
        if self.bracketed_paste {
            let _ = execute!(
                self.terminal.backend_mut(),
                DisableBracketedPaste,
                Clear(ClearType::All),
                MoveTo(0, 0),
                Show,
                LeaveAlternateScreen
            );
        } else {
            let _ = execute!(
                self.terminal.backend_mut(),
                Clear(ClearType::All),
                MoveTo(0, 0),
                Show,
                LeaveAlternateScreen
            );
        }
        let _ = disable_raw_mode();
    }
}

/// Cloneable cooperative cancellation state shared with an owned worker.
#[derive(Clone, Debug)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Reports whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// A named worker that returns one result and is cancelled and joined on drop.
pub struct CooperativeWorker<T> {
    label: String,
    cancellation: CancellationToken,
    receiver: Receiver<T>,
    handle: Option<JoinHandle<()>>,
}

impl<T: Send + 'static> CooperativeWorker<T> {
    /// Starts one named worker thread.
    pub fn spawn(
        label: impl Into<String>,
        work: impl FnOnce(CancellationToken) -> T + Send + 'static,
    ) -> Result<Self> {
        let label = label.into();
        let cancellation = CancellationToken {
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        let worker_cancellation = cancellation.clone();
        let (sender, receiver) = mpsc::channel();
        let handle = thread::Builder::new()
            .name(label.clone())
            .spawn(move || {
                let result = work(worker_cancellation);
                let _ = sender.send(result);
            })
            .with_context(|| format!("failed to start the {label} worker"))?;
        Ok(Self {
            label,
            cancellation,
            receiver,
            handle: Some(handle),
        })
    }

    /// Returns the worker result when available without blocking.
    pub fn try_finish(&mut self) -> Option<std::result::Result<T, String>> {
        self.handle.as_ref()?;
        let result = match self.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                return Some(Err(if self.join() {
                    format!("{} worker ended without returning a result", self.label)
                } else {
                    format!("{} worker panicked", self.label)
                }));
            }
        };
        let joined = self.join();
        if joined {
            Some(Ok(result))
        } else {
            Some(Err(format!("{} worker panicked", self.label)))
        }
    }

    /// Requests cancellation and joins the worker.
    pub fn cancel_and_join(&mut self) {
        self.cancellation.cancelled.store(true, Ordering::SeqCst);
        self.join();
    }

    fn join(&mut self) -> bool {
        self.handle
            .take()
            .is_none_or(|handle| handle.join().is_ok())
    }
}

impl<T> Drop for CooperativeWorker<T> {
    fn drop(&mut self) {
        self.cancellation.cancelled.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, time::Duration};

    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    use super::*;

    #[test]
    fn terminal_requirement_explains_redirected_streams() {
        assert!(require_terminal_with_state("jig demo", "use --json", true, true).is_ok());
        for (stdin, stdout, expected) in [
            (false, false, "terminal input and output"),
            (false, true, "terminal input"),
            (true, false, "terminal output"),
        ] {
            let error = require_terminal_with_state("jig demo", "use --json", stdin, stdout)
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "{error}");
            assert!(error.contains("use --json"), "{error}");
        }
    }

    #[test]
    fn actionable_keys_include_press_and_repeat_only() {
        assert!(is_actionable_key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE
        )));
        assert!(is_actionable_key(KeyEvent::new_with_kind(
            KeyCode::Down,
            KeyModifiers::NONE,
            KeyEventKind::Repeat
        )));
        assert!(!is_actionable_key(KeyEvent::new_with_kind(
            KeyCode::Down,
            KeyModifiers::NONE,
            KeyEventKind::Release
        )));
    }

    #[test]
    fn terminal_text_sanitizer_replaces_controls_and_unsafe_formatting() {
        assert_eq!(
            sanitize_text("safe\u{1b}[31m\u{202e}text\u{2069}\u{200c}\u{200d}"),
            "safe\u{fffd}[31m\u{fffd}text\u{fffd}\u{200c}\u{200d}"
        );
    }

    #[test]
    fn cooperative_worker_returns_results_and_joins() {
        let mut worker = CooperativeWorker::spawn("test-result", |_| 42).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            if let Some(result) = worker.try_finish() {
                assert_eq!(result.unwrap(), 42);
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(worker.try_finish().is_none());
    }

    #[test]
    fn cooperative_worker_observes_cancellation_before_join() {
        let (sender, receiver) = mpsc::channel();
        let mut worker = CooperativeWorker::spawn("test-cancel", move |cancelled| {
            while !cancelled.is_cancelled() {
                std::thread::yield_now();
            }
            sender.send(()).unwrap();
        })
        .unwrap();

        worker.cancel_and_join();
        receiver.recv_timeout(Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn cooperative_worker_reports_panics() {
        let mut worker = CooperativeWorker::spawn("test-panic", |_| -> () {
            panic!("expected worker panic");
        })
        .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            if let Some(result) = worker.try_finish() {
                assert_eq!(result.unwrap_err(), "test-panic worker panicked");
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}
