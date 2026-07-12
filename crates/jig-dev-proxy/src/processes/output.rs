use std::collections::VecDeque;
use std::io::{self, IsTerminal, Read, Write};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use terminal_size::{Width, terminal_size_of};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::child_lifecycle::terminate_and_reap_logged;

pub(super) const MAX_APP_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const PROGRESS_REFRESH_INTERVAL: Duration = Duration::from_millis(80);
const OUTPUT_DRAIN_GRACE: Duration = Duration::from_millis(100);
const DEFAULT_TERMINAL_WIDTH: usize = 100;

pub(super) struct CapturedAppOutput {
    app_name: String,
    buffer: Arc<Mutex<TailBuffer>>,
    readers: Vec<OutputReader>,
    progress: LiveAppProgress,
    diagnostics: Vec<String>,
}

impl CapturedAppOutput {
    pub(super) fn from_child(child: &mut Child, app_name: &str) -> Result<Self> {
        let Some(stdout) = child.stdout.take() else {
            cleanup_failed_capture_start(child, None, Vec::new());
            return Err(anyhow::anyhow!("Failed to capture development app stdout"));
        };
        let Some(stderr) = child.stderr.take() else {
            drop(stdout);
            cleanup_failed_capture_start(child, None, Vec::new());
            return Err(anyhow::anyhow!("Failed to capture development app stderr"));
        };
        let buffer = Arc::new(Mutex::new(TailBuffer::default()));
        let mut progress = match LiveAppProgress::new(app_name) {
            Ok(progress) => progress,
            Err(error) => {
                drop((stdout, stderr));
                cleanup_failed_capture_start(child, None, Vec::new());
                return Err(error).context("Failed to start development app progress renderer");
            }
        };
        let stdout_reader =
            match spawn_output_reader(stdout, Arc::clone(&buffer), progress.state(), "stdout") {
                Ok(reader) => reader,
                Err(error) => {
                    drop(stderr);
                    cleanup_failed_capture_start(child, Some(&mut progress), Vec::new());
                    return Err(error).context("Failed to start development app stdout capture");
                }
            };
        let stderr_reader =
            match spawn_output_reader(stderr, Arc::clone(&buffer), progress.state(), "stderr") {
                Ok(reader) => reader,
                Err(error) => {
                    cleanup_failed_capture_start(child, Some(&mut progress), vec![stdout_reader]);
                    return Err(error).context("Failed to start development app stderr capture");
                }
            };
        Ok(Self {
            app_name: app_name.to_string(),
            buffer,
            readers: vec![stdout_reader, stderr_reader],
            progress,
            diagnostics: Vec::new(),
        })
    }

    pub(super) fn finish_progress(&mut self) {
        if let Some(diagnostic) = self.progress.finish() {
            self.diagnostics.push(diagnostic);
        }
    }

    pub(super) fn print_failure(&mut self, app_name: &str) {
        self.finish_progress();
        self.finish_readers();
        let buffer = match self.buffer.lock() {
            Ok(buffer) => buffer,
            Err(_) => {
                self.diagnostics
                    .push("capture buffer mutex was poisoned".to_string());
                print_capture_diagnostics(app_name, &self.diagnostics);
                eprintln!("App '{app_name}' failed; its captured output could not be read");
                return;
            }
        };
        let bytes = buffer.bytes.iter().copied().collect::<Vec<_>>();
        let truncated = buffer.truncated;
        drop(buffer);
        print_capture_diagnostics(app_name, &self.diagnostics);
        if bytes.is_empty() {
            return;
        }
        eprintln!("--- output from app '{app_name}' ---");
        if truncated {
            eprintln!("[earlier output omitted; showing the last 2 MiB]");
        }
        eprint!("{}", String::from_utf8_lossy(&bytes));
        if bytes.last() != Some(&b'\n') {
            eprintln!();
        }
        eprintln!("--- end output from app '{app_name}' ---");
    }

    pub(super) fn discard(&mut self) {
        self.finish_progress();
        self.finish_readers();
        print_capture_diagnostics(&self.app_name, &self.diagnostics);
    }

    fn finish_readers(&mut self) {
        // A wrapper may exit while a grandchild keeps inherited output pipes
        // open. Give completed readers a brief chance to flush, but never let
        // failure reporting hang on such a grandchild.
        finish_output_readers(&mut self.readers, &mut self.diagnostics);
    }

    #[cfg(test)]
    pub(super) fn captured_bytes(&mut self) -> Vec<u8> {
        self.finish_progress();
        self.finish_readers();
        self.buffer.lock().unwrap().bytes.iter().copied().collect()
    }
}

type ReaderResult = std::result::Result<(), String>;

