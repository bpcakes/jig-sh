use std::io::{self, Write};

use zeroize::Zeroizing;

pub(crate) const PEEK_BEGIN_MARKER: &str = "BEGIN CONTROLLED VAULT PEEK";
pub(crate) const PEEK_END_MARKER: &str = "END CONTROLLED VAULT PEEK";
pub(crate) const MAX_PEEK_SOURCE_BYTES: usize = 4 * 1024;

/// Immediate terminal sink that emits no control byte from the source value.
///
/// The source is consumed without retaining it. Printable Unicode is written
/// directly, while terminal controls, directional formatting, invalid UTF-8,
/// and backslashes use visible escapes. Only a bounded source prefix reaches
/// the terminal, but every source byte is reported as consumed so the core
/// reveal lifecycle can finish truthfully.
pub(crate) struct TerminalSafePreviewWriter<'a> {
    output: &'a mut dyn Write,
    source_bytes: usize,
    previewed_source_bytes: usize,
}

impl<'a> TerminalSafePreviewWriter<'a> {
    pub(crate) fn new(output: &'a mut dyn Write) -> Self {
        Self {
            output,
            source_bytes: 0,
            previewed_source_bytes: 0,
        }
    }

    pub(crate) fn finish(&mut self) -> io::Result<()> {
        if self.source_bytes > self.previewed_source_bytes {
            write!(
                self.output,
                "\r\n… preview limited to {} of {} source bytes",
                self.previewed_source_bytes, self.source_bytes
            )?;
        }
        self.output.flush()
    }

    #[cfg(test)]
    pub(crate) const fn source_bytes(&self) -> usize {
        self.source_bytes
    }

    fn write_preview(&mut self, mut bytes: &[u8]) -> io::Result<()> {
        while !bytes.is_empty() {
            match std::str::from_utf8(bytes) {
                Ok(text) => {
                    self.write_valid_text(text)?;
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    if valid > 0 {
                        // SAFETY: `Utf8Error::valid_up_to` guarantees this
                        // prefix is valid UTF-8.
                        let text = unsafe { std::str::from_utf8_unchecked(&bytes[..valid]) };
                        self.write_valid_text(text)?;
                    }
                    let invalid = error.error_len().unwrap_or(bytes.len() - valid);
                    for byte in &bytes[valid..valid + invalid] {
                        self.write_escaped_byte(*byte)?;
                    }
                    bytes = &bytes[valid + invalid..];
                }
            }
        }
        Ok(())
    }

    fn write_valid_text(&mut self, text: &str) -> io::Result<()> {
        for character in text.chars() {
            match character {
                '\0' => self.output.write_all(br"\0")?,
                '\n' => self.output.write_all(br"\n")?,
                '\r' => self.output.write_all(br"\r")?,
                '\t' => self.output.write_all(br"\t")?,
                '\\' => self.output.write_all(br"\\")?,
                character if terminal_safe_character(character) => {
                    let mut encoded = Zeroizing::new([0_u8; 4]);
                    self.output
                        .write_all(character.encode_utf8(&mut *encoded).as_bytes())?;
                }
                character => {
                    let mut encoded = Zeroizing::new([0_u8; 4]);
                    let encoded = character.encode_utf8(&mut *encoded).as_bytes();
                    for byte in encoded {
                        self.write_escaped_byte(*byte)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn write_escaped_byte(&mut self, byte: u8) -> io::Result<()> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let escaped = Zeroizing::new([
            b'\\',
            b'x',
            HEX[(byte >> 4) as usize],
            HEX[(byte & 15) as usize],
        ]);
        self.output.write_all(&escaped[..])
    }
}

impl Write for TerminalSafePreviewWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = MAX_PEEK_SOURCE_BYTES.saturating_sub(self.previewed_source_bytes);
        let preview_len = bytes.len().min(remaining);
        self.write_preview(&bytes[..preview_len])?;
        self.previewed_source_bytes += preview_len;
        self.source_bytes = self.source_bytes.saturating_add(bytes.len());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

fn terminal_safe_character(character: char) -> bool {
    !character.is_control()
        && !matches!(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_preview_preserves_printable_text_and_escapes_unsafe_bytes() {
        let mut output = Vec::new();
        let source = "hello\\world\n\t\0\u{1b}[31mé\u{202e}";
        let mut bytes = source.as_bytes().to_vec();
        bytes.push(0xff);
        {
            let mut writer = TerminalSafePreviewWriter::new(&mut output);
            writer.write_all(&bytes).unwrap();
            writer.finish().unwrap();
            assert_eq!(writer.source_bytes(), bytes.len());
        }

        let output = String::from_utf8(output).unwrap();
        assert_eq!(
            output,
            "hello\\\\world\\n\\t\\0\\x1b[31mé\\xe2\\x80\\xae\\xff"
        );
        assert!(!output.contains('\n'));
        assert!(!output.contains('\t'));
        assert!(!output.contains('\u{1b}'));
        assert!(!output.contains('\u{202e}'));
    }

    #[test]
    fn terminal_preview_consumes_but_does_not_display_beyond_the_bound() {
        let source = vec![b'a'; MAX_PEEK_SOURCE_BYTES + 17];
        let mut output = Vec::new();
        {
            let mut writer = TerminalSafePreviewWriter::new(&mut output);
            writer.write_all(&source).unwrap();
            writer.finish().unwrap();
            assert_eq!(writer.source_bytes(), source.len());
        }

        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with(&"a".repeat(MAX_PEEK_SOURCE_BYTES)));
        assert!(output.ends_with("preview limited to 4096 of 4113 source bytes"));
        assert_eq!(output.matches('a').count(), MAX_PEEK_SOURCE_BYTES);
    }
}
