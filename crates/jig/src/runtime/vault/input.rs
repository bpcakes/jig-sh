use std::io::{Read, Write};

use anyhow::{Context, Result, bail};
use crossterm::terminal;
use jig_vault::{FieldKind, MAX_SECRET_VALUE_LEN, SecretBytes};
use zeroize::Zeroizing;

use super::lifecycle::{hidden_terminal_input_available, prompt_zeroizing};

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
    let result = read_text_input(&mut std::io::stdin().lock(), &mut output);
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

fn read_text_input(input: &mut impl Read, output: &mut impl Write) -> Result<SecretBytes> {
    let mut value = Zeroizing::new(String::new());
    loop {
        draw_text_input(output, &value)?;
        let mut bytes = [0; 4];
        input
            .read_exact(&mut bytes[..1])
            .context("text field input ended before submission; value was not saved")?;
        match bytes[0] {
            b'\r' | b'\n' => return Ok(SecretBytes::new(std::mem::take(&mut *value).into_bytes())),
            3 | 4 | 27 => bail!("text field input cancelled; value was not saved"),
            8 | 127 => {
                value.pop();
            }
            21 => value.clear(),
            b'\t' => append_text(&mut value, '\t')?,
            0..=31 => {}
            first => {
                let width = match first {
                    0..=127 => 1,
                    0xc2..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf4 => 4,
                    _ => bail!("text field input is not valid UTF-8; value was not saved"),
                };
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
        for mut bytes in [b"".as_slice(), b"partial", b"partial\x04", b"\xc3"] {
            assert!(read_text_input(&mut bytes, &mut Vec::new()).is_err());
        }
        for (bytes, expected) in [("\n", ""), (" café  \x7f\r", " café ")] {
            assert_eq!(
                read_text_input(&mut bytes.as_bytes(), &mut Vec::new())
                    .unwrap()
                    .as_slice(),
                expected.as_bytes()
            );
        }
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
