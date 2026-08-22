use std::fmt;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::execution::{ExecutionCancellation, ExecutionEvent, ExecutionObserver};

const CLI_PROGRESS_OUTPUT_LIMIT: usize = 64 * 1024;
const CLI_PROGRESS_STRUCTURE_LIMIT: usize = 16 * 1024;
const CLI_PROGRESS_DELIVERY_TIMEOUT: Duration = Duration::from_millis(250);

/// Command-scoped terminal progress output.
///
/// Kept `Copy` so one progress value can be passed through request structs while
/// preserving the original elapsed-time origin.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CliProgress {
    command: &'static str,
    enabled: bool,
    color: bool,
    started_at: Instant,
}

impl CliProgress {
    pub(crate) fn new(command: &'static str) -> Self {
        let enabled = io::stderr().is_terminal();
        Self {
            command,
            enabled,
            color: enabled && color_enabled(),
            started_at: Instant::now(),
        }
    }

    pub(crate) fn disabled(command: &'static str) -> Self {
        Self {
            command,
            enabled: false,
            color: false,
            started_at: Instant::now(),
        }
    }

    pub(crate) fn for_human_output(command: &'static str, json_output: bool) -> Self {
        let enabled = !json_output;
        Self {
            command,
            enabled,
            color: enabled && io::stderr().is_terminal() && color_enabled(),
            started_at: Instant::now(),
        }
    }

    pub(crate) fn header(&self, action: impl fmt::Display) {
        if !self.enabled {
            return;
        }
        eprintln!(
            "{} {} | {}",
            self.paint("jig", Style::Strong),
            self.paint(self.command, Style::Accent),
            action
        );
    }

    pub(crate) fn header_for_path(&self, action: impl fmt::Display, destination: &Path) {
        self.header(action);
        self.info("target", destination.display());
    }

    pub(crate) fn info(&self, label: &str, detail: impl fmt::Display) {
        if !self.enabled {
            return;
        }
        self.line(label, detail, Status::Info);
    }

    pub(crate) fn step(&self, label: &str, detail: impl fmt::Display) {
        if !self.enabled {
            return;
        }
        self.line(label, detail, Status::Working);
    }

    pub(crate) fn blocked(&self, detail: impl fmt::Display) {
        if !self.enabled {
            return;
        }
        self.line("blocked", detail, Status::Blocked);
    }

    pub(crate) fn done(&self, detail: impl fmt::Display) {
        if !self.enabled {
            return;
        }
        self.line(
            &detail.to_string(),
            format_duration(self.started_at.elapsed()),
            Status::Ok,
        );
    }

    pub(crate) fn log_blocked_on_err<T, E>(
        &self,
        result: std::result::Result<T, E>,
    ) -> std::result::Result<T, E>
    where
        E: fmt::Display,
    {
        result.inspect_err(|_| {
            self.blocked("operation failed; see error below");
        })
    }

    fn line(&self, label: &str, detail: impl fmt::Display, status: Status) {
        eprintln!(
            "  {} {} {} {}",
            self.status_token(status),
            self.paint(label, Style::Label),
            leader(label),
            self.paint(&detail.to_string(), Style::Detail)
        );
    }

    fn status_token(&self, status: Status) -> String {
        let (token, style) = match status {
            Status::Info => ("[--]", Style::Dim),
            Status::Working => ("[..]", Style::Accent),
            Status::Blocked => ("[!!]", Style::Warn),
            Status::Ok => ("[ok]", Style::Ok),
        };
        self.paint(token, style)
    }

    fn paint(&self, text: &str, style: Style) -> String {
        if !self.color {
            return text.to_string();
        }
        let code = match style {
            Style::Strong => "1",
            Style::Accent => "36",
            Style::Label => "1;37",
            Style::Detail => "2",
            Style::Dim => "90",
            Style::Warn => "33",
            Style::Ok => "32",
        };
        format!("\x1b[{code}m{text}\x1b[0m")
    }
}

#[derive(Clone, Copy)]
enum Status {
    Info,
    Working,
    Blocked,
    Ok,
}

#[derive(Clone, Copy)]
enum Style {
    Strong,
    Accent,
    Label,
    Detail,
    Dim,
    Warn,
    Ok,
}

