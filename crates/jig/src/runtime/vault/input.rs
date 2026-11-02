use std::io::{BufReader, Read, Write};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use crossterm::terminal;
use jig_vault::{FieldKind, MAX_SECRET_VALUE_LEN, SecretBytes};
use zeroize::Zeroizing;

use super::lifecycle::{hidden_terminal_input_available, prompt_zeroizing};

mod terminal_input;

pub(super) fn read_field_value_from_prompt(kind: FieldKind) -> Result<SecretBytes> {
    if !hidden_terminal_input_available() {
        bail!("--value-prompt requires an interactive terminal; use --value-stdin for automation");
    }
    if kind == FieldKind::Concealed {
        let mut value = prompt_zeroizing("Field value: ")
            .context("failed to read field value from terminal")?;
        return Ok(SecretBytes::new(std::mem::take(&mut *value).into_bytes()));
    }
    // Canonical terminal buffers discard long input before an ordinary read
    // can see it. Raw mode makes submission, cancellation, and limits explicit.
    terminal::enable_raw_mode().context("failed to prepare text field input")?;
    let guard = RawInput;
    let mut output = std::io::stderr().lock();
    let result = terminal_input::read(&mut output);
    let restored = terminal::disable_raw_mode();
    drop(guard);
    writeln!(output)?;
    restored.context("failed to restore terminal after text field input")?;
    result
}

struct RawInput;
impl Drop for RawInput {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

fn read_text_input(
    input: &mut BufReader<impl Read>,
    output: &mut impl Write,
    mut wait: impl FnMut(Duration) -> std::io::Result<bool>,
) -> Result<SecretBytes> {
    let mut value = Zeroizing::new(String::new());
    draw_text_input(output, &value)?;
    loop {
        let mut bytes = [0; 4];
        input
            .read_exact(&mut bytes[..1])
            .context("text field input ended before submission; value was not saved")?;
        match bytes[0] {
            b'\r' | b'\n' => {
                draw_text_input(output, &value)?;
                return Ok(SecretBytes::new(std::mem::take(&mut *value).into_bytes()));
            }
            3 | 4 => bail!("text field input cancelled; value was not saved"),
            27 => terminal_input::skip_escape(input, &mut wait)?,
            8 | 127 => {
                value.pop();
            }
            21 => value.clear(),
            b'\t' => append_text(&mut value, '\t')?,
            0..=31 => {}
            first => {
                let width = utf8_width(first)?;
                input
                    .read_exact(&mut bytes[1..width])
                    .context("text field input ended before submission; value was not saved")?;
                let text = std::str::from_utf8(&bytes[..width])
                    .context("text field input is not valid UTF-8; value was not saved")?;
                append_text(
                    &mut value,
                    text.chars().next().expect("nonempty UTF-8 character"),
                )?;
            }
        }
        // One redraw per buffered batch, including the last typed character
        // before blocking for more input. Do not fill the buffer just to decide
        // whether to draw: that could block with the newest input still hidden.
        if input.buffer().is_empty() {
            draw_text_input(output, &value)?;
        }
    }
}

fn utf8_width(first: u8) -> Result<usize> {
    match first {
        0..=127 => Ok(1),
        0xc2..=0xdf => Ok(2),
        0xe0..=0xef => Ok(3),
        0xf0..=0xf4 => Ok(4),
        _ => bail!("text field input is not valid UTF-8; value was not saved"),
    }
}

fn append_text(value: &mut String, character: char) -> Result<()> {
    if value.len() + character.len_utf8() > MAX_SECRET_VALUE_LEN {
        bail!("text field value exceeds the maximum length; value was not saved");
    }
    value.push(character);
    Ok(())
}

fn draw_text_input(output: &mut impl Write, value: &str) -> Result<()> {
    let width = terminal::size().map_or(80, |(width, _)| width);
    // Reserve two cells per Unicode scalar so even wide characters stay on
    // this row. Keeping only a bounded tail also bounds redraw work on paste.
    let tail_len = usize::from(width.saturating_sub(16) / 2);
    let tail: String = value.chars().rev().take(tail_len).collect();
    let tail: String = tail.chars().rev().collect();
    write!(
        output,
        "\r\x1b[2KField value: {}",
        jig_tui::sanitize_text(&tail)
    )?;
    output.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_input_requires_submission_and_preserves_empty_and_unicode() {
        for bytes in [
            b"".as_slice(),
            b"partial",
            b"partial\x04",
            b"\xc3",
            b"partial\x1b",
        ] {
            assert!(
                read_text_input(&mut BufReader::new(bytes), &mut Vec::new(), |_| Ok(false))
                    .is_err()
            );
        }
        for (bytes, expected) in [("\n", ""), (" café  \x7f\r", " café ")] {
            assert_eq!(
                read_text_input(
                    &mut BufReader::new(bytes.as_bytes()),
                    &mut Vec::new(),
                    |_| Ok(false)
                )
                .unwrap()
                .as_slice(),
                expected.as_bytes()
            );
        }
    }

    #[test]
    fn navigation_and_alt_sequences_are_consumed_across_buffer_boundaries() {
        let bytes = "start\x1b[D\x1b[3~\x1bOH\x1bOF\x1b[1;5C\x1bx\x1béend\n".as_bytes();
        for capacity in [1, 2, 4, 4096] {
            let value = read_text_input(
                &mut BufReader::with_capacity(capacity, bytes),
                &mut Vec::new(),
                |_| Ok(true),
            )
            .unwrap();
            assert_eq!(value.as_slice(), b"startend");
        }
    }

    #[test]
    fn alt_sequence_prefixes_timeout_without_cancelling() {
        use std::io::BufRead;
        for prefix in [b'[', b'O'] {
            let bytes = [prefix];
            let mut input = BufReader::new(bytes.as_slice());
            // Feed the prefix through the same explicit buffer as production.
            input.fill_buf().unwrap();
            assert!(terminal_input::skip_escape(&mut input, &mut |_| Ok(false)).is_ok());
        }
        let mut bare_escape = BufReader::new(b"".as_slice());
        assert!(terminal_input::skip_escape(&mut bare_escape, &mut |_| Ok(false)).is_err());
    }

    #[test]
    fn maximum_paste_preserves_utf8_with_bounded_redraws() {
        let value = "é".repeat(MAX_SECRET_VALUE_LEN / 2);
        let input = format!("{value}\n");
        let mut output = Vec::new();
        let actual = read_text_input(
            &mut BufReader::with_capacity(4096, input.as_bytes()),
            &mut output,
            |_| Ok(false),
        )
        .unwrap();
        assert_eq!(actual.as_slice(), value.as_bytes());
        let redraws = output
            .windows(b"Field value:".len())
            .filter(|window| *window == b"Field value:")
            .count();
        assert!(
            redraws <= MAX_SECRET_VALUE_LEN / 4096 + 2,
            "{redraws} redraws"
        );
    }

    #[test]
    fn text_input_bound_counts_utf8_bytes_without_truncating_a_character() {
        let mut value = "a".repeat(MAX_SECRET_VALUE_LEN - 2);
        append_text(&mut value, 'é').unwrap();
        assert_eq!(value.len(), MAX_SECRET_VALUE_LEN);
        assert!(append_text(&mut value, 'x').is_err());
        assert!(value.ends_with('é'));
    }
}
