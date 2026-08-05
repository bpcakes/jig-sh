use std::fmt;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::time::Instant;

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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{CliProgress, format_duration, leader};

    #[test]
    fn disabled_progress_never_enables_output_or_color() {
        let progress = CliProgress::disabled("test");

        assert!(!progress.enabled);
        assert!(!progress.color);
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