fn cleanup_failed_capture_start(
    child: &mut Child,
    progress: Option<&mut LiveAppProgress>,
    mut readers: Vec<OutputReader>,
) {
    terminate_and_reap_logged(
        child,
        "could not clean up after output-capture startup failure",
    );
    if let Some(progress) = progress {
        let _ = progress.finish();
    }
    let mut diagnostics = Vec::new();
    finish_output_readers(&mut readers, &mut diagnostics);
    for diagnostic in diagnostics {
        eprintln!("Development app output capture startup was incomplete: {diagnostic}");
    }
}

struct OutputReader {
    stream_name: &'static str,
    handle: thread::JoinHandle<ReaderResult>,
}

fn reader_diagnostic(reader: OutputReader) -> Option<String> {
    match reader.handle.join() {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(_) => Some(format!(
            "{} capture thread panicked; output may be incomplete",
            reader.stream_name
        )),
    }
}

fn finish_output_readers(readers: &mut Vec<OutputReader>, diagnostics: &mut Vec<String>) {
    let deadline = Instant::now() + OUTPUT_DRAIN_GRACE;
    while readers.iter().any(|reader| !reader.handle.is_finished()) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    for reader in readers.drain(..) {
        if reader.handle.is_finished() {
            if let Some(diagnostic) = reader_diagnostic(reader) {
                diagnostics.push(diagnostic);
            }
        } else {
            diagnostics.push(format!(
                "{} capture did not finish within {:?}; output may be incomplete",
                reader.stream_name, OUTPUT_DRAIN_GRACE
            ));
            // Dropping a JoinHandle detaches the still-blocked reader. The
            // child process group has already been terminated on cleanup
            // paths, but a descendant may retain an inherited pipe forever;
            // never turn capture cleanup into an unbounded join.
        }
    }
}

fn print_capture_diagnostics(app_name: &str, diagnostics: &[String]) {
    for diagnostic in diagnostics {
        eprintln!("App '{app_name}' output capture was incomplete: {diagnostic}");
    }
}

struct LiveAppProgress {
    state: Arc<ProgressState>,
    renderer: Option<thread::JoinHandle<()>>,
}

impl LiveAppProgress {
    fn new(app_name: &str) -> Result<Self> {
        let enabled = io::stderr().is_terminal();
        let app_name = sanitize_terminal_text(app_name);
        let state = Arc::new(ProgressState {
            app_name: truncate_display(
                if app_name.trim().is_empty() {
                    "app"
                } else {
                    app_name.trim()
                },
                24,
            ),
            detail: Mutex::new("starting".to_string()),
            stopped: AtomicBool::new(false),
            color: enabled && color_enabled(),
            started_at: Instant::now(),
        });
        let renderer = if enabled {
            let state = Arc::clone(&state);
            Some(
                thread::Builder::new()
                    .name("jig-app-progress".to_string())
                    .spawn(move || render_progress_until_stopped(state))?,
            )
        } else {
            None
        };
        Ok(Self { state, renderer })
    }

    fn state(&self) -> Arc<ProgressState> {
        Arc::clone(&self.state)
    }

    fn finish(&mut self) -> Option<String> {
        if self.state.stopped.swap(true, Ordering::SeqCst) {
            return None;
        }
        let mut diagnostic = None;
        if let Some(renderer) = self.renderer.take() {
            if renderer.join().is_err() {
                diagnostic = Some("progress renderer thread panicked".to_string());
            }
            clear_progress_line();
        }
        diagnostic
    }
}