fn leader(label: &str) -> String {
    let width = 28usize;
    let count = width.saturating_sub(label.len()).max(3);
    ".".repeat(count)
}

fn format_duration(duration: std::time::Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1_000 {
        return format!("{millis}ms");
    }
    format!("{:.1}s", duration.as_secs_f64())
}

fn color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").map_or(true, |term| term != "dumb")
}

pub(crate) struct CliExecutionObserver {
    enabled: bool,
    pending: Vec<ProgressChunk>,
    output_bytes: usize,
    structural_bytes: usize,
    output_truncated: bool,
    structure_truncated: bool,
    output_needs_newline: bool,
    cancellation: Box<dyn Fn() -> bool>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ProgressChunkKind {
    Output,
    Structure,
}

struct ProgressChunk {
    kind: ProgressChunkKind,
    bytes: Vec<u8>,
}

impl CliExecutionObserver {
    #[cfg(any(not(unix), test))]
    pub(crate) fn for_human_output(json_output: bool) -> Self {
        Self {
            enabled: !json_output,
            pending: Vec::new(),
            output_bytes: 0,
            structural_bytes: 0,
            output_truncated: false,
            structure_truncated: false,
            output_needs_newline: false,
            cancellation: Box::new(|| false),
        }
    }

    #[cfg(all(unix, not(test)))]
    pub(crate) fn with_cancellation(
        json_output: bool,
        cancellation: impl Fn() -> bool + 'static,
    ) -> Self {
        Self {
            enabled: !json_output,
            pending: Vec::new(),
            output_bytes: 0,
            structural_bytes: 0,
            output_truncated: false,
            structure_truncated: false,
            output_needs_newline: false,
            cancellation: Box::new(cancellation),
        }
    }

    fn queue_output(&mut self, bytes: &[u8]) {
        if !self.enabled {
            return;
        }
        let remaining = CLI_PROGRESS_OUTPUT_LIMIT.saturating_sub(self.output_bytes);
        let retained = remaining.min(bytes.len());
        self.push_chunk(ProgressChunkKind::Output, &bytes[..retained]);
        self.output_bytes += retained;
        self.output_truncated |= retained < bytes.len();
        if retained > 0 {
            self.output_needs_newline = bytes[retained - 1] != b'\n';
        }
    }

    fn queue_structure(&mut self, bytes: &[u8]) {
        if !self.enabled {
            return;
        }
        let remaining = CLI_PROGRESS_STRUCTURE_LIMIT.saturating_sub(self.structural_bytes);
        let retained = remaining.min(bytes.len());
        self.push_chunk(ProgressChunkKind::Structure, &bytes[..retained]);
        self.structural_bytes += retained;
        self.structure_truncated |= retained < bytes.len();
    }

    fn push_chunk(&mut self, kind: ProgressChunkKind, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if let Some(chunk) = self.pending.last_mut().filter(|chunk| chunk.kind == kind) {
            chunk.bytes.extend_from_slice(bytes);
        } else {
            self.pending.push(ProgressChunk {
                kind,
                bytes: bytes.to_vec(),
            });
        }
    }

    fn line(&mut self, line: String) {
        if self.output_needs_newline {
            self.queue_structure(b"\n");
            self.output_needs_newline = false;
        }
        self.queue_structure(format!("{line}\n").as_bytes());
    }

    pub(crate) fn finish_with<T>(&mut self, outcome: anyhow::Result<T>) -> anyhow::Result<T> {
        let rendered = self.take_rendered();
        finish_rendered_with_timeout(
            outcome,
            rendered,
            io::stderr(),
            CLI_PROGRESS_DELIVERY_TIMEOUT,
        )
    }

    #[cfg(test)]
    fn finish_to(&mut self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(&self.take_rendered())?;
        writer.flush()
    }

