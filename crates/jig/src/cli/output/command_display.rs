use jig_tui::sanitize_text;

/// Tracks differences between command values and their safe human display.
#[derive(Default)]
pub(super) struct CommandDisplay {
    sanitized: bool,
}

impl CommandDisplay {
    pub(super) fn text(&mut self, value: &str) -> String {
        let sanitized = sanitize_text(value);
        self.sanitized |= sanitized != value;
        sanitized
    }

    pub(super) fn finish(self, mut output: String, representation_lossy: bool) -> String {
        if representation_lossy {
            output.push_str("\n  Warning: command contains non-UTF-8 values; display is lossy");
        }
        if self.sanitized {
            output.push_str("\n  Warning: terminal controls were replaced; displayed command is not launch-equivalent");
        }
        output
    }
}