impl Drop for LiveAppProgress {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

struct ProgressState {
    app_name: String,
    detail: Mutex<String>,
    stopped: AtomicBool,
    color: bool,
    started_at: Instant,
}

impl ProgressState {
    fn update(&self, line: &[u8]) {
        if self.stopped.load(Ordering::Relaxed) {
            return;
        }
        let Some(detail) = progress_detail(&String::from_utf8_lossy(line)) else {
            return;
        };
        if let Ok(mut current) = self.detail.lock() {
            *current = detail;
        }
    }
}

fn render_progress_until_stopped(state: Arc<ProgressState>) {
    let frames = ["[. ]", "[..]", "[ .]", "[..]"];
    let mut frame = 0usize;
    while !state.stopped.load(Ordering::Relaxed) {
        render_progress(&state, frames[frame % frames.len()]);
        frame += 1;
        thread::sleep(PROGRESS_REFRESH_INTERVAL);
    }
}

fn render_progress(state: &ProgressState, frame: &str) {
    let detail = state
        .detail
        .lock()
        .map(|detail| detail.clone())
        .unwrap_or_else(|_| "working".to_string());
    let elapsed = format!("({:.1}s)", state.started_at.elapsed().as_secs_f64());
    let terminal_width = terminal_width();
    let app_name = truncate_display(
        &state.app_name,
        terminal_width.saturating_sub(UnicodeWidthStr::width(elapsed.as_str()) + 12),
    );
    let fixed_width =
        UnicodeWidthStr::width(app_name.as_str()) + UnicodeWidthStr::width(elapsed.as_str()) + 11;
    let detail = truncate_display(&detail, terminal_width.saturating_sub(fixed_width));
    let line = if state.color {
        format!(
            "\x1b[36m{frame}\x1b[0m \x1b[1;37m{}\x1b[0m \x1b[90m·\x1b[0m {detail} \x1b[90m{elapsed}\x1b[0m",
            app_name
        )
    } else {
        format!("{frame} {app_name} · {detail} {elapsed}")
    };
    let mut stderr = io::stderr().lock();
    let _ = write!(stderr, "\r\x1b[2K  {line}");
    let _ = stderr.flush();
}

fn clear_progress_line() {
    let mut stderr = io::stderr().lock();
    let _ = write!(stderr, "\r\x1b[2K");
    let _ = stderr.flush();
}

fn progress_detail(line: &str) -> Option<String> {
    let stripped = sanitize_terminal_text(line);
    let line = stripped.trim();
    if line.is_empty()
        || line.starts_with('|')
        || line.starts_with('^')
        || line.starts_with("-->")
        || line.starts_with("= note:")
        || source_gutter(line)
    {
        return None;
    }
    let detail = line.split_whitespace().collect::<Vec<_>>().join(" ");
    (!detail.is_empty()).then_some(detail)
}

fn source_gutter(line: &str) -> bool {
    line.split_once('|')
        .is_some_and(|(prefix, _)| prefix.trim().parse::<usize>().is_ok())
}

fn sanitize_terminal_text(value: &str) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Text,
        Escape,
        ControlSequence,
        OperatingSystemCommand,
        OperatingSystemCommandEscape,
    }

    let mut output = String::with_capacity(value.len());
    let mut state = State::Text;
    for character in value.chars() {
        state = match state {
            State::Text if character == '\x1b' => State::Escape,
            State::Text => {
                if !character.is_control() {
                    output.push(character);
                }
                State::Text
            }
            State::Escape if character == '[' => State::ControlSequence,
            State::Escape if matches!(character, ']' | 'P' | '^' | '_') => {
                State::OperatingSystemCommand
            }
            State::Escape => State::Text,
            State::ControlSequence if ('@'..='~').contains(&character) => State::Text,
            State::ControlSequence => State::ControlSequence,
            State::OperatingSystemCommand if character == '\x07' => State::Text,
            State::OperatingSystemCommand if character == '\x1b' => {
                State::OperatingSystemCommandEscape
            }
            State::OperatingSystemCommand => State::OperatingSystemCommand,
            State::OperatingSystemCommandEscape if character == '\\' => State::Text,
            State::OperatingSystemCommandEscape => State::OperatingSystemCommand,
        };
    }
    output
}

fn truncate_display(value: &str, width: usize) -> String {
    if UnicodeWidthStr::width(value) <= width {
        return value.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let content_width = width.saturating_sub(UnicodeWidthChar::width('…').unwrap_or(1));
    let mut output = String::new();
    let mut used = 0;
    for character in value.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width > content_width {
            break;
        }
        output.push(character);
        used += character_width;
    }
    output.push('…');
    output
}

fn terminal_width() -> usize {
    terminal_size_of(io::stderr())
        .map(|(Width(width), _)| usize::from(width))
        .filter(|width| *width > 0)
        .or_else(|| {
            std::env::var("COLUMNS")
                .ok()
                .and_then(|value| value.parse().ok())
                .filter(|width| *width > 0)
        })
        .unwrap_or(DEFAULT_TERMINAL_WIDTH)
}

fn color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").map_or(true, |term| term != "dumb")
}

fn spawn_output_reader(
    mut output: impl Read + Send + 'static,
    buffer: Arc<Mutex<TailBuffer>>,
    progress: Arc<ProgressState>,
    stream_name: &'static str,
) -> io::Result<OutputReader> {
    let handle = thread::Builder::new()
        .name(format!("jig-app-{stream_name}"))
        .spawn(move || {
            let mut chunk = [0_u8; 8192];
            let mut progress_line = Vec::new();
            loop {
                match output.read(&mut chunk) {
                    Ok(0) => {
                        progress.update(&progress_line);
                        return Ok(());
                    }
                    Err(error) => return Err(format!("{stream_name} read failed: {error}")),
                    Ok(read) => {
                        for byte in &chunk[..read] {
                            if matches!(byte, b'\n' | b'\r') {
                                progress.update(&progress_line);
                                progress_line.clear();
                            } else if progress_line.len() < 8192 {
                                progress_line.push(*byte);
                            }
                        }
                        let Ok(mut buffer) = buffer.lock() else {
                            return Err(format!("{stream_name} capture buffer mutex was poisoned"));
                        };
                        buffer.push(&chunk[..read]);
                    }
                }
            }
        })?;
    Ok(OutputReader {
        stream_name,
        handle,
    })
}