    fn take_rendered(&mut self) -> Vec<u8> {
        if !self.enabled {
            return Vec::new();
        }
        let mut rendered = Vec::with_capacity(
            self.output_bytes
                .saturating_add(self.structural_bytes)
                .saturating_add(128),
        );
        for chunk in std::mem::take(&mut self.pending) {
            rendered.extend_from_slice(&chunk.bytes);
        }
        if self.output_truncated {
            if rendered.last().is_some_and(|byte| *byte != b'\n') {
                rendered.push(b'\n');
            }
            rendered.extend_from_slice(b"[..] progress preview truncated after 64 KiB\n");
        }
        if self.structure_truncated {
            if !self.output_truncated && rendered.last().is_some_and(|byte| *byte != b'\n') {
                rendered.push(b'\n');
            }
            rendered.extend_from_slice(b"[..] structural progress truncated after 16 KiB\n");
        }
        self.output_bytes = 0;
        self.structural_bytes = 0;
        self.output_truncated = false;
        self.structure_truncated = false;
        self.output_needs_newline = false;
        rendered
    }
}

fn finish_rendered_with_timeout<T, W>(
    outcome: anyhow::Result<T>,
    rendered: Vec<u8>,
    mut writer: W,
    timeout: Duration,
) -> anyhow::Result<T>
where
    W: Write + Send + 'static,
{
    if rendered.is_empty() {
        return outcome;
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    let writer = std::thread::Builder::new()
        .name("jig-progress-writer".into())
        .spawn(move || {
            let result = writer.write_all(&rendered).and_then(|()| writer.flush());
            let _ = sender.send(result);
        });
    let writer = match writer {
        Ok(writer) => writer,
        Err(error) => return combine_progress_delivery(outcome, Err(error)),
    };
    match receiver.recv_timeout(timeout) {
        Ok(result) => {
            let _ = writer.join();
            combine_progress_delivery(outcome, result)
        }
        Err(mpsc::RecvTimeoutError::Timeout) => outcome,
        Err(mpsc::RecvTimeoutError::Disconnected) => combine_progress_delivery(
            outcome,
            Err(io::Error::other(
                "execution progress writer stopped before reporting its result",
            )),
        ),
    }
}

fn combine_progress_delivery<T>(
    outcome: anyhow::Result<T>,
    delivery: io::Result<()>,
) -> anyhow::Result<T> {
    match (outcome, delivery) {
        (outcome, Ok(())) => outcome,
        (Ok(_), Err(error)) => Err(error.into()),
        (Err(error), Err(delivery_error)) => Err(error.context(format!(
            "Execution progress also failed to flush: {delivery_error}"
        ))),
    }
}

impl ExecutionObserver for CliExecutionObserver {
    fn event(&mut self, event: ExecutionEvent<'_>) {
        match event {
            ExecutionEvent::PhaseStarted { label, position } => self.line(format!(
                "[..] {label} ({}/{})",
                position.current(),
                position.total()
            )),
            ExecutionEvent::Output { stream, bytes } => {
                let _ = stream;
                self.queue_output(bytes);
            }
            ExecutionEvent::Heartbeat { label, elapsed } => {
                self.line(format!("[..] {label} reached {}", format_duration(elapsed)))
            }
            ExecutionEvent::PhaseFinished {
                label,
                success,
                elapsed,
            } => self.line(format!(
                "[{}] {label} ({})",
                if success { "ok" } else { "!!" },
                format_duration(elapsed)
            )),
        }
    }
}

impl ExecutionCancellation for CliExecutionObserver {
    fn cancelled(&self) -> bool {
        (self.cancellation)()
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::time::Duration;

    use crate::execution::{ExecutionEvent, ExecutionObserver, ExecutionStream, PhasePosition};

    use super::{
        CLI_PROGRESS_OUTPUT_LIMIT, CLI_PROGRESS_STRUCTURE_LIMIT, CliExecutionObserver, CliProgress,
        finish_rendered_with_timeout, format_duration, leader,
    };

    #[test]
    fn disabled_progress_never_enables_output_or_color() {
        let progress = CliProgress::disabled("test");

        assert!(!progress.enabled);
        assert!(!progress.color);
    }

    #[test]
    fn human_progress_remains_enabled_when_stderr_is_captured() {
        let progress = CliProgress::for_human_output("test", false);
        let observer = CliExecutionObserver::for_human_output(false);

        assert!(progress.enabled);
        assert!(observer.enabled);
    }

    #[test]
    fn json_output_disables_human_progress() {
        let progress = CliProgress::for_human_output("test", true);
        let observer = CliExecutionObserver::for_human_output(true);

        assert!(!progress.enabled);
        assert!(!observer.enabled);
    }

    #[test]
    fn execution_progress_is_bounded_and_flushed_after_collection() {
        let mut observer = CliExecutionObserver::for_human_output(false);
        observer.event(ExecutionEvent::PhaseStarted {
            label: "fixture",
            position: PhasePosition::single(),
        });
        observer.event(ExecutionEvent::Output {
            stream: ExecutionStream::Stdout,
            bytes: &vec![b'x'; CLI_PROGRESS_OUTPUT_LIMIT * 2],
        });
        observer.event(ExecutionEvent::PhaseFinished {
            label: "fixture",
            success: true,
            elapsed: Duration::from_millis(1),
        });

        assert_eq!(observer.output_bytes, CLI_PROGRESS_OUTPUT_LIMIT);
        assert!(observer.structural_bytes < CLI_PROGRESS_STRUCTURE_LIMIT);
        assert!(observer.output_truncated);
        assert!(!observer.structure_truncated);
        let mut rendered = Vec::new();
        observer.finish_to(&mut rendered).unwrap();
        assert!(rendered.starts_with(b"[..] fixture (1/1)\n"));
        assert!(
            String::from_utf8_lossy(&rendered).contains("[ok] fixture (1ms)"),
            "{}",
            String::from_utf8_lossy(&rendered)
        );
        assert!(
            rendered.ends_with(b"\n[..] progress preview truncated after 64 KiB\n"),
            "{}",
            String::from_utf8_lossy(&rendered)
        );
    }

    #[test]
    fn structural_progress_has_an_independent_bound() {
        let mut observer = CliExecutionObserver::for_human_output(false);

        for _ in 0..CLI_PROGRESS_STRUCTURE_LIMIT {
            observer.event(ExecutionEvent::Heartbeat {
                label: "fixture",
                elapsed: Duration::from_secs(1),
            });
        }

        assert_eq!(observer.structural_bytes, CLI_PROGRESS_STRUCTURE_LIMIT);
        assert!(observer.structure_truncated);
        assert_eq!(observer.output_bytes, 0);
    }

    #[test]
    fn progress_flush_reports_both_operation_and_flush_errors() {
        struct FailingWriter;

        impl Write for FailingWriter {
            fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("fixture sink failed"))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let operation: anyhow::Result<()> = Err(anyhow::anyhow!("fixture operation failed"));
        let error = finish_rendered_with_timeout(
            operation,
            b"pending progress".to_vec(),
            FailingWriter,
            Duration::from_secs(1),
        )
        .unwrap_err();

        let rendered = format!("{error:#}");
        assert!(rendered.starts_with("Execution progress also failed to flush"));
        assert!(rendered.contains("fixture operation failed"));
        assert!(rendered.contains("fixture sink failed"));
    }

    #[test]
    fn progress_delivery_timeout_does_not_block_the_operation_result() {
        struct SlowWriter;

        impl Write for SlowWriter {
            fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
                std::thread::sleep(Duration::from_millis(200));
                Ok(buffer.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let started = std::time::Instant::now();
        let result = finish_rendered_with_timeout(
            Ok(7),
            b"pending progress".to_vec(),
            SlowWriter,
            Duration::from_millis(10),
        )
        .unwrap();

        assert_eq!(result, 7);
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn progress_rendering_is_consuming() {
        let mut observer = CliExecutionObserver::for_human_output(false);
        observer.queue_output(b"one copy\n");
        let mut first = Vec::new();
        let mut second = Vec::new();

        observer.finish_to(&mut first).unwrap();
        observer.finish_to(&mut second).unwrap();

        assert_eq!(first, b"one copy\n");
        assert!(second.is_empty());
    }

    #[test]
    fn leader_keeps_minimum_spacing_for_long_labels() {
        assert_eq!(leader("abcdefghijklmnopqrstuvwxyz"), "...");
    }

    #[test]
    fn leader_pads_short_labels_to_fixed_width() {
        assert_eq!(leader("target"), ".".repeat(22));
    }

    #[test]
    fn format_duration_uses_milliseconds_below_one_second() {
        assert_eq!(format_duration(Duration::from_millis(999)), "999ms");
    }

    #[test]
    fn format_duration_uses_single_decimal_seconds() {
        assert_eq!(format_duration(Duration::from_millis(1250)), "1.2s");
    }
}
