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
    pending_utf8: Zeroizing<[u8; 4]>,
    pending_utf8_len: usize,
    truncated_boundary_prefix_len: Option<usize>,
}

impl<'a> TerminalSafePreviewWriter<'a> {
    pub(crate) fn new(output: &'a mut dyn Write) -> Self {
        Self {
            output,
            source_bytes: 0,
            previewed_source_bytes: 0,
            pending_utf8: Zeroizing::new([0; 4]),
            pending_utf8_len: 0,
            truncated_boundary_prefix_len: None,
        }
    }

    pub(crate) fn finish(&mut self) -> io::Result<()> {
        self.flush_pending_utf8()?;
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

    fn write_preview(&mut self, bytes: &[u8]) -> io::Result<()> {
        debug_assert!(self.truncated_boundary_prefix_len.is_none());
        for byte in bytes {
            debug_assert!(self.pending_utf8_len < self.pending_utf8.len());
            self.pending_utf8[self.pending_utf8_len] = *byte;
            self.pending_utf8_len += 1;
            self.flush_decodable_utf8()?;
        }
        Ok(())
    }

    fn flush_decodable_utf8(&mut self) -> io::Result<()> {
        loop {
            if self.pending_utf8_len == 0 {
                return Ok(());
            }
            let (valid_len, invalid_len) =
                match std::str::from_utf8(&self.pending_utf8[..self.pending_utf8_len]) {
                    Ok(_) => (self.pending_utf8_len, None),
                    Err(error) => (error.valid_up_to(), error.error_len()),
                };
            if valid_len > 0 {
                let bytes = self.take_pending_prefix(valid_len);
                let text = std::str::from_utf8(&bytes[..valid_len])
                    .expect("Utf8Error::valid_up_to always ends at a valid UTF-8 boundary");
                self.write_valid_text(text)?;
                continue;
            }
            let Some(invalid_len) = invalid_len else {
                return Ok(());
            };
            let bytes = self.take_pending_prefix(invalid_len);
            for byte in &bytes[..invalid_len] {
                self.write_escaped_byte(*byte)?;
            }
        }
    }

    fn resolve_truncated_utf8_boundary(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.pending_utf8_len == 0 {
            return Ok(());
        }
        let prefix_len = *self
            .truncated_boundary_prefix_len
            .get_or_insert(self.pending_utf8_len);
        for byte in bytes {
            debug_assert!(self.pending_utf8_len < self.pending_utf8.len());
            self.pending_utf8[self.pending_utf8_len] = *byte;
            self.pending_utf8_len += 1;
            match std::str::from_utf8(&self.pending_utf8[..self.pending_utf8_len]) {
                Ok(_) => {
                    self.clear_pending_utf8();
                    return Ok(());
                }
                Err(error) if error.error_len().is_some() => {
                    let prefix = self.copy_pending_prefix(prefix_len);
                    self.clear_pending_utf8();
                    for byte in &prefix[..prefix_len] {
                        self.write_escaped_byte(*byte)?;
                    }
                    return Ok(());
                }
                Err(_) => {}
            }
        }
        Ok(())
    }

    fn flush_pending_utf8(&mut self) -> io::Result<()> {
        let visible_len = self
            .truncated_boundary_prefix_len
            .unwrap_or(self.pending_utf8_len)
            .min(self.pending_utf8_len);
        let pending = self.copy_pending_prefix(visible_len);
        self.clear_pending_utf8();
        for byte in &pending[..visible_len] {
            self.write_escaped_byte(*byte)?;
        }
        Ok(())
    }

    fn copy_pending_prefix(&self, len: usize) -> Zeroizing<[u8; 4]> {
        let mut bytes = Zeroizing::new([0; 4]);
        bytes[..len].copy_from_slice(&self.pending_utf8[..len]);
        bytes
    }

    fn take_pending_prefix(&mut self, len: usize) -> Zeroizing<[u8; 4]> {
        let bytes = self.copy_pending_prefix(len);
        self.pending_utf8.copy_within(len..self.pending_utf8_len, 0);
        let remaining = self.pending_utf8_len - len;
        self.pending_utf8[remaining..self.pending_utf8_len].fill(0);
        self.pending_utf8_len = remaining;
        bytes
    }

    fn clear_pending_utf8(&mut self) {
        self.pending_utf8.fill(0);
        self.pending_utf8_len = 0;
        self.truncated_boundary_prefix_len = None;
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
        if preview_len < bytes.len() {
            self.resolve_truncated_utf8_boundary(&bytes[preview_len..])?;
        }
        self.source_bytes = self.source_bytes.saturating_add(bytes.len());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_pending_utf8()?;
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

    #[test]
    fn terminal_preview_preserves_utf8_split_across_writes() {
        let mut output = Vec::new();
        {
            let mut writer = TerminalSafePreviewWriter::new(&mut output);
            writer.write_all(&[0xe7, 0x95]).unwrap();
            writer.write_all(&[0x8c]).unwrap();
            writer.finish().unwrap();
        }

        assert_eq!(String::from_utf8(output).unwrap(), "界");
    }

    #[test]
    fn terminal_preview_omits_a_utf8_character_split_by_the_bound() {
        let mut source = vec![b'a'; MAX_PEEK_SOURCE_BYTES - 1];
        source.extend_from_slice("界".as_bytes());
        let mut output = Vec::new();
        {
            let mut writer = TerminalSafePreviewWriter::new(&mut output);
            writer.write_all(&source[..MAX_PEEK_SOURCE_BYTES]).unwrap();
            writer.write_all(&source[MAX_PEEK_SOURCE_BYTES..]).unwrap();
            writer.finish().unwrap();
        }

        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.matches('a').count(), MAX_PEEK_SOURCE_BYTES - 1);
        assert!(!output.contains("界"));
        assert!(!output.contains("\\xe7"));
        assert!(output.ends_with("preview limited to 4096 of 4098 source bytes"));
    }

    #[test]
    fn terminal_preview_escapes_incomplete_utf8_when_the_source_ends() {
        let mut output = Vec::new();
        {
            let mut writer = TerminalSafePreviewWriter::new(&mut output);
            writer.write_all(&[0xe7]).unwrap();
            writer.finish().unwrap();
        }

        assert_eq!(String::from_utf8(output).unwrap(), "\\xe7");
    }
}