#[derive(Default)]
pub(super) struct TailBuffer {
    pub(super) bytes: VecDeque<u8>,
    pub(super) truncated: bool,
}

impl TailBuffer {
    pub(super) fn push(&mut self, bytes: &[u8]) {
        if bytes.len() >= MAX_APP_OUTPUT_BYTES {
            self.bytes.clear();
            self.bytes
                .extend(&bytes[bytes.len() - MAX_APP_OUTPUT_BYTES..]);
            self.truncated = true;
            return;
        }
        let overflow = self
            .bytes
            .len()
            .saturating_add(bytes.len())
            .saturating_sub(MAX_APP_OUTPUT_BYTES);
        if overflow > 0 {
            self.bytes.drain(..overflow);
            self.truncated = true;
        }
        self.bytes.extend(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_detail_cleans_compiler_output() {
        assert_eq!(
            progress_detail("   Compiling creditkit-http v0.1.0\n").as_deref(),
            Some("Compiling creditkit-http v0.1.0")
        );
        assert_eq!(progress_detail("   |\n"), None);
        assert_eq!(progress_detail("19 | unused();\n"), None);
        assert_eq!(progress_detail("   = note: details\n"), None);
    }

    #[test]
    fn truncate_marks_clipped_text_by_display_width() {
        assert_eq!(truncate_display("Compiling", 6), "Compi…");
        assert_eq!(truncate_display("short", 6), "short");
        assert_eq!(truncate_display("编译 crate", 7), "编译 c…");
        assert_eq!(
            UnicodeWidthStr::width(truncate_display("编译 crate", 7).as_str()),
            7
        );
        assert_eq!(truncate_display("e\u{301}clair", 2), "e\u{301}…");
    }

    #[test]
    fn terminal_text_removes_color_titles_and_controls() {
        assert_eq!(
            sanitize_terminal_text("\x1b[31mCompile\x1b[0m\x07 safe"),
            "Compile safe"
        );
        assert_eq!(
            sanitize_terminal_text("before\x1b]0;spoofed title\x07after"),
            "beforeafter"
        );
    }

    #[test]
    fn reader_reports_io_failure_instead_of_treating_it_as_eof() {
        struct FailingReader;
        impl Read for FailingReader {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "fixture failure"))
            }
        }
        let state = Arc::new(ProgressState {
            app_name: "test".into(),
            detail: Mutex::new("starting".into()),
            stopped: AtomicBool::new(false),
            color: false,
            started_at: Instant::now(),
        });
        let reader = spawn_output_reader(
            FailingReader,
            Arc::new(Mutex::new(TailBuffer::default())),
            state,
            "stdout",
        )
        .unwrap();
        assert_eq!(
            reader_diagnostic(reader).as_deref(),
            Some("stdout read failed: fixture failure")
        );
    }

    #[test]
    fn reader_diagnostic_reports_panics() {
        let reader = OutputReader {
            stream_name: "stderr",
            handle: thread::spawn(|| -> ReaderResult { panic!("fixture panic") }),
        };
        assert_eq!(
            reader_diagnostic(reader).as_deref(),
            Some("stderr capture thread panicked; output may be incomplete")
        );
    }

    #[test]
    fn unfinished_reader_is_bounded_and_reports_partial_capture() {
        struct BlockingReader(std::sync::mpsc::Receiver<()>);

        impl Read for BlockingReader {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                let _ = self.0.recv();
                Ok(0)
            }
        }

        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let state = Arc::new(ProgressState {
            app_name: "test".into(),
            detail: Mutex::new("starting".into()),
            stopped: AtomicBool::new(false),
            color: false,
            started_at: Instant::now(),
        });
        let mut readers = vec![
            spawn_output_reader(
                BlockingReader(release_rx),
                Arc::new(Mutex::new(TailBuffer::default())),
                state,
                "stdout",
            )
            .unwrap(),
        ];
        let mut diagnostics = Vec::new();
        let started = Instant::now();

        finish_output_readers(&mut readers, &mut diagnostics);

        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(readers.is_empty());
        assert_eq!(
            diagnostics,
            [format!(
                "stdout capture did not finish within {:?}; output may be incomplete",
                OUTPUT_DRAIN_GRACE
            )]
        );
        let _ = release_tx.send(());
    }
}
